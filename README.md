<div align="center">

<img src="assets/banner.svg" alt="Snapt: select, annotate, copy" width="100%">

**A fast, lightweight, open-source screenshot tool for Windows, macOS and Linux.**

Press PrtSc, select an area, add an arrow or a caption, then copy it or save it.<br>
No accounts, no cloud, a few megabytes on disk.

[![CI](https://github.com/vkardin/snapt/actions/workflows/ci.yml/badge.svg)](https://github.com/vkardin/snapt/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-8B5CF6.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-5B21B6.svg)

</div>

> [!NOTE]
> Snapt is in early development. Today you can select an area and copy or save it.
> Drawing tools are next.

## Why Snapt

- **Fast.** A native Rust binary that starts instantly and sits idle in the tray.
- **Light.** No Electron, no web view, no runtime to install.
- **Private.** Shots stay on your machine. Snapt never uploads anything.
- **Familiar.** If you have used LightShot, you already know how Snapt works.

## Roadmap

- [x] Tray icon with a menu
- [x] Global PrtSc hotkey
- [x] Region selection on every monitor, HiDPI aware
- [x] Copy to clipboard, save as PNG or JPEG
- [ ] Editor: pen, line, arrow, rectangle, marker, text, colors, undo
- [ ] Print and reverse image search
- [ ] Settings: hotkeys, save folder, image format, start with the system
- [x] Installers: `.exe`, `.msi`, `.dmg`, `.deb` and AppImage
- [ ] Flatpak and code signing

## Install

Download the installer for your system from the
[latest release](https://github.com/vkardin/snapt/releases):
`.exe` or `.msi` for Windows, `.dmg` for macOS, `.deb` or AppImage for Linux.

Snapt is not code-signed yet. On Windows, click **More info → Run anyway** in the
SmartScreen prompt. On macOS, open **System Settings → Privacy & Security** after the
first launch attempt and click **Open Anyway**.

## Usage

Press **PrtSc** or pick **Take screenshot** in the tray menu. The screen freezes, then:

| Action | How |
| --- | --- |
| Select an area | Drag with the left mouse button |
| Move or resize it | Drag inside the selection or by its handles |
| Select the whole screen | `Ctrl+A` |
| Copy to clipboard | `Ctrl+C`, `Enter` or double-click the selection |
| Save to a file | `Ctrl+S` |
| Cancel | `Esc` or right-click |

On macOS, use `Cmd` instead of `Ctrl`.

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
  System Settings → Privacy & Security.
- **Linux:** the tray icon uses the StatusNotifierItem protocol. KDE supports it out of
  the box; on GNOME, install the AppIndicator extension. The global hotkey currently
  works on X11 only. Wayland support is planned through the desktop portal.

## Contributing

Bug reports and ideas are welcome in [Issues](https://github.com/vkardin/snapt/issues).
See [CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test and send a pull request.

## License

[MIT](LICENSE) © vkardin
