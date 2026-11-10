# Styling lists with CSS and HTML

A list can be styled with real CSS, and a row can hold real HTML tags, a subset of both.
Sicompass draws all of it with its own renderer, with no web view and nothing from the
network. A plugin or a small site sends it as rows of FFON, and you can style every list
yourself with a `user.css`.

Keys and speech are the point of Sicompass, so CSS never changes what a row says or the
order you move through it. Whatever would make the screen disagree with the keys is not
supported at all (see [What is ignored](#what-is-ignored)).

## Who may use it

Tags in a row are only read as markup where it is safe to:

- a provider that asks for it. A plugin puts `"markup": true` in its `plugin.json`.
- an FFON document reached through a `<link>`, such as a small site.

**Asking for markup makes every outside string yours to escape.** A file called
`<b>x.jpg` must stay a file name, so use `tags::escape` for text and
`markup::escape_attribute` for attribute values, both in `sicompass_sdk`. The photo album
plugin is a worked example, with a test holding exactly that file.

Every provider and plugin of ours asks for it. A third party's plugin that does not ask keeps
its tags literal, because a plugin written before this could be showing a stranger's text
unescaped.

Every list is drawn the same way, whether or not anything styles it, and every background
has rounded corners (`border-radius: 5px`) until a stylesheet says otherwise. A terminal's
cell colours stay square, because neighbouring cells have to touch.

## A row of HTML

| Tag | Use |
|---|---|
| `div`, `p`, `h1`, `h2`, `h3`, `header`, `span`, `b` | Structure and text. `class` works on all of them. |
| `img src alt` | A picture. `alt` is what the screen reader says. |
| `a href` | A link. Its text is its name. |
| `video src poster` | A video. Its text is its name. With no `poster`, the row shows its first picture. |

`src` is a file path, or `asset:<provider>/<file>` for a file compiled into a provider or
installed with a plugin. A tag that is not in the table stays as literal text.

Pictures, links, buttons and videos are the row's *targets*. Landing on a row, the screen
reader says how many it holds. `i` goes to the first and `a` to the last, which is in element
mode: Left and Right move in reading order, Up and Down move a line, Enter activates, `d`
opens a picture or video in the big view, Escape leaves. In the big view Enter pauses and
resumes a video (see [video.md](video.md)).

## A stylesheet

Send CSS as a row of its own, anywhere in the list it styles:

```
<style>ul { max-width: none } .urgent { color: #e06c75 }</style>
```

It is taken out of the tree when it arrives, so it is never a row, never spoken and never
counted. It covers its list and everything below it. A list's `ul` takes the classes of the
row you entered it from, so one stylesheet can address particular lists. Never send CSS as
a separate file.

Your own `user.css` sits in the same folder as `settings.json` and is read when Sicompass
starts. (A debug build's folder is `sicompass-dev`.)

The order, later winning, and by specificity within each:

1. Sicompass's defaults: `ul { max-width: 120ch }` and the heading sizes.
2. The provider's or site's sheet.
3. `user.css`.

So the 120 character column is an ordinary rule, and `ul { max-width: none }` uses the
whole window.

## What a selector can match

| Selector | Matches |
|---|---|
| `ul`, `li`, `ul.class` | The list and its rows. |
| `.class`, `div`, `p`, `img`, `a`, `video` | What is inside a row. |
| `.a .b` | Descendants. |
| `li:focus` | The row the cursor is on. |
| `img:focus`, `video:focus`, `a:focus`, `button:focus` | The target the cursor is on in element mode. |
| `li::marker` | The list prefix: the arrow or dash and its letter. |
| `@media (prefers-color-scheme: dark)` | The colour scheme setting, or `light`. |

## Properties

| Property | Values |
|---|---|
| `color`, `background`, `background-color` | A colour. |
| `font-size` | A length. |
| `font-weight` | `bold` or normal. |
| `padding`, `margin` | One to four lengths. |
| `border`, `border-width`, `border-color`, `border-radius` | |
| `width`, `height`, `max-width` | A length. `max-width` also takes `none`. |
| `text-align` | `left`, `start`, `center`, `right`, `end`. |
| `display` | `block`, `grid`, `flex`. |
| `grid-template-columns`, `grid-template-rows` | Tracks in any length, `auto`, or `repeat(n, ...)`. |
| `grid-column` | `span n`. |
| `gap` | A length. |
| `flex-wrap` | `wrap`, `nowrap`. |
| `justify-content` | `start`, `flex-start`, `center`, `end`, `flex-end`, `space-between`. |
| `object-fit` | `contain`, `cover`. |
| `aspect-ratio` | `1` or `16 / 9`. |
| `outline`, `outline-width`, `outline-color`, `outline-offset` | The mark round the cursor. See below. |

Lengths are `px`, `em`, `rem`, `ch`, `%`, and `fr` in a grid track. CSS pixels grow with the
font scale setting, so a sheet keeps working at any size.

## A grid

```
<style>
  ul { max-width: none }
  .grid-container { display: grid; grid-template-columns: repeat(3, 1fr); gap: 15px }
  .grid-item img { aspect-ratio: 1; object-fit: cover; border-radius: 6px }
</style>
```

```
<div class="grid-container"><div class="grid-item"><img src="a.jpg" alt="Beach"><p>Beach</p></div>...</div>
```

All of that is one row with one target per picture, and in element mode, Up and Down move
a row of the grid. A row is laid out in boxes when its display is `grid` or `flex`, or when
it holds a video or two pictures or more. The tutorial has a working one, and so do the
photo album and video store plugins.

## The cursor's outline

The cursor's row and, in element mode, its target are marked with `outline`. The default sheet
gives the row a 3 px ring and every target a 3 px frame:

```
li:focus { outline: 3px solid; outline-offset: 3px }
img:focus, video:focus { outline: 3px solid }
a:focus, button:focus { outline: 3px solid; outline-offset: 2px }
```

Unlike a browser's, the outline is drawn inside the edge of what it marks, and `outline-offset`
is the room between it and what is inside. A row of boxes keeps that room whether or not the
cursor is on it, so the outline never covers a picture, a neighbour or the row's own ring, and
nothing moves when the cursor arrives. A picture's box keeps the size the sheet gives it, and
the picture is drawn smaller inside it. Leave out the colour to keep the theme's.

```
li:focus { outline: 4px solid #e80; outline-offset: 6px }
img:focus { outline-color: #e80 }
```

## What is ignored

These are refused on purpose. Source order is what the keys and the screen reader follow:

- `display: none`, `order`, `content`, reversed flex and positioned grid items
- `url()` of any kind, since a stylesheet never reaches the network

Any property not listed above is ignored and logged once, the way a browser ignores it.

Accessibility sits on top of the cascade, and a sheet cannot turn it off:

- Text below 4.5:1 contrast against what is behind it falls back to the theme's colour.
- `:focus` must stay visible. A focus style that would hide the cursor is not used, and an
  outline is never thinner than 2 px, `outline: none` included.

## Limits

This is not a browser. There is no JavaScript, no absolute positioning, no web fonts, and
no form elements other than the app's own rows. Selectors and properties grow as real plugins
need them.

`<input>` in a row is the app's own editable field, written `<input>text</input>`, which is
not the HTML form (`<input value="text">`).
