//! The errors and warnings that `janky` collects, and their output.
//!
//! A [`Diagnostic`] has a code, a message, an optional help text and labels that point into a
//! [`Source`]. [`Diagnostics`] collects them, so that a command can report every problem at
//! once. It renders them for the terminal through `miette`, or writes them as JSON Lines for
//! `--json`.
//!
//! Columns count characters, not bytes. [`Source`] implements `miette`'s [`SourceCode`] for this
//! reason, so that multi-byte text such as emoji before an error does not move the column.
//!
//! Text from the author or from a plugin can contain control characters and secrets. Text that
//! goes to the terminal passes through [`visible`], and JSON passes through [`json_text`]. Both
//! escape control characters and bidirectional formatting characters, and both replace each
//! secret registered with [`redact`] by a placeholder.
//!
//! Source snippets do not pass through [`visible`]. They show control characters and
//! bidirectional formatting characters as `?` (see [`Source`]), and they are not redacted.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use indexmap::IndexMap;
use miette::{
    GraphicalReportHandler, LabeledSpan, MietteError, MietteSpanContents, SourceCode, SourceSpan, SpanContents,
};
use serde::Serialize;

/// How serious a [`Diagnostic`] is. The JSON output writes it in lower case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The command exits with a failure status. See [`Diagnostics::has_errors`].
    Error,
    /// It does not change the exit status.
    Warning,
    /// It does not change the exit status. The terminal shows it in full only with
    /// `--verbose`. See [`Diagnostics::render`].
    Info,
}

/// A position in a [`Source`]. Both numbers count from 1, and the column counts characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// The line, from 1.
    pub line: usize,
    /// The column, from 1, in characters.
    pub column: usize,
}

/// The text of a file and its name, for diagnostics that point into it.
///
/// Control characters and bidirectional formatting characters in the text show as `?` in
/// rendered snippets. Each one becomes as many `?` as it has UTF-8 bytes, so byte offsets into
/// the text stay valid.
#[derive(Debug)]
pub struct Source {
    name: String,
    text: String,
    display: Option<String>,
}

