# Styling and scripts

A note type can have one CSS file for the look of its cards, and JavaScript files for behaviour on
the cards. CSS is the language that sets colours, fonts, sizes and layout. JavaScript is the
programming language of web pages. Anki runs it when it shows a card.

## CSS: the `css` key

Give the path of one CSS file in `css`:

```toml
css = "style.css"
```

janky copies the text of the file, without changes, into the styling of the note type. In Anki,
this is the "Styling" part of the card templates. All card types of the note type use it.

This is the CSS of the tutorial's word note type:

```css
.card {
  font-family: system-ui, sans-serif;
  font-size: 22px;
  text-align: center;
}
.word { font-size: 36px; }
.article-m { color: #1f5fbf; }
.article-f { color: #bf1f5f; }
.gender, .frequency { color: #777; font-size: 16px; }
.examples { font-style: italic; }
.tap-reveal { filter: blur(6px); cursor: pointer; }
.tap-reveal.shown { filter: none; }
.notes { font-size: 16px; text-align: left; }
```

The classes such as `word`, `meaning` and `examples` come from the HTML in the card templates and
the field templates.

The phrase note type of the tutorial uses the same file with `css = "word/style.css"`. See
[Share files between note types](structure.md#share-files-between-note-types).

## JavaScript: the `assets` key

Give the paths of JavaScript files in `assets`:

```toml
assets = ["assets/reveal.js"]
```

Then load a file in a card template with a `<script>` element. In `src`, give the **file name**
only, not the path:

```html
<script src="reveal.js"></script>
```

janky replaces this element with the text of the file. Anki then has the script in the card
template itself, so the package needs no extra media file for it. In the tutorial, the back of the
recognition card becomes:

```html
...
{{#Notes}}<div class="notes">{{Notes}}</div>{{/Notes}}
<script>
document.querySelectorAll(".tap-reveal").forEach((element) => {
  element.addEventListener("click", () => element.classList.add("shown"));
});
</script>
```

### The exact rule for `<script src>`

janky replaces a `<script>` element only when all of these are true:

- `<script` is in lowercase letters.
- The value of `src` is in quotes (`"` or `'`).
- The value of `src` is the file name of one of the `assets`.
- There is nothing between `<script ...>` and `</script>`, or only spaces and newlines.

If one of these is not true, janky does not change the element. These elements stay as they are:

| Element | Why janky does not replace it |
|---------|-------------------------------|
| `<script src="reveal.js"> keep </script>` | There is text between the tags |
| `<script src="other.js"></script>` | `other.js` is not in `assets` |
| `<SCRIPT src="reveal.js"></SCRIPT>` | Uppercase letters |
| `<script src=reveal.js></script>` | No quotes |

> [!NOTE]
> An element that janky does not replace is a normal script reference. janky does not put the
> file in the package.

Two assets with the same file name in different directories cannot both work. Give each asset its
own file name.

### An asset must not contain `</script`

janky puts the asset between `<script>` and `</script>`. So the asset itself must not contain the
text `</script`, in uppercase or lowercase letters. If it does, janky gives this error:

```text
template::script-end  `assets/a.js` contains `</script>`
```

If you need this text in a JavaScript string, split it, for example `"</scr" + "ipt>"`.

## See your cards in the browser

Run `janky preview` to see your cards while you change the CSS or the JavaScript. Reload the page
after each change. [The command reference](../cli/commands.md#preview) explains the command.

The preview puts the same classes on `<body>` as Anki:

| Class | When |
|-------|------|
| `card` and `card1`, `card2`, … | Always. The number is the position of the card type in the note type file, from 1. |
| `nightMode` and `night_mode` | Night mode is on. |
| `isLin` | The platform menu shows Desktop. Anki uses `isMac` or `isWin` on those systems. |
| `mobile android` | The platform menu shows Android. The card is 412 pixels wide. |
| `mobile iphone` | The platform menu shows iPhone. The card is 390 pixels wide. |

Use these classes in your CSS, for example `.card.nightMode { background-color: #222; }`. The
`card` class and the night class are on the same element, so write them without a space.

The preview also has the base CSS of the Anki reviewer, for example the margin of `body`, the
maximum size of images and the colours of a typed answer. So a card looks the same as in Anki.

## Tips

- Keep the CSS classes the same in the card templates and the field templates. For example, the
  `article` plugin filter of the tutorial makes `<span class="article-f">`, and `style.css` gives
  this class a colour.
- To use one CSS file for several note types, use the single-file form for them. See
  [Share files between note types](structure.md#share-files-between-note-types).
- After you change the CSS or an asset, run `janky plan`. It shows a line such as
  `~ note type Spanish Word: card templates or CSS` for each note type that uses the file.
- Use the file name of each asset only once, also in different directories.
