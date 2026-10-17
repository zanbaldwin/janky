// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { isFlipKey as isCardFlipKey } from "./card/flip";
import { isFlipKey as isPageFlipKey } from "./flip";

type Check = (event: KeyboardEvent) => boolean;

function flipsWith(isFlipKey: Check, target: EventTarget, init: KeyboardEventInit = {}): boolean {
  let result = false;
  document.addEventListener("keydown", (event) => (result = isFlipKey(event)), { once: true });
  target.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true, ...init }));
  return result;
}

function add(html: string): HTMLElement {
  document.body.innerHTML = html;
  return document.body.firstElementChild as HTMLElement;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe.each([
  ["page", isPageFlipKey],
  ["card", isCardFlipKey],
])("isFlipKey of the %s", (_, isFlipKey) => {
  const flips = (target: EventTarget, init?: KeyboardEventInit) => flipsWith(isFlipKey, target, init);

  it("flips on Space in the page", () => {
    expect(flips(document.body)).toBe(true);
  });

  it("does not flip on another key, a repeat, or with a modifier", () => {
    expect(flips(document.body, { key: "Enter" })).toBe(false);
    expect(flips(document.body, { repeat: true })).toBe(false);
    expect(flips(document.body, { shiftKey: true })).toBe(false);
    expect(flips(document.body, { ctrlKey: true })).toBe(false);
  });

  it("leaves Space to a field that takes text", () => {
    expect(flips(add('<input id="typeans">'))).toBe(false);
    expect(flips(add("<textarea></textarea>"))).toBe(false);
    expect(flips(add("<select></select>"))).toBe(false);
  });

  it("leaves Space to a button that the keyboard focused", () => {
    const button = add("<button>Back</button>");
    expect(flips(button)).toBe(true);
    button.focus();
    expect(flips(button)).toBe(false);
  });

  it("does not flip when a handler took the key", () => {
    const row = add("<div></div>");
    row.addEventListener("keydown", (event) => event.preventDefault());
    expect(flips(row)).toBe(false);
  });
});
