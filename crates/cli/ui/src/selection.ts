import type { Platform, Row, View } from "./api";

export interface Selection extends View {
  row: number;
}

const PLATFORMS: Platform[] = ["desktop", "android", "iphone"];

/** Reads the selection from a URL hash such as `#row=3&side=back&night=1&platform=android`, so that a reload keeps
 * it. */
export function parseHash(hash: string): Selection {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const row = Number.parseInt(params.get("row") ?? "", 10);
  const platform = PLATFORMS.find((platform) => platform === params.get("platform"));
  return {
    row: Number.isInteger(row) && row >= 0 ? row : 0,
    side: params.get("side") === "back" ? "back" : "front",
    night: params.get("night") !== "0",
    platform: platform ?? "desktop",
  };
}

export function formatHash(selection: Selection): string {
  const night = selection.night ? "1" : "0";
  return `#row=${selection.row}&side=${selection.side}&night=${night}&platform=${selection.platform}`;
}

/** The rows whose note type, entry, deck or card contains `text`, ignoring case. */
export function filterRows(rows: Row[], text: string): Row[] {
  const needle = text.trim().toLowerCase();
  if (needle === "") return rows;
  return rows.filter((row) =>
    [
      row.notetype.key,
      row.notetype.name,
      row.entry?.id,
      row.entry?.file,
      row.deck?.full_name,
      row.card.key,
      row.card.name,
    ].some((value) => value?.toLowerCase().includes(needle)),
  );
}
