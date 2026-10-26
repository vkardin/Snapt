<div align="center">

<img src="assets/banner.svg" alt="Snapt: select, annotate, copy" width="100%">

**A fast, lightweight, open-source screenshot tool for Windows, macOS and Linux.**

Press PrtSc, select an area, add an arrow or a caption, then copy it or save it.<br>
No accounts, no cloud, a few megabytes on disk.

**[Website](https://vkardin.github.io/snapt/) ([RU](https://vkardin.github.io/snapt/ru/), [UA](https://vkardin.github.io/snapt/uk/)) · [Download](https://vkardin.github.io/snapt/#download) · [Report a bug](https://github.com/vkardin/snapt/issues)**

[![CI](https://github.com/vkardin/snapt/actions/workflows/ci.yml/badge.svg)](https://github.com/vkardin/snapt/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-8B5CF6.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-5B21B6.svg)
[![Release](https://img.shields.io/github/v/release/vkardin/snapt?include_prereleases&color=7C3AED)](https://github.com/vkardin/snapt/releases)
[![Downloads](https://img.shields.io/github/downloads/vkardin/snapt/total?color=7C3AED)](https://github.com/vkardin/snapt/releases)
![Rust](https://img.shields.io/badge/made%20with-Rust-B7410E.svg)

</div>

## Why Snapt

- **Fast.** A native Rust binary that starts instantly and sits idle in the tray.
- **Light.** No Electron, no web view, no runtime to install.
- **Private.** Shots stay on your machine. Snapt never uploads anything.
- **Familiar.** If you have used LightShot, you already know how Snapt works.

## Features

- Select an area on any monitor, or a whole window with one click, HiDPI aware, with
  handles to move and resize it and a magnifier for pixel-exact edges.
- Draw on it: pen, line, arrow, rectangle, ellipse, marker, text and numbered steps, in
  any color, or one picked from the screen with the eyedropper. Pixelate or blur what should
  stay private, and fill rectangles and ellipses with color. Click a shape to move it, or delete it with `Delete`.
- A color picker on its own shortcut (`Ctrl+Alt+C`) or in the tray: a magnifier shows the
  pixels under the cursor and a click copies the color as HEX or RGB.
- Copy the text in the selection, recognized by the system's own OCR (English, Russian,
  Ukrainian and more).
- Copy it, save it as PNG or JPEG, print it, or search Google Lens for similar images.
- Pin it on top of all windows to keep it in sight while you work, as in Snipaste.
- Scrolling screenshots (experimental): scroll a long page and Snapt glues it into one image.
- Record an area as a GIF, or as an MP4 when [ffmpeg](https://ffmpeg.org) is installed.
- From the tray: copy the full screen in one click, take a screenshot after 3, 5 or 10
  seconds, select the last area again, and reopen your recent screenshots.
- A settings window with three pages: general options, a shortcut for every action, and
  switches to turn off the features you don't use.
- In English, Russian and Ukrainian, and it updates itself from GitHub Releases.

Planned: Flatpak and code-signed installers.

## Install

Download the installer for your system from the
[website](https://vkardin.github.io/snapt/#download) or the
[latest release](https://github.com/vkardin/snapt/releases):
`.exe` or `.msi` for Windows, `.dmg` for macOS, `.deb` or AppImage for Linux.

Snapt is not code-signed yet. On Windows, click **More info → Run anyway** in the
SmartScreen prompt. On macOS, open **System Settings → Privacy & Security** after the
first launch attempt and click **Open Anyway**.

## Usage

Press **PrtSc** (**Cmd+Shift+9** on macOS) or pick **Take screenshot** in the tray menu.
The screen freezes, then:

| Action | How |
| --- | --- |
| Select an area | Drag with the left mouse button |
| Select a window | Point at it and click |
| Move or resize it | Drag inside the selection or by its handles |
| Select the whole screen | `Ctrl+A` |
| Pick a drawing tool | Click it in the panel, or press `P` pen, `L` line, `A` arrow, `R` rectangle, `E` ellipse, `M` marker, `T` text, `N` numbered step, `B` pixelate, `U` blur |
| Filled rectangles and ellipses | `F` switches filling on and off |
| Move or delete a shape | With no tool picked, drag it, or click it and press `Delete` |
| Straight lines, squares | Hold `Shift` while drawing |
| Line width, text size | Mouse wheel |
| Undo, redo | `Ctrl+Z`, `Ctrl+Y` or `Ctrl+Shift+Z` |
| Copy to clipboard | `Ctrl+C`, `Enter` or double-click the selection |
| Save to a file | `Ctrl+S` |
| Print | `Ctrl+P` |
| Search for similar images | `Ctrl+F` |
| Copy the text in it | `Ctrl+Shift+C` |
| Pin it on top of all windows | `F3` |
| Scrolling screenshot (experimental) | `Ctrl+Shift+S`, then scroll the page down |
| Record a video of the area | `Ctrl+Shift+R`; `Enter` stops, `Esc` cancels |
| Pick a color from the screen | `I`, then click; the HEX code is copied too |
| Cancel | `Esc` or right-click |

On macOS, use `Cmd` instead of `Ctrl`. While typing text, `Enter` starts a new line and
`Esc` finishes the text.

A pinned screenshot stays on top of other windows. Drag it to move it, zoom it with the mouse
wheel or `+` and `-` (`0` goes back to 100 %), copy it again with `Ctrl+C`, save it with
`Ctrl+S`, and close it with `Esc`, a double click or the middle mouse button.

For a scrolling screenshot, select the visible part of the page and press `Ctrl+Shift+S` or
the scroll button, then scroll down at an even pace. A panel shows the height so far; press
`Enter` or click it when done (`Esc` cancels). The result is copied and pinned so you can
check it and save it with `Ctrl+S`. It is experimental: sticky headers and animations can
confuse the gluing.

To record a video, select an area and press `Ctrl+Shift+R` or the camera button. Snapt records
10 frames a second, for up to two minutes; the panel shows the time. Press `Enter` or click
the panel to stop and choose where to save it: MP4 if `ffmpeg` is on the `PATH`, GIF always.

The tray menu also has:

- **Copy full screen**: all monitors at once, straight to the clipboard;
- **Repeat last area**: opens the screenshot with the area you selected last time;
- **Screenshot with delay**: in 3, 5 or 10 seconds, to catch menus and tooltips;
- **Recent screenshots**: the last 10, kept between restarts; click one to pin it.

### Settings

Pick **Settings…** in the tray menu. There you can:

- switch the language: English, Russian or Ukrainian (it follows the system by default);
- on **Shortcuts**, give every action its own keys: capture, color picker, full screen,
  repeat last area, and copy text, scrolling screenshot, recording, pin, copy and save on the open
  screenshot. Click a field and press the keys; Backspace clears it. Snapt tells you when
  the keys are already taken;
- on **Features**, turn off text recognition, scrolling screenshots, pinning, image search,
  printing, video recording or the color picker; their buttons and shortcuts go away;
- let Snapt replace the system screenshot tool: on Windows, PrtSc stops opening the Snipping
  Tool; on GNOME, the shortcut opens Snapt instead of GNOME Screenshot, on Wayland too; on
  macOS and other desktops, the window explains the one manual step;
- choose the screenshots folder and format, save without a dialog, also save what you copy,
  open a file once it is saved, and start at login;
- keep Snapt up to date: it checks GitHub Releases once a day and offers **Update** in the
  tray and in the window. The Windows installer, the macOS app and the AppImage update
  themselves; for the `.deb`, the download page opens.

The settings are stored in a plain-text `settings.ini` in the config folder. Two more
settings live only there: `history` sets how many recent screenshots to keep (0 keeps none),
and `file_name` sets how saved files are named, with `strftime` codes such as `%Y` year,
`%m` month, `%d` day, `%H` hour, `%M` minute and `%S` second. The default is
`Snapt_%Y-%m-%d_%H-%M-%S`.

### Command line

`snapt --capture` takes a screenshot right away, `snapt --pick` opens the color picker and
`snapt --settings` opens the settings.
If Snapt is already running, that copy does it, so the command can be bound to a key in the
system settings, for example on Wayland desktops without a global shortcuts portal.

`snapt --logs` opens the folder with the log of the last run (`snapt.log`) and a
`crash-<time>.txt` report for each crash. Attach them to a bug report.

## Building from source

You need a recent stable [Rust toolchain](https://rustup.rs).

```sh
git clone https://github.com/vkardin/snapt
cd snapt
cargo run --release
```

On Debian or Ubuntu, install the system libraries first:

```sh
sudo apt install libxcb1-dev libxcb-randr0-dev libxcb-shm0-dev libdbus-1-dev \
  libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev clang
```

Set `RUST_LOG=debug` to see detailed logs.

To build the installers for your platform, install
[cargo-packager](https://github.com/crabnebula-dev/cargo-packager) and run:

```sh
cargo install cargo-packager --locked
cargo build --release
cargo packager --release
```

They land in `target/packages`. Pushing a `v*` tag builds them for all platforms in CI
and publishes a GitHub Release.

## Platform notes

- **macOS:** the first capture asks for the Screen Recording permission in
  System Settings → Privacy & Security. Restart Snapt after allowing it.
- **Linux:** the tray icon uses the StatusNotifierItem protocol. KDE supports it out of
  the box; on GNOME, install the AppIndicator extension. Text recognition uses Tesseract:
  `sudo apt install tesseract-ocr tesseract-ocr-rus tesseract-ocr-ukr`.
- **Wayland:** Snapt takes screenshots and gets its shortcut through the desktop portal
  (xdg-desktop-portal). The first time, the desktop may ask you to allow screenshots and
  to confirm the shortcut; you can change the key later in the desktop's keyboard
  settings. Global shortcuts need a portal that has them, such as KDE Plasma 6 or
  GNOME 48 and later. Elsewhere, bind `snapt --capture` to a key in the keyboard settings.
- **Windows:** text recognition works for the languages added in Settings → Time & language
  that include it.

## Contributing

Bug reports and ideas are welcome in [Issues](https://github.com/vkardin/snapt/issues).
See [CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test and send a pull request.

## Support

Snapt is free and has no ads. If it saves you time, you can support development with crypto on the
[website](https://vkardin.github.io/snapt/#donate).

## License

[MIT](LICENSE) © vkardin

Snapt bundles a subset of the [Inter](https://github.com/rsms/inter) typeface,
licensed under the [SIL Open Font License](assets/fonts/OFL.txt).
