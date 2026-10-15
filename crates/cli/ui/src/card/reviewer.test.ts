// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { typeset } from "./mathjax";
import { runHooks, show } from "./reviewer";

vi.mock("./mathjax", async (original) => ({
  ...(await original<typeof import("./mathjax")>()),
  typeset: vi.fn(async () => {}),
}));

beforeEach(() => {
  document.body.className = "";
  document.body.innerHTML = '<div id="qa"></div>';
  window.scrollTo = vi.fn();
});

describe("show", () => {
  it("runs the scripts of the card HTML", async () => {
    await show({
      html: '<p id="out"></p><script>document.getElementById("out").textContent = "ran 前"</script>',
      bodyclass: "card card1",
      side: "front",
      sounds: [],
    });
    expect(document.getElementById("out")?.textContent).toBe("ran 前");
  });

  it("runs the update hooks, then shows the card, then runs the shown hooks", async () => {
    const qa = document.getElementById("qa") as HTMLElement;
    const log: string[] = [];
    const bodyclass = "card card2 isLin nightMode night_mode";
    const shown = show({ html: "<b>前</b>", bodyclass, side: "front", sounds: [] });
    window.onUpdateHook.push(() => void log.push(`update ${document.body.className} ${qa.style.opacity}`));
    window.onShownHook.push(() => void log.push(`shown ${qa.style.opacity}`));
    await shown;
    expect(log).toEqual(["update card card2 isLin nightMode night_mode 0", "shown 1"]);
    expect(qa.querySelector("b")?.textContent).toBe("前");
  });

  it("starts each side with no hooks of an earlier side", async () => {
    const log: string[] = [];
    window.onShownHook = [() => void log.push("old")];
    await show({ html: "x", bodyclass: "card card1", side: "front", sounds: [] });
    expect(log).toEqual([]);
  });

  it("scrolls to the answer on the back", async () => {
    const scroll = vi.fn();
    window.HTMLElement.prototype.scrollIntoView = scroll;
    await show({ html: "front<hr id=answer>back", bodyclass: "card card1", side: "back", sounds: [] });
    expect(scroll).toHaveBeenCalledOnce();
  });
});

describe("show with maths and sounds", () => {
  it("typesets only a side with a MathJax delimiter", async () => {
    vi.mocked(typeset).mockClear();
    await show({ html: "no maths, only $x$", bodyclass: "card card1", side: "front", sounds: [] });
    expect(typeset).not.toHaveBeenCalled();
    await show({ html: "\\(x^2\\)", bodyclass: "card card1", side: "front", sounds: [] });
    expect(typeset).toHaveBeenCalledWith(document.getElementById("qa"));
  });

  it("does not typeset in a document without a server", async () => {
    vi.mocked(typeset).mockClear();
    await show({ html: "\\(x^2\\)", bodyclass: "card card1", side: "front", sounds: [], media: {}, mathjax: false });
    expect(typeset).not.toHaveBeenCalled();
  });

  it("puts the file URLs of the media in place of the names before the card scripts run", async () => {
    await show({
      html: '<img src="cat.png"><p id="out"></p><script>document.getElementById("out").textContent = document.querySelector("img").getAttribute("src")</script>',
      bodyclass: "card card1",
      side: "front",
      sounds: [],
      media: { "cat.png": "file:///deck/cat.png" },
      mathjax: false,
    });
    expect(document.getElementById("out")?.textContent).toBe("file:///deck/cat.png");
  });

  it("puts play buttons in place of the play tags, before the card scripts run", async () => {
    await show({
      html: '[anki:play:q:0]<p id="out"></p><script>document.getElementById("out").textContent = document.querySelectorAll(".replay-button").length</script>',
      bodyclass: "card card1",
      side: "front",
      sounds: [{ kind: "file", name: "hello.ogg" }],
    });
    expect(document.getElementById("out")?.textContent).toBe("1");
    expect(document.querySelector(".replay-button")?.getAttribute("onclick")).toBe("pycmd('play:q:0'); return false;");
  });
});

describe("show with a type answer input", () => {
  const html = '<center><input type=text id=typeans onkeypress="_typeAnsPress();"></center>';

  it("focuses the input on the front", async () => {
    await show({ html, bodyclass: "card card1", side: "front", sounds: [] });
    expect(document.activeElement?.id).toBe("typeans");
  });

  it("sends the typed text to the page, and asks for the back on pycmd('ans')", async () => {
    const post = vi.spyOn(window.parent, "postMessage").mockImplementation(() => {});
    await show({ html, bodyclass: "card card1", side: "front", sounds: [] });
    const input = document.getElementById("typeans") as HTMLInputElement;
    input.value = "ねこ";
    input.dispatchEvent(new Event("input"));
    window.pycmd("ans");
    const origin = new URL(window.location.href).origin;
    expect(post.mock.calls).toEqual([
      [{ kind: "janky-typed", text: "ねこ", answer: false }, origin],
      [{ kind: "janky-typed", text: "ねこ", answer: true }, origin],
    ]);
    post.mockRestore();
  });

  it("answers on Enter in the input, as Anki's _typeAnsPress does", async () => {
    await show({ html, bodyclass: "card card1", side: "front", sounds: [] });
    const pycmd = vi.spyOn(window, "pycmd");
    window.event = new KeyboardEvent("keypress", { key: "a" });
    window._typeAnsPress();
    window.event = new KeyboardEvent("keypress", { key: "Enter" });
    window._typeAnsPress();
    expect(pycmd.mock.calls).toEqual([["ans"]]);
    pycmd.mockRestore();
  });
});

describe("runHooks", () => {
  it("runs the other hooks when one hook throws", async () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    const ran: number[] = [];
    await runHooks([
      () => void ran.push(1),
      () => {
        throw new Error("broken");
      },
      async () => void ran.push(3),
    ]);
    expect(ran).toEqual([1, 3]);
    expect(log).toHaveBeenCalledWith("Hook failed: ", expect.any(Error));
  });
});
