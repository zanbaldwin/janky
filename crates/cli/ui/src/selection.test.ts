import { describe, expect, it } from "vitest";
import type { Row } from "./api";
import { filterRows, formatHash, parseHash } from "./selection";

function row(row: number, entry: string | null, card = "Recognition"): Row {
  return {
    row,
    notetype: { key: "vocab", name: "Vocabulary" },
    entry: entry === null ? null : { id: entry, file: "content/words.yaml", index: row },
    deck: entry === null ? null : { key: "words", full_name: "日本語::Words" },
    card: { key: card.toLowerCase(), name: card, ord: 0 },
    empty: false,
    error: null,
  };
}

describe("parseHash", () => {
  const start = { row: 0, side: "front", night: false, platform: "desktop" } as const;

  it("reads the selection that formatHash writes", () => {
    const selection = { row: 12, side: "back", night: true, platform: "android" } as const;
    expect(parseHash(formatHash(selection))).toEqual(selection);
  });

  it("falls back to the first row, the front, day mode and desktop for a missing or bad hash", () => {
    expect(parseHash("")).toEqual(start);
    expect(parseHash("#row=-1&side=sideways&night=yes&platform=symbian")).toEqual(start);
    expect(parseHash("#row=abc")).toEqual(start);
  });
});

describe("filterRows", () => {
  const rows = [row(0, "hello"), row(1, "goodbye", "Production"), row(2, null)];

  it("keeps every row for an empty filter", () => {
    expect(filterRows(rows, "  ")).toBe(rows);
  });

  it("matches the entry, the card and the deck without regard to case", () => {
    expect(filterRows(rows, "HELLO").map((row) => row.row)).toEqual([0]);
    expect(filterRows(rows, "product").map((row) => row.row)).toEqual([1]);
    expect(filterRows(rows, "日本語").map((row) => row.row)).toEqual([0, 1]);
  });

  it("matches a sample row by its note type", () => {
    expect(filterRows(rows, "vocabulary").map((row) => row.row)).toEqual([0, 1, 2]);
  });
});
