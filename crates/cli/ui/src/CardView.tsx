import { useCallback, useEffect, useRef } from "react";
import { cardUrl, type Platform, type Row, type View } from "./api";
import type { FlipMessage, TypedMessage } from "./card/reviewer";
import { isFlipKey } from "./flip";

interface Props {
  generation: string;
  exported: boolean;
  row: Row;
  view: View;
  onView: (view: Partial<View>) => void;
  typed: string;
  onTyped: (typed: string) => void;
}

const PLATFORMS: { value: Platform; label: string }[] = [
  { value: "desktop", label: "Desktop" },
  { value: "android", label: "Android" },
  { value: "iphone", label: "iPhone" },
];

export function CardView({ generation, exported, row, view, onView, typed, onTyped }: Props) {
  const title = [row.notetype.name, row.entry?.id ?? "sample", row.card.name].join(" · ");
  const frame = useRef<HTMLIFrameElement>(null);
  const other = view.side === "front" ? "back" : "front";

  const showSide = useCallback(
    (side: View["side"]) => {
      if (side === "front") onTyped("");
      onView({ side });
    },
    [onTyped, onView],
  );

  useEffect(() => {
    function onMessage(event: MessageEvent) {
      if (event.source === null || event.source !== frame.current?.contentWindow) return;
      const message = event.data as Partial<TypedMessage> | Partial<FlipMessage>;
      if (message.kind === "janky-flip") showSide(other);
      if (message.kind !== "janky-typed" || typeof message.text !== "string") return;
      onTyped(message.text);
      if (message.answer === true) onView({ side: "back" });
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [onTyped, onView, showSide, other]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (!isFlipKey(event)) return;
      event.preventDefault();
      showSide(other);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [showSide, other]);

  return (
    <section className="view">
      <header>
        <h2>{title}</h2>
        <div
          className="sides"
          role="group"
          aria-label="Side"
          aria-keyshortcuts="Space"
          title="Space flips the card"
          data-side={view.side}
        >
          <svg className="flip" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M8 3 4 7l4 4M4 7h16M16 21l4-4-4-4M20 17H4" />
          </svg>
          <div className="segments">
            {(["front", "back"] as const).map((side) => (
              <button
                key={side}
                type="button"
                aria-pressed={view.side === side}
                onClick={() => showSide(side)}
              >
                {side === "front" ? "Front" : "Back"}
              </button>
            ))}
          </div>
        </div>
        <div className="controls">
          <label>
            <input type="checkbox" checked={view.night} onChange={(event) => onView({ night: event.target.checked })} />
            Night mode
          </label>
          <select
            aria-label="Platform"
            value={view.platform}
            onChange={(event) => onView({ platform: event.target.value as Platform })}
          >
            {PLATFORMS.map((platform) => (
              <option key={platform.value} value={platform.value}>
                {platform.label}
              </option>
            ))}
          </select>
        </div>
      </header>
      {row.empty && <p className="note">The front is empty, so Anki does not make this card.</p>}
      {exported && row.type_answer && view.side === "back" && (
        <p className="note">
          Only <code>janky preview</code> compares the typed answer. This back shows the expected answer.
        </p>
      )}
      {row.error !== null ? (
        <pre className="card-error">{row.error}</pre>
      ) : (
        <div className={`frame ${view.platform}`}>
          <iframe
            ref={frame}
            title={`${title} (${view.side})`}
            sandbox="allow-scripts"
            src={cardUrl(generation, row.row, view, exported ? "" : typed)}
          />
        </div>
      )}
    </section>
  );
}
