# Snapt for Chrome and Firefox

The browser side of Snapt: a Manifest V3 extension built from one source for
Chrome (and other Chromium browsers) and Firefox.

- **Save image as PNG / JPG.** Right-click any image, including WebP, AVIF and SVG.
  The image is converted locally; nothing is uploaded.
- **Lightbox.** `Alt`+click any image (`Option`+click on macOS) to open it over the
  page. Scroll to zoom from a thumbnail down to a pixel grid, drag to pan,
  double-click to switch between fit and 100%, `R` / `Shift+R` to rotate,
  `0` to fit, `1` for actual size, `Esc` to close.

## Build

Needs Python 3.

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
- `src/background.js`: the context menu, fetching and saving images.
- `src/convert.js`: PNG / JPG conversion with `OffscreenCanvas`.
- `src/offscreen.*`: Chrome only, decodes SVG for the service worker.
- `src/content.js`: the lightbox and the page side of saving.
- `src/_locales`: English, Russian and Ukrainian.
