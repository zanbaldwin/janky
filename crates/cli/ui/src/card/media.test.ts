// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { mediaUrl, resolveMedia } from "./media";

const media = { "cat.png": "file:///deck/media/cat.png", "le chat.mp4": "file:///deck/media/le%20chat.mp4" };

function resolved(html: string): string {
  const template = document.createElement("template");
  template.innerHTML = html;
  resolveMedia(template.content, media);
  return template.innerHTML;
}

describe("resolveMedia", () => {
  it("replaces media names in src and poster, also URL encoded", () => {
    expect(resolved('<img src="cat.png"><video poster="cat.png" src="le%20chat.mp4"></video>')).toBe(
      '<img src="file:///deck/media/cat.png"><video poster="file:///deck/media/cat.png" src="file:///deck/media/le%20chat.mp4"></video>',
    );
  });

  it("replaces media names in CSS url() of style elements and style attributes", () => {
    expect(resolved("<style>.a { background: url('cat.png') }</style><p style=\"background: url(cat.png)\">x</p>")).toBe(
      '<style>.a { background: url("file:///deck/media/cat.png") }</style><p style="background: url(&quot;file:///deck/media/cat.png&quot;)">x</p>',
    );
  });

  it("leaves names that are not media of the card", () => {
    const html = '<img src="https://example.com/cat.png"><img src="dog.png"><style>a { b: url(dog.png) }</style>';
    expect(resolved(html)).toBe(html);
  });
});

describe("mediaUrl", () => {
  it("uses the file URL in a document without a server, and the name with the server", () => {
    expect(mediaUrl("cat.png", media)).toBe("file:///deck/media/cat.png");
    expect(mediaUrl("le chat.mp4", undefined)).toBe("le%20chat.mp4");
  });
});
