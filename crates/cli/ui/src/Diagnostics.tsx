import type { Diagnostic } from "./api";

function location(diagnostic: Diagnostic): string | null {
  if (diagnostic.file === null) return null;
  if (diagnostic.line === null) return diagnostic.file;
  return `${diagnostic.file}:${diagnostic.line}:${diagnostic.column ?? 1}`;
}

function Item({ diagnostic }: { diagnostic: Diagnostic }) {
  const where = location(diagnostic);
  return (
    <li className={`diagnostic ${diagnostic.severity}`}>
      <div>
        <span className="severity">{diagnostic.severity}</span> <code>{diagnostic.code}</code>
        {where !== null && <span className="where"> {where}</span>}
      </div>
      <div>{diagnostic.message}</div>
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
  const errors = diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  return (
    <details className="diagnostics" open={errors > 0}>
      <summary>
        {errors} {errors === 1 ? "error" : "errors"}, {diagnostics.length - errors} other
      </summary>
      <ul>
        {diagnostics.map((diagnostic, index) => (
          <Item key={index} diagnostic={diagnostic} />
        ))}
      </ul>
    </details>
  );
}
