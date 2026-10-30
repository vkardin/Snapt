# Snapt for Chrome and Firefox

The browser side of Snapt: a Manifest V3 extension built from one source for
Chrome (and other Chromium browsers) and Firefox.

- **Screenshots.** From the toolbar button or a shortcut: select an area
  (`Alt+Shift+S`; drag, or click an element to take just it), the visible part
  (`Alt+Shift+V`) or the whole page, scrolled and glued into one image
  (`Alt+Shift+P`). Shortcuts can be changed in the browser's extension settings.
- **Editor.** Pen, line, arrow, rectangle, ellipse, marker, text, numbered steps,
  pixelate and blur, with the desktop app's keys (`P L A R E M T N B U`, `F` for
  fill), undo and redo, then copy (`Ctrl+C`) or save as PNG (`Ctrl+S`) or JPG
  (`Ctrl+Shift+S`). Shapes are drawn by Snapt's own Rust code
  (`src/overlay/annotate.rs` and `src/font.rs`), compiled to WebAssembly, so they
  look exactly as in the desktop app.
- **Save image as PNG / JPG.** Right-click any image, including WebP, AVIF and SVG.
  The image is converted locally; nothing is uploaded.
- **Lightbox.** `Alt`+click any image (`Option`+click on macOS) to open it over the
  page. Scroll to zoom from a thumbnail down to a pixel grid, drag to pan,
  double-click to switch between fit and 100%, `R` / `Shift+R` to rotate,
  `0` to fit, `1` for actual size, `Esc` to close.

## Build

Needs Python 3 and Rust with the WebAssembly target
(`rustup target add wasm32-unknown-unknown`).

```sh
python3 extension/build.py
```

This writes the unpacked extension to `extension/dist/chrome` and
`extension/dist/firefox`, and a zip of each for the stores.

- **Chrome:** open `chrome://extensions`, turn on Developer mode, click
  **Load unpacked** and pick `extension/dist/chrome`.
- **Firefox:** open `about:debugging#/runtime/this-firefox`, click
  **Load Temporary Add-on** and pick `extension/dist/firefox/manifest.json`.

## Layout

- `src/manifest.json`: the shared manifest; `build.py` adapts it for Firefox
  (an event page instead of a service worker, the add-on id).
- `wasm/`: the editor core, a thin C-style API over the desktop app's modules.
- `src/background.js`: captures, the context menu, fetching and saving images.
- `src/popup.*`: the toolbar popup.
- `src/editor.*`: the editor page.
- `src/convert.js`: PNG / JPG conversion with `OffscreenCanvas`.
- `src/offscreen.*`: Chrome only, decodes SVG for the service worker.
- `src/content.js`: the lightbox, area selection, scrolling for whole-page
  captures and the page side of saving.
- `src/_locales`: English, Russian and Ukrainian.

Store texts, screenshots and permission justifications are in
[store/](store/README.md).
