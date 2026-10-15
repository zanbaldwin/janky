export type Severity = "error" | "warning" | "info";

export interface Diagnostic {
  file: string | null;
  line: number | null;
  column: number | null;
  end_line: number | null;
  end_column: number | null;
  severity: Severity;
  code: string;
  message: string;
  help: string | null;
  related: Diagnostic[];
}

export interface Row {
  row: number;
  notetype: { key: string; name: string };
  /** `null` for the sample note of a note type that no entry uses. */
  /** `index` counts from 0 in the `entries` of the file, also the entries with errors. */
  entry: { id: string; file: string; index: number | null } | null;
  /** `null` for the sample note of a note type that no entry uses. */
  deck: { key: string; full_name: string } | null;
  card: { key: string; name: string; ord: number };
  /** The front is empty, so Anki does not make this card. */
  empty: boolean;
  error: string | null;
}

export interface Preview {
  /** Random hex. Card and media URLs of an older generation get `410 Gone`. */
  generation: string;
  /** The project did not load, so `rows` are from an earlier page load. */
  stale: boolean;
  diagnostics: Diagnostic[];
  rows: Row[];
}

export type Side = "front" | "back";

export type Platform = "desktop" | "android" | "iphone";

export interface View {
  side: Side;
  night: boolean;
  platform: Platform;
}

export async function fetchPreview(signal?: AbortSignal): Promise<Preview> {
  const response = await fetch("/api/preview", { cache: "no-store", signal });
  if (!response.ok) throw new Error(`/api/preview answered ${response.status}`);
  return (await response.json()) as Preview;
}

/** The URL of the card document. `typed` is the typed answer, which only the back uses. */
export function cardUrl(generation: string, row: number, view: View, typed: string): string {
  const query = new URLSearchParams({ night: view.night ? "1" : "0", platform: view.platform });
  if (view.side === "back" && typed !== "") query.set("typed", typed);
  return `/card/${generation}/${row}/${view.side}?${query}`;
}
