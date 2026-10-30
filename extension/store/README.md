# Store submission

Everything needed to publish the extension, apart from the developer accounts.

1. `python3 extension/build.py` and take the two zips from `extension/dist/`:
   `snapt-chrome-<version>.zip` for the Chrome Web Store (also accepted by Edge
   Add-ons and Opera), `snapt-firefox-<version>.zip` for addons.mozilla.org.
2. Store texts: [English](listing-en.md), [Russian](listing-ru.md),
   [Ukrainian](listing-uk.md). The name and the short description come from
   `src/_locales`, so each store shows them in the visitor's language.
3. Screenshots (1280 × 800): `screenshots/`. Small promo tile (440 × 280):
   `promo-440x280.png`.
4. Privacy: the extension collects no data and makes no network requests of its
   own. On the Chrome Web Store, answer "does not collect" for every data type
   and paste the permission justifications below. On AMO the manifest already
   declares `data_collection_permissions: none`.
5. AMO asks for the source code because of the WebAssembly module: point the
   reviewer to this repository and `extension/README.md` (build with
   `python3 extension/build.py`; Rust stable with the `wasm32-unknown-unknown`
   target and Python 3).

## Single purpose

Taking, annotating and saving screenshots and images in the browser.

## Permission justifications

- **Host permission `<all_urls>`**: `tabs.captureVisibleTab` needs it to take a
  screenshot of the current page; the content script on every page provides
  area selection, whole-page scrolling and the Alt+click image viewer; images
  saved as PNG/JPG are fetched with it so that cross-origin images can be
  converted.
- **contextMenus**: the "Save image as PNG / JPG" items on images.
- **downloads**: saving screenshots and converted images to disk.
- **scripting**: loading the content script into tabs that were open before
  the extension was installed, when the user takes a screenshot of them.
- **offscreen** (Chrome only): decoding SVG images, which the service worker
  can't, to save them as PNG or JPG.
- **Remote code**: none. The WebAssembly module (`snapt.wasm`) is built from
  this repository and ships inside the package.
