import type { ReactNode } from "react";

const ICONS = {
  warning: "M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h16.9a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0ZM12 9v4M12 17h.01",
  info: "M12 2a10 10 0 1 1 0 20 10 10 0 0 1 0-20ZM12 16v-4M12 8h.01",
};

export function Notice({ kind, children }: { kind: keyof typeof ICONS; children: ReactNode }) {
  return (
    <div className={`notice ${kind}`}>
      <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
        <path d={ICONS[kind]} />
      </svg>
      <p>{children}</p>
    </div>
  );
}