fn hidden(c: char) -> bool {
    (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        || matches!(c, '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

static REDACTIONS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// Registers a secret, so that [`visible`] and [`json_text`] replace it with `<secret:name>`.
///
/// It registers the raw value and its JSON-escaped form. An empty value does nothing. The
/// registry is global for the process, and longer values are replaced first.
pub fn redact(value: &str, name: &str) {
    if value.is_empty() {
        return;
    }
    let mut redactions = REDACTIONS.lock().expect("redactions");
    let label = format!("<secret:{name}>");
    let escaped = serde_json::to_string(value).unwrap_or_default();
    let escaped = escaped.trim_matches('"').to_string();
    for form in [value.to_string(), escaped] {
        if !redactions.iter().any(|(known, _)| *known == form) {
            redactions.push((form, label.clone()));
        }
    }
    redactions.sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
}

/// Whether `text` holds a secret registered with [`redact`], as it is or JSON-escaped.
pub fn holds_secret(text: &str) -> bool {
    REDACTIONS.lock().expect("redactions").iter().any(|(value, _)| text.contains(value.as_str()))
}

/// Replaces each secret registered with [`redact`] by its placeholder. No characters are
/// escaped.
pub fn redacted(text: &str) -> String {
    let redactions = REDACTIONS.lock().expect("redactions");
    let mut text = text.to_string();
    for (value, label) in redactions.iter() {
        if text.contains(value.as_str()) {
            text = text.replace(value.as_str(), label);
        }
    }
    text
}

/// Makes text safe to print on a terminal.
///
/// It replaces secrets as [`redacted`] does. Then it writes control characters (except tab and
/// new line), carriage returns and bidirectional formatting characters as `\u{..}` escapes.
pub fn visible(text: &str) -> String {
    let text = redacted(text);
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if hidden(c) || c == '\r' {
            out.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// Serialises a JSON value to one line, with secrets replaced as [`redacted`] does.
///
/// It also writes control characters and bidirectional formatting characters as `\uXXXX`
/// escapes. `serde_json` leaves C1 control characters and bidirectional characters as they are.
pub fn json_text(value: &serde_json::Value) -> String {
    redacted(&value.to_string())
        .chars()
        .map(|c| if hidden(c) { format!("\\u{:04x}", u32::from(c)) } else { c.to_string() })
        .collect()
}

impl Source {
    /// Makes a shared source. The name goes through [`visible`], so it is safe to print.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Arc<Self> {
        let text = text.into();
        let display = text
            .contains(hidden)
            .then(|| text.chars().map(|c| if hidden(c) { "?".repeat(c.len_utf8()) } else { c.to_string() }).collect());
        Arc::new(Self {
            name: visible(&name.into()),
            text,
            display,
        })
    }

    /// The name, already passed through [`visible`].
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The original text, with no characters replaced.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The line and column of a byte offset into the text. The column counts characters.
    ///
    /// It fails if `offset` is past the end of the text.
    pub fn location(&self, offset: usize) -> Result<Location, MietteError> {
        let contents = self.read_span(&SourceSpan::from(offset..offset), 0, 0)?;
        Ok(Location {
            line: contents.line() + 1,
            column: contents.column() + 1,
        })
    }
}

impl SourceCode for Source {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
        let shown = self.display.as_deref().unwrap_or(&self.text);
        let inner = shown.read_span(span, context_lines_before, context_lines_after)?;
        let offset = span.offset();
        let column = offset
            .checked_sub(inner.column())
            .and_then(|line_start| self.text.get(line_start..offset))
            .map_or(inner.column(), |prefix| prefix.chars().count());
        Ok(Box::new(MietteSpanContents::new_named(
            self.name.clone(),
            inner.data(),
            *inner.span(),
            inner.line(),
            column,
            inner.line_count(),
        )))
    }
}

/// A span in the [`Source`] of a [`Diagnostic`], with the text to show next to it.
#[derive(Debug, Clone)]
pub struct Label {
    /// Byte offsets into [`Source::text`].
    pub span: Range<usize>,
    /// The text next to the span. It goes through [`visible`] when it is rendered.
    pub text: Option<String>,
}

/// One error or warning, with the file and the spans that it points to.
///
/// Make one with [`error`](Self::error) or [`warning`](Self::warning) and the `with_` methods.
/// Its `Display` output is the message, passed through [`visible`].
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Whether the problem is an error or a warning.
    pub severity: Severity,
    /// A code such as `content::type` that names the kind of problem.
    pub code: String,
    /// The main text. It is not redacted or escaped until it is rendered or written as JSON.
    pub message: String,
    /// Advice on how to fix the problem.
    pub help: Option<String>,
    /// The file that the [`labels`](Self::labels) point into. `None` if the problem is not in one
    /// file.
    pub source: Option<Arc<Source>>,
    /// The spans to show. The first one is the primary span. Its position goes into the JSON
    /// output.
    pub labels: Vec<Label>,
    /// Further diagnostics that belong to this one. They show below it.
    pub related: Vec<Diagnostic>,
}

impl Diagnostic {
    /// Makes a diagnostic with no help, source or labels.
    pub fn new(severity: Severity, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            help: None,
            source: None,
            labels: Vec::new(),
            related: Vec::new(),
        }
    }

    /// Shorthand for [`new`](Self::new) with [`Severity::Error`].
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    /// Shorthand for [`new`](Self::new) with [`Severity::Warning`].
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    /// Shorthand for [`new`](Self::new) with [`Severity::Info`].
    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, code, message)
    }

    /// Sets the help text. It replaces an earlier one.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Sets the file that the labels point into.
    pub fn with_source(mut self, source: Arc<Source>) -> Self {
        self.source = Some(source);
        self
    }

    /// Adds a span with a text. The first label that you add is the primary span.
    pub fn with_label(mut self, span: Range<usize>, text: impl Into<String>) -> Self {
        self.labels.push(Label { span, text: Some(text.into()) });
        self
    }

    /// Adds a diagnostic that shows below this one.
    pub fn with_related(mut self, related: Diagnostic) -> Self {
        self.related.push(related);
        self
    }

    /// The JSON object of one JSON Lines record.
    ///
    /// `line`, `column`, `end_line` and `end_column` come from the primary span and count from 1.
    /// They are `null` if there is no source or no label. This object is not redacted or escaped:
    /// [`Diagnostics::to_json_lines`] does that through [`json_text`].
    pub fn to_json(&self) -> serde_json::Value {
        let primary = self.labels.first();
        let start = self.position(primary.map(|label| label.span.start));
        let end = self.position(primary.map(|label| label.span.end));
        serde_json::json!({
            "file": self.source.as_ref().map(|source| source.name()),
            "line": start.map(|location| location.line),
            "column": start.map(|location| location.column),
            "end_line": end.map(|location| location.line),
            "end_column": end.map(|location| location.column),
            "severity": self.severity,
            "code": self.code,
            "message": self.message,
            "help": self.help,
            "related": self.related.iter().map(Diagnostic::to_json).collect::<Vec<_>>(),
        })
    }

    fn position(&self, offset: Option<usize>) -> Option<Location> {
        self.source.as_ref()?.location(offset?).ok()
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&visible(&self.message))
    }
}

impl std::error::Error for Diagnostic {}

