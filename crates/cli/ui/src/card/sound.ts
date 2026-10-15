// The play buttons of Anki's reviewer (av_refs_to_play_icons in qt/aqt/sound.py of Anki 26.08.1), and what their
// `pycmd('play:q:0')` calls do in the preview.

import { mediaUrl, type Media } from "./media";

export type Sound = { kind: "file"; name: string } | { kind: "speech"; text: string; language: string };

const PLAY_TAG = /\[anki:(play:[qa]:\d+)\]/g;

/** Replaces each `[anki:play:q:N]` and `[anki:play:a:N]` tag with Anki's play button. */
export function playButtons(html: string): string {
  return html.replace(
    PLAY_TAG,
    (_, command: string) => `
<a class="replay-button soundLink" href=# onclick="pycmd('${command}'); return false;" draggable="false">
    <svg class="playImage" viewBox="0 0 64 64" version="1.1">
        <circle cx="32" cy="32" r="29" />
        <path d="M56.502,32.301l-37.502,20.101l0.329,-40.804l37.173,20.703Z" />
    </svg>
</a>`,
  );
}

let playing: HTMLAudioElement | undefined;

/** Plays one sound, and stops the sound before it. A file plays from the media of the card document. Speech uses
 * the speech synthesis of the browser. */
export function play(sound: Sound, media?: Media): void {
  playing?.pause();
  window.speechSynthesis?.cancel();
  if (sound.kind === "file") {
    playing = new Audio(mediaUrl(sound.name, media));
    void playing.play().catch((error: unknown) => console.log("cannot play", sound.name, error));
  } else if (window.speechSynthesis) {
    const utterance = new SpeechSynthesisUtterance(sound.text);
    utterance.lang = sound.language.replace("_", "-");
    window.speechSynthesis.speak(utterance);
  } else {
    console.log("this browser has no speech synthesis for", sound.language, sound.text);
  }
}

/** Handles a `pycmd` command of the card. `play:q:N` and `play:a:N` play sound `N` of the side that is shown. */
export function command(text: string, sounds: Sound[], media?: Media): void {
  const match = /^play:[qa]:(\d+)$/.exec(text);
  const sound = match ? sounds[Number(match[1])] : undefined;
  if (sound) {
    play(sound, media);
  } else {
    console.log("pycmd:", text);
  }
}
