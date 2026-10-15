// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { MIN_CARD, MIN_TABLE, clampWidth, loadWidth, saveWidth } from "./width";

describe("clampWidth", () => {
  it("keeps a width that leaves room for both panes", () => {
    expect(clampWidth(500.4, 1400)).toBe(500);
  });

  it("keeps the table at its smallest width", () => {
    expect(clampWidth(10, 1400)).toBe(MIN_TABLE);
  });

  it("leaves the smallest width for the card", () => {
    expect(clampWidth(1390, 1400)).toBe(1400 - MIN_CARD);
  });
});

describe("loadWidth", () => {
  it("reads the width that saveWidth wrote", () => {
    saveWidth(612);
    expect(loadWidth()).toBe(612);
  });

  it("gives null for a missing or bad value", () => {
    window.localStorage.setItem("janky-preview-table-width", "wide");
    expect(loadWidth()).toBeNull();
  });
});
