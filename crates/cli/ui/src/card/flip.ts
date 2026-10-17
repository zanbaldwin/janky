const TYPING = "textarea, select, input:not([type=checkbox], [type=radio], [type=button], [type=submit], [type=reset])";

/** `isFlipKey` of src/flip.ts, as a copy, because the `card` entry must be one JS file. */
export function isFlipKey(event: KeyboardEvent): boolean {
  if (event.key !== " " || event.repeat || event.defaultPrevented) return false;
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return false;
  const target = event.target;
  if (!(target instanceof HTMLElement)) return true;
  if (target.isContentEditable || target.matches(TYPING)) return false;
  return !(target.matches("button, input") && target.matches(":focus-visible"));
}
