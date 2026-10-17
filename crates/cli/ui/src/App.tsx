import { useCallback, useEffect, useMemo, useState, type CSSProperties } from "react";
import { fetchPreview, type Preview, type View } from "./api";
import { CardTable } from "./CardTable";
import { CardView } from "./CardView";
import { Diagnostics } from "./Diagnostics";
import { filterRows, formatHash, parseHash, type Selection } from "./selection";
import { Splitter } from "./Splitter";
import { loadWidth, saveWidth } from "./width";

export function App() {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [showEmpty, setShowEmpty] = useState(false);
  const [selection, setSelection] = useState<Selection>(() => parseHash(window.location.hash));
  const [typed, setTyped] = useState("");
  const [width, setWidth] = useState<number | null>(loadWidth);
  const [resizing, setResizing] = useState(false);
  const view = useCallback((view: Partial<View>) => setSelection((selection) => ({ ...selection, ...view })), []);

  useEffect(() => {
    const controller = new AbortController();
    fetchPreview(controller.signal)
      .then(setPreview)
      .catch((error: unknown) => {
        if (!controller.signal.aborted) setFailure(String(error));
      });
    return () => controller.abort();
  }, []);

  useEffect(() => {
    window.history.replaceState(null, "", formatHash(selection));
  }, [selection]);

  useEffect(() => {
    if (width !== null) saveWidth(width);
  }, [width]);

  const rows = useMemo(
    () => filterRows(preview?.rows ?? [], filter).filter((row) => showEmpty || !row.empty),
    [preview, filter, showEmpty],
  );

  if (failure !== null) return <p className="failure">The preview server did not answer: {failure}</p>;
  if (preview === null) return <p className="loading">Rendering the cards…</p>;

  const row = preview.rows[selection.row] ?? preview.rows[0];
  const select = (row: number) => {
    setTyped("");
    setSelection((selection) => ({ ...selection, row }));
  };

  return (
    <div
      className={resizing ? "layout resizing" : "layout"}
      style={width === null ? undefined : ({ "--table-width": `${width}px` } as CSSProperties)}
    >
      <aside>
        {preview.stale && (
          <p className="stale">The project did not load. These cards are from an earlier load. Fix the errors, then reload the page.</p>
        )}
        <Diagnostics diagnostics={preview.diagnostics} />
        <input
          type="search"
          placeholder="Filter cards"
          aria-label="Filter cards"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
        <label className="show-empty">
          <input type="checkbox" checked={showEmpty} onChange={(event) => setShowEmpty(event.target.checked)} />
          Show empty cards
        </label>
        <CardTable rows={rows} selected={row?.row ?? -1} onSelect={select} />
      </aside>
      <Splitter width={width} onWidth={setWidth} onResizing={setResizing} />
      <main>
        {row === undefined ? (
          <p className="loading">The project has no cards.</p>
        ) : (
          <CardView
            generation={preview.generation}
            exported={preview.exported}
            row={row}
            view={selection}
            onView={view}
            typed={typed}
            onTyped={setTyped}
          />
        )}
      </main>
    </div>
  );
}