impl miette::Diagnostic for Diagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(&self.code))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(match self.severity {
            Severity::Error => miette::Severity::Error,
            Severity::Warning => miette::Severity::Warning,
            Severity::Info => miette::Severity::Advice,
        })
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.help.as_ref().map(|help| Box::new(visible(help)) as Box<dyn fmt::Display>)
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.source.as_deref().map(|source| source as &dyn SourceCode)
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn miette::Diagnostic> + 'a>> {
        if self.related.is_empty() {
            return None;
        }
        Some(Box::new(self.related.iter().map(|diagnostic| diagnostic as &dyn miette::Diagnostic)))
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        if self.labels.is_empty() {
            return None;
        }
        Some(Box::new(
            self.labels
                .iter()
                .map(|label| LabeledSpan::new_with_span(label.text.as_deref().map(visible), label.span.clone())),
        ))
    }
}

/// All diagnostics of a command, in the order they were added.
#[derive(Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Adds a diagnostic at the end.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// Sorts `other` by file name and primary span start, then adds it at the end.
    ///
    /// Diagnostics without a source come first, and in each file those without a label come
    /// first. The sort is stable, so diagnostics with the same key keep their order.
    pub fn append_sorted(&mut self, mut other: Diagnostics) {
        other.items.sort_by_key(|diagnostic| {
            let name = diagnostic.source.as_ref().map(|source| source.name().to_string());
            (name, diagnostic.labels.first().map(|label| label.span.start))
        });
        self.items.append(&mut other.items);
    }

    /// The diagnostics in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    /// Whether there are no diagnostics at all, warnings included.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Whether any diagnostic is an error. Warnings do not count.
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    /// Renders each diagnostic for the terminal with `handler`, one after the other.
    ///
    /// Without `verbose`, info diagnostics do not show in full. Each info code gets one line
    /// with its count at the end, in the order of the first diagnostic with that code. It fails
    /// only if `handler` fails to write.
    pub fn render(&self, handler: &GraphicalReportHandler, verbose: bool) -> Result<String, fmt::Error> {
        let mut out = String::new();
        let mut hidden: IndexMap<&str, usize> = IndexMap::new();
        for diagnostic in &self.items {
            if diagnostic.severity == Severity::Info && !verbose {
                *hidden.entry(diagnostic.code.as_str()).or_default() += 1;
            } else {
                handler.render_report(&mut out, diagnostic)?;
            }
        }
        for (code, count) in hidden {
            out.push_str(&format!("info: {count} × {code} (run with --verbose to see them)\n"));
        }
        Ok(out)
    }

    /// One line of JSON for each diagnostic, for `--json`. Secrets are redacted and control
    /// characters are escaped.
    pub fn to_json_lines(&self) -> String {
        self.items.iter().map(|diagnostic| format!("{}\n", json_text(&diagnostic.to_json()))).collect()
    }
}

#[cfg(test)]
mod tests {
    use miette::GraphicalTheme;

    use super::*;

    fn span_of(text: &str, needle: &str) -> Range<usize> {
        let start = text.find(needle).unwrap();
        start..start + needle.len()
    }

