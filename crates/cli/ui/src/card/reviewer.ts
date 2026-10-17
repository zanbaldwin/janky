// The parts of Anki's reviewer (ts/reviewer/index.ts of Anki 26.08.1) that card HTML and card JS depend on.

import { hasMath, typeset } from "./mathjax";
import { resolveMedia, type Media } from "./media";
import { command, playButtons, type Sound } from "./sound";

export type Callback = () => void | Promise<void>;

declare global {
  interface Window {
    onUpdateHook: Callback[];
    onShownHook: Callback[];
    pycmd: (command: string, callback?: unknown) => void;
    _typeAnsPress: () => void;
  }
}

/** A message from the card document to the page: the text of `#typeans`, and whether to show the back. */
export interface TypedMessage {
  kind: "janky-typed";
  text: string;
  answer: boolean;
}

function typed(answer: boolean): void {
  const input = document.getElementById("typeans");
  const text = input instanceof HTMLInputElement ? input.value : "";
  const message: TypedMessage = { kind: "janky-typed", text, answer };
  window.parent.postMessage(message, new URL(window.location.href).origin);
}

export interface CardData {
  /** The note type CSS in a `<style>` element, then the card HTML, as Anki's `question_and_style()` gives it. */
  html: string;
  /** The card ordinal, from 0. */
  ord: number;
  /** The classes of the body. Without it, they come from `ord` and the query of the document URL. */
  bodyclass?: string;
  side: "front" | "back";
  /** The sounds of the play tags of this side, by their number. */
  sounds: Sound[];
  /** In a document without a server: the `file://` URL of each media name. */
  media?: Media;
  /** `false` in a document without a server, where MathJax cannot load its fonts. */
  mathjax?: boolean;
}

/** The classes of the card body, as Anki's reviewer sets them (`body_class` in crates/cli/src/preview.rs), from the
 * `night` and `platform` parameters of `search`. */
export function bodyClass(ord: number, search: string): string {
  const query = new URLSearchParams(search);
  const platforms: Record<string, string> = { android: "mobile android", iphone: "mobile iphone" };
  const platform = platforms[query.get("platform") ?? ""] ?? "isLin";
  const night = query.get("night") === "1" ? " nightMode night_mode" : "";
  return `card card${ord + 1} ${platform}${night}`;
}

/** Calls each hook. A hook that throws is logged, and the other hooks still run. */
export function runHooks(hooks: Callback[]): Promise<PromiseSettledResult<void>[]> {
  const results: (void | Promise<void>)[] = [];
  for (const hook of hooks) {
    try {
      results.push(hook());
    } catch (error) {
      console.log("Hook failed: ", error);
    }
  }
  return Promise.allSettled(results);
}

// innerHTML does not run scripts, so each script is replaced by a new element, which runs. The promise resolves
// when an external script has loaded or failed.
function replaceScript(old: HTMLScriptElement): Promise<void> {
  return new Promise((resolve) => {
    const script = document.createElement("script");
    if (old.src) {
      script.addEventListener("load", () => resolve());
      script.addEventListener("error", () => resolve());
    }
    for (const attribute of Array.from(old.attributes)) {
      script.setAttribute(attribute.name, attribute.value);
    }
    script.appendChild(document.createTextNode(old.innerHTML));
    old.replaceWith(script);
    if (!old.src) resolve();
  });
}

export async function setInnerHTML(element: Element, html: string, media?: Media): Promise<void> {
  if (media) {
    // The content of a template loads nothing, so the names change before the browser loads any file.
    const template = document.createElement("template");
    template.innerHTML = html;
    resolveMedia(template.content, media);
    element.replaceChildren(template.content);
  } else {
    element.innerHTML = html;
  }
  for (const script of Array.from(element.getElementsByTagName("script"))) {
    await replaceScript(script);
  }
}

function renderError(error: unknown): string {
  const message = String(error).substring(0, 2000);
  const stack = error instanceof Error ? String(error.stack).substring(0, 2000) : "";
  return `<div>Invalid html on card: ${message}\n${stack}</div>`.replace(/\n/g, "<br>");
}

/** Shows one card side in `#qa` in the same order as Anki's `_updateQA`. */
export async function show(data: CardData): Promise<void> {
  window.pycmd = (text) => (text === "ans" ? typed(true) : command(text, data.sounds, data.media));
  window._typeAnsPress = () => {
    if ((window.event as KeyboardEvent | undefined)?.key === "Enter") window.pycmd("ans");
  };
  window.onUpdateHook = [
    () => {
      window.scrollTo(0, 0);
      document.body.className = data.bodyclass ?? bodyClass(data.ord, window.location.search);
    },
  ];
  window.onShownHook =
    data.side === "front"
      ? [
          () => {
            const input = document.getElementById("typeans");
            input?.addEventListener("input", () => typed(false));
            input?.focus();
          },
        ]
      : [];
  const qa = document.getElementById("qa");
  if (!qa) throw new Error("missing #qa");
  qa.style.opacity = "0";
  try {
    await setInnerHTML(qa, playButtons(data.html), data.media);
  } catch (error) {
    await setInnerHTML(qa, renderError(error));
  }
  await runHooks(window.onUpdateHook);
  if (data.mathjax !== false && hasMath(data.html)) await typeset(qa);
  qa.style.opacity = "1";
  await runHooks(window.onShownHook);
  if (data.side === "back") document.getElementById("answer")?.scrollIntoView();
}
