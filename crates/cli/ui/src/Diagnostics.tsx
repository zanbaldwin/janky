import type { Diagnostic, Severity } from "./api";

const SEVERITIES: { severity: Severity; one: string; many: string }[] = [
  { severity: "error", one: "error", many: "errors" },
  { severity: "warning", one: "warning", many: "warnings" },
  { severity: "info", one: "info", many: "info" },
];

function location(diagnostic: Diagnostic): string | null {
  if (diagnostic.file === null) return null;
  if (diagnostic.line === null) return diagnostic.file;
  return `${diagnostic.file}:${diagnostic.line}:${diagnostic.column ?? 1}`;
}

function Item({ diagnostic }: { diagnostic: Diagnostic }) {
  const where = location(diagnostic);
  return (
    <li className={`diagnostic ${diagnostic.severity}`}>
      <div className="message">{diagnostic.message}</div>
      <div className="meta">
        <span className="severity">{diagnostic.severity}</span> · {diagnostic.code}
        {where !== null && <> · {where}</>}
      </div>
      {diagnostic.help !== null && <div className="help">help: {diagnostic.help}</div>}
      {diagnostic.related.length > 0 && (
        <ul>
          {diagnostic.related.map((related, index) => (
            <Item key={index} diagnostic={related} />
          ))}
        </ul>
      )}
    </li>
  );
}

export function Diagnostics({ diagnostics }: { diagnostics: Diagnostic[] }) {
  if (diagnostics.length === 0) return null;
  const count = (severity: Severity) => diagnostics.filter((diagnostic) => diagnostic.severity === severity).length;
  return (
    <details className="diagnostics" open={count("error") > 0}>
      <summary>
        <svg className="icon chevron" viewBox="0 0 24 24" aria-hidden="true">
          <path d="m9 6 6 6-6 6" />
        </svg>
        {SEVERITIES.filter(({ severity }) => count(severity) > 0).map(({ severity, one, many }) => (
          <span key={severity} className={`chip ${severity}`}>
            {count(severity)} {count(severity) === 1 ? one : many}
          </span>
        ))}
      </summary>
      <ul>
        {diagnostics.map((diagnostic, index) => (
          <Item key={index} diagnostic={diagnostic} />
        ))}
      </ul>
    </details>
  );
}
