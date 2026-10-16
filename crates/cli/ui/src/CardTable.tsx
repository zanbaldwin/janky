import type { KeyboardEvent } from "react";
import type { Row } from "./api";

interface Props {
  rows: Row[];
  selected: number;
  onSelect: (row: number) => void;
}

function source(entry: NonNullable<Row["entry"]>): string {
  return entry.index === null ? entry.file : `${entry.file} #${entry.index}`;
}

export function CardTable({ rows, selected, onSelect }: Props) {
  function onKeyDown(event: KeyboardEvent, row: number) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect(row);
    }
  }

  return (
    <table className="cards">
      <thead>
        <tr>
          <th>Deck</th>
          <th>Entry</th>
          <th>Source</th>
          <th>Note type</th>
          <th>Card</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr
            key={row.row}
            tabIndex={0}
            aria-selected={row.row === selected}
            className={row.entry === null ? "sample" : undefined}
            onClick={() => onSelect(row.row)}
            onKeyDown={(event) => onKeyDown(event, row.row)}
          >
            <td>{row.deck?.full_name ?? "N/A"}</td>
            <td>{row.entry?.id ?? "N/A"}</td>
            <td>{row.entry === null ? "N/A" : source(row.entry)}</td>
            <td title={row.notetype.key}>{row.notetype.name}</td>
            <td title={row.card.key}>
              {row.card.name}
              {row.empty && <span className="badge">empty</span>}
              {row.error !== null && <span className="badge error">error</span>}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