    fn header(diagnostic: Diagnostic) -> String {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(diagnostic);
        let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor());
        let rendered = diagnostics.render(&handler, false).unwrap();
        let start = rendered.find('[').unwrap();
        let end = rendered[start..].find(']').unwrap();
        rendered[start..=start + end].to_string()
    }

    #[test]
    fn emoji_before_the_error_on_the_same_line() {
        let text = "name: ⚛️ 🧪 👩‍🔬 🇬🇧, level: x\n";
        let source = Source::new("lesson.yaml", text);
        let span = span_of(text, "x");
        assert_eq!(source.location(span.start).unwrap(), Location { line: 1, column: 27 });
        let diagnostic = Diagnostic::error("schema::type", "wrong type").with_source(source).with_label(span, "here");
        assert_eq!(header(diagnostic), "[lesson.yaml:1:27]");
    }

    #[test]
    fn emoji_on_earlier_lines() {
        let text = "- symbol: ⚛️\n  name: 👩‍🔬\n  level: x\n";
        let source = Source::new("lesson.yaml", text);
        let span = span_of(text, "x");
        assert_eq!(source.location(span.start).unwrap(), Location { line: 3, column: 10 });
        let diagnostic = Diagnostic::error("schema::type", "wrong type").with_source(source).with_label(span, "here");
        assert_eq!(header(diagnostic), "[lesson.yaml:3:10]");
    }

    #[test]
    fn emoji_on_earlier_lines_and_the_same_line() {
        let text = "symbol = \"⚛️\"\nname = \"👩‍🔬\" # 🇬🇧\nlevel = 🧪\n";
        let source = Source::new("lesson.toml", text);
        let span = span_of(text, "🧪");
        assert_eq!(source.location(span.start).unwrap(), Location { line: 3, column: 9 });
        let end = source.location(span.end).unwrap();
        assert_eq!(end, Location { line: 3, column: 10 });
    }

    #[test]
    fn other_multi_byte_text() {
        let text = "{\"note\": \"café 🙂 naïve\", \"level\": x}";
        let source = Source::new("lesson.json", text);
        let span = span_of(text, "x");
        assert_eq!(source.location(span.start).unwrap(), Location { line: 1, column: 35 });
        let diagnostic = Diagnostic::error("schema::type", "wrong type").with_source(source).with_label(span, "here");
        assert_eq!(header(diagnostic), "[lesson.json:1:35]");
    }

    #[test]
    fn location_at_end_of_file() {
        let text = "symbol: 👩‍🔬";
        let source = Source::new("lesson.yaml", text);
        assert_eq!(source.location(text.len()).unwrap(), Location { line: 1, column: 12 });
    }

    #[test]
    fn json_lines() {
        let text = "- symbol: 🧪\n  level: x\n";
        let source = Source::new("lesson.yaml", text);
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(
            Diagnostic::error("schema::type", "\"x\" is not of type \"integer\"")
                .with_help("use a number from 1 to 6")
                .with_source(source.clone())
                .with_label(span_of(text, "x"), "here"),
        );
        diagnostics.push(Diagnostic::warning("media::missing", "missing generated file").with_source(source));
        assert!(diagnostics.has_errors());
        assert_eq!(
            diagnostics.to_json_lines(),
            concat!(
                r#"{"file":"lesson.yaml","line":2,"column":10,"end_line":2,"end_column":11,"severity":"error","code":"schema::type","message":"\"x\" is not of type \"integer\"","help":"use a number from 1 to 6","related":[]}"#,
                "\n",
                r#"{"file":"lesson.yaml","line":null,"column":null,"end_line":null,"end_column":null,"severity":"warning","code":"media::missing","message":"missing generated file","help":null,"related":[]}"#,
                "\n",
            )
        );
    }

    #[test]
    fn info_shows_as_one_line_for_each_code_unless_verbose() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(Diagnostic::info("enrich::warning", "dict: Unobtainium: not in the dictionary"));
        diagnostics.push(Diagnostic::warning("media::missing", "missing generated file"));
        diagnostics.push(Diagnostic::info("enrich::warning", "dict: Adamantium: not in the dictionary"));
        assert!(!diagnostics.has_errors());
        let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor());

        let short = diagnostics.render(&handler, false).unwrap();
        assert!(short.contains("missing generated file"));
        assert!(!short.contains("Unobtainium"));
        assert!(short.ends_with("info: 2 × enrich::warning (run with --verbose to see them)\n"));

        let verbose = diagnostics.render(&handler, true).unwrap();
        assert!(verbose.contains("Unobtainium") && verbose.contains("Adamantium"));
        assert!(!verbose.contains("--verbose"));
        assert!(diagnostics.to_json_lines().contains(r#""severity":"info","code":"enrich::warning""#));
    }

    #[test]
    fn control_characters_are_shown_not_sent() {
        let text = "x = \"\u{1b}[2J\"\nlevel = 🧪\n";
        let source = Source::new("evil\u{1b}]0;title\u{7}.toml", text);
        let span = span_of(text, "🧪");
        assert_eq!(source.location(span.start).unwrap(), Location { line: 2, column: 9 });
        let diagnostic = Diagnostic::error("x::y", "bad \u{1b}[31mred")
            .with_help("help \u{9b}2J")
            .with_source(source)
            .with_label(span, "label \u{1b}c");
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(diagnostic);
        let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor());
        let rendered = diagnostics.render(&handler, false).unwrap();
        assert!(!rendered.contains(['\u{1b}', '\u{7}', '\u{9b}']), "{rendered:?}");
        assert!(rendered.contains("bad \\u{1b}[31mred"));
        assert!(rendered.contains("help \\u{9b}2J"));
        assert!(rendered.contains("x = \"?[2J\""));
        assert_eq!(visible("tab\tand\nnew line"), "tab\tand\nnew line");
        redact("tok\"en-value-123", "api_key");
        assert_eq!(visible("key tok\"en-value-123 here"), "key <secret:api_key> here");
        let echoed = json_text(&serde_json::json!({ "message": "echo tok\"en-value-123" }));
        assert_eq!(echoed, r#"{"message":"echo <secret:api_key>"}"#);
        let json = json_text(&serde_json::json!({ "message": "a\u{9b}b\u{202e}c\u{1b}" }));
        assert_eq!(json, r#"{"message":"a\u009bb\u202ec\u001b"}"#);
        assert_eq!(visible("a\rb\u{202e}c"), "a\\u{d}b\\u{202e}c");
    }
}
