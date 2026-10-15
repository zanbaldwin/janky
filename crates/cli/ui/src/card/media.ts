// The media names of a card document that has no server (`janky show --format html`). The card data then maps each
// media name to a `file://` URL, and the shim puts the URLs in place of the names before the browser loads them.

/** Media names by their Anki name. */
export type Media = Record<string, string>;

function decode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function resolve(media: Media, value: string): string | undefined {
  return media[value] ?? media[decode(value)];
}

const URL_FUNCTION = /url\(\s*(['"]?)([^'")]+)\1\s*\)/g;

function resolveCss(media: Media, css: string): string {
  return css.replace(URL_FUNCTION, (match, _quote: string, name: string) => {
    const url = resolve(media, name.trim());
    return url === undefined ? match : `url("${url}")`;
  });
}

/** Replaces the media names in `src` and `poster` attributes and in CSS `url()` with their URLs. A name that is not
 * in `media` stays. */
export function resolveMedia(root: DocumentFragment, media: Media): void {
  for (const element of Array.from(root.querySelectorAll("[src], [poster]"))) {
    for (const name of ["src", "poster"]) {
      const value = element.getAttribute(name);
      const url = value === null ? undefined : resolve(media, value);
      if (url !== undefined) element.setAttribute(name, url);
    }
  }
  for (const style of Array.from(root.querySelectorAll("style"))) {
    style.textContent = resolveCss(media, style.textContent ?? "");
  }
  for (const element of Array.from(root.querySelectorAll("[style]"))) {
    element.setAttribute("style", resolveCss(media, element.getAttribute("style") ?? ""));
  }
}

/** The URL of a media file: from `media` in a document without a server, else relative to the media of the
 * server. */
export function mediaUrl(name: string, media: Media | undefined): string {
  return media?.[name] ?? encodeURIComponent(name);
}
