// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { command, playButtons } from "./sound";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("playButtons", () => {
  it("gives each play tag Anki's button with its pycmd command", () => {
    const html = playButtons("a [anki:play:q:0] b [anki:play:a:12]");
    expect(html.match(/class="replay-button soundLink"/g)).toHaveLength(2);
    expect(html).toContain(`onclick="pycmd('play:q:0'); return false;"`);
    expect(html).toContain(`onclick="pycmd('play:a:12'); return false;"`);
    expect(html).not.toContain("[anki:play");
  });

  it("leaves other bracket text alone", () => {
    const html = "[sound:x.mp3] [anki:tts lang=en_US]hi[/anki:tts] [anki:play:x:0] [anki:play:q:]";
    expect(playButtons(html)).toBe(html);
  });
});

describe("command", () => {
  it("plays the media file of the sound number from the media of the card document", () => {
    const played: string[] = [];
    vi.stubGlobal(
      "Audio",
      class {
        constructor(public src: string) {
          played.push(src);
        }
        play = () => Promise.resolve();
        pause = () => {};
      },
    );
    command("play:a:1", [
      { kind: "file", name: "a.ogg" },
      { kind: "file", name: "hello world #1.mp3" },
    ]);
    expect(played).toEqual(["hello%20world%20%231.mp3"]);
  });

  it("speaks a speech sound in its language", () => {
    const spoken: { text: string; lang: string }[] = [];
    vi.stubGlobal("speechSynthesis", { cancel: () => {}, speak: (utterance: { text: string; lang: string }) => spoken.push(utterance) });
    vi.stubGlobal(
      "SpeechSynthesisUtterance",
      class {
        lang = "";
        constructor(public text: string) {}
      },
    );
    command("play:q:0", [{ kind: "speech", text: "こんにちは", language: "ja_JP" }]);
    expect(spoken).toEqual([{ text: "こんにちは", lang: "ja-JP" }]);
  });

  it("logs any other command, and a sound number that the side does not have", () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    command("ans", []);
    command("play:q:3", [{ kind: "file", name: "a.ogg" }]);
    expect(log).toHaveBeenCalledWith("pycmd:", "ans");
    expect(log).toHaveBeenCalledWith("pycmd:", "play:q:3");
  });
});
