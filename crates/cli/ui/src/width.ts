/** The smallest width of the table pane, in pixels. */
export const MIN_TABLE = 200;
/** The smallest width of the card pane, in pixels. */
export const MIN_CARD = 240;

const KEY = "janky-preview-table-width";

/** Keeps the table pane between `MIN_TABLE` and the width that leaves `MIN_CARD` for the card. */
export function clampWidth(width: number, total: number): number {
  return Math.round(Math.max(MIN_TABLE, Math.min(width, total - MIN_CARD)));
}

/** The width of the table pane that this browser saved, or `null`. */
export function loadWidth(): number | null {
  try {
    const width = Number.parseInt(window.localStorage.getItem(KEY) ?? "", 10);
    return Number.isFinite(width) ? width : null;
  } catch {
    return null;
  }
}

export function saveWidth(width: number): void {
  try {
    window.localStorage.setItem(KEY, String(width));
  } catch {
    // The width is a convenience. Without storage, the next load uses the default width.
  }
}
