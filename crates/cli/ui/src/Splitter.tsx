import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import { clampWidth } from "./width";

interface Props {
  /** The width of the table pane in pixels, or `null` for the default width. */
  width: number | null;
  onWidth: (width: number) => void;
  onResizing: (resizing: boolean) => void;
}

const STEP = 16;

/** The handle between the table and the card. Drag it, or focus it and use the arrow keys. */
export function Splitter({ width, onWidth, onResizing }: Props) {
  const start = useRef<{ x: number; width: number } | null>(null);

  function current(element: HTMLElement): number {
    return width ?? element.previousElementSibling?.getBoundingClientRect().width ?? 0;
  }

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    start.current = { x: event.clientX, width: current(event.currentTarget) };
    onResizing(true);
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    if (start.current === null) return;
    onWidth(clampWidth(start.current.width + event.clientX - start.current.x, window.innerWidth));
  }

  function onPointerUp(event: PointerEvent<HTMLDivElement>) {
    if (start.current === null) return;
    event.currentTarget.releasePointerCapture(event.pointerId);
    start.current = null;
    onResizing(false);
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const step = event.key === "ArrowLeft" ? -STEP : event.key === "ArrowRight" ? STEP : 0;
    if (step === 0) return;
    event.preventDefault();
    onWidth(clampWidth(current(event.currentTarget) + step, window.innerWidth));
  }

  return (
    <div
      className="splitter"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the table"
      aria-valuenow={width === null ? undefined : Math.round(width)}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onKeyDown={onKeyDown}
    />
  );
}
