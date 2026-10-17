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

const PLATFORMS: { value: Platform; label: string; icon: string }[] = [
  {
    value: "desktop",
    label: "Desktop",
    icon: "M4 3h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2ZM8 21h8M12 17v4",
  },
  {
    value: "iphone",
    label: "Mobile",
    icon: "M7 2h10a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2ZM12 18h.01",
  },
];

const SUN =
  "M12 8a4 4 0 1 1 0 8 4 4 0 0 1 0-8ZM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4";
const MOON = "M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z";

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
          className="pill sides"
          role="group"
          aria-label="Side"
          aria-keyshortcuts="Space"
          title="Space flips the card"
          data-side={view.side}
        >
          <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M8 3 4 7l4 4M4 7h16M16 21l4-4-4-4M20 17H4" />
          </svg>
          <div className={view.side === "back" ? "segments end" : "segments"}>
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
          <button
            type="button"
            className="night"
            aria-label="Night mode"
            aria-pressed={view.night}
            title={view.night ? "Night mode is on" : "Night mode is off"}
            onClick={() => onView({ night: !view.night })}
          >
            <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
              <path d={view.night ? MOON : SUN} />
            </svg>
          </button>
          <div className="pill" role="group" aria-label="Platform">
            <div className={view.platform === "desktop" ? "segments" : "segments end"}>
              {PLATFORMS.map((platform) => (
                <button
                  key={platform.value}
                  type="button"
                  aria-pressed={(view.platform === "desktop") === (platform.value === "desktop")}
                  onClick={() => onView({ platform: platform.value })}
                >
                  <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
                    <path d={platform.icon} />
                  </svg>
                  {platform.label}
                </button>
              ))}
            </div>
          </div>
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
