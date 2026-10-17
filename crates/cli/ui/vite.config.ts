import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

const preview = { target: "http://127.0.0.1:5259" };

/** Copies the MathJax component of Anki's reviewer and its fonts into the build. Their names are not hashed,
 * because MathJax loads the fonts by name, so the directory has the MathJax version. The card shim gets the
 * directory as `__MATHJAX__`, relative to the base of the card documents, `{generation}/media/`. */
function mathjax(): Plugin {
  const root = join(import.meta.dirname, "node_modules/mathjax/es5");
  const { version } = JSON.parse(readFileSync(join(root, "../package.json"), "utf8")) as { version: string };
  const directory = `assets/mathjax-${version}`;
  const fonts = "output/chtml/fonts/woff-v2";
  return {
    name: "janky-mathjax",
    config: () => ({ define: { __MATHJAX__: JSON.stringify(`../../${directory}`) } }),
    generateBundle() {
      const files = ["tex-chtml-full.js", ...readdirSync(join(root, fonts)).map((font) => `${fonts}/${font}`)];
      for (const file of files) {
        this.emitFile({ type: "asset", fileName: `${directory}/${file}`, source: readFileSync(join(root, file)) });
      }
    },
  };
}

export default defineConfig({
  // CWD-independent, so `npm --prefix crates/cli/ui run build` works from the workspace root.
  root: import.meta.dirname,
  // Relative URLs, so that an exported preview also works below a subpath.
  base: "./",
  plugins: [react(), mathjax()],
  build: {
    outDir: "dist",
    // crates/cli/build.rs embeds every file in dist/, so old hashed files must not stay.
    emptyOutDir: true,
    // Read by crates/cli/build.rs and vite-chunks (dist/.vite/manifest.json).
    manifest: true,
    sourcemap: false,
    rollupOptions: {
      input: { index: "index.html", card: "src/card/main.ts" },
      output: {
        entryFileNames: "assets/[name].[hash].js",
        chunkFileNames: "assets/[name].[hash].js",
        assetFileNames: "assets/[name].[hash][extname]",
      },
    },
  },
  server: {
    proxy: { "/preview.json": preview, "^/[0-9a-f]{32}/": preview },
  },
});
