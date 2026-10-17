const TYPING = "textarea, select, input:not([type=checkbox], [type=radio], [type=button], [type=submit], [type=reset])";

/** Whether a key press flips the card: Space without modifiers, which no handler took. A field that takes text keeps
 * Space. A button or checkbox keeps it only when the keyboard focused it, so Space still flips after a click. */
export function isFlipKey(event: KeyboardEvent): boolean {
  if (event.key !== " " || event.repeat || event.defaultPrevented) return false;
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return false;
  const target = event.target;
  if (!(target instanceof HTMLElement)) return true;
  if (target.isContentEditable || target.matches(TYPING)) return false;
  return !(target.matches("button, input") && target.matches(":focus-visible"));
}
