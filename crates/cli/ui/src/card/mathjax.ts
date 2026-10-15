// MathJax as Anki's reviewer loads it: the `tex-chtml-full` component of MathJax 3.2.2, with the configuration of
// ts/mathjax/index.ts of Anki 26.08.1.

declare const __MATHJAX__: string;

interface MathJax {
  startup: { promise: Promise<void> };
  typesetClear(): void;
  typesetPromise(elements: Element[]): Promise<void>;
}

declare global {
  interface Window {
    MathJax: MathJax | object;
  }
}

const packages = ["noerrors", "mathtools"];

let loading: Promise<MathJax> | undefined;

function load(): Promise<MathJax> {
  window.MathJax = {
    tex: {
      displayMath: [["\\[", "\\]"]],
      processEscapes: false,
      processEnvironments: false,
      processRefs: false,
      packages: { "[+]": packages, "[-]": ["textmacros"] },
    },
    loader: {
      load: packages.map((name) => `[tex]/${name}`),
      paths: { mathjax: __MATHJAX__ },
    },
    startup: { typeset: false },
  };
  return new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = `${__MATHJAX__}/tex-chtml-full.js`;
    script.addEventListener("load", () => {
      const mathjax = window.MathJax as MathJax;
      mathjax.startup.promise.then(() => resolve(mathjax), reject);
    });
    script.addEventListener("error", () => reject(new Error(`cannot load ${script.src}`)));
    document.head.appendChild(script);
  });
}

/** Whether `html` has a MathJax delimiter that Anki typesets: `\(` or `\[`. */
export function hasMath(html: string): boolean {
  return html.includes("\\(") || html.includes("\\[");
}

/** Typesets the maths in `element`. It loads MathJax on the first call. An error is logged, as in Anki. */
export async function typeset(element: Element): Promise<void> {
  try {
    loading ??= load();
    const mathjax = await loading;
    mathjax.typesetClear();
    await mathjax.typesetPromise([element]);
  } catch (error) {
    console.log("MathJax failed: ", error);
  }
}
