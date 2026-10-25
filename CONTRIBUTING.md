# Contributing to Snapt

Thanks for helping make Snapt better! Bug reports, ideas and pull requests are all welcome.

## Reporting bugs and ideas

Open an [issue](https://github.com/vkardin/snapt/issues/new/choose) and pick a template.
For bugs, include your OS and the output of Snapt run with `RUST_LOG=debug`.

## Development setup

1. Install the stable [Rust toolchain](https://rustup.rs).
2. On Linux, install the system libraries listed in the [README](README.md#building-from-source).
3. Run `cargo run` to start Snapt in the tray.

## Before you open a pull request

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same checks on Windows, macOS and Linux.

### Snapshot tests

Some tests draw the overlay, a saved image and the settings window and compare them with
the PNGs in [`tests/snapshots`](tests/snapshots). If you change how something looks on
purpose, regenerate them, look at the new pictures and commit them:

```sh
SNAPT_UPDATE_SNAPSHOTS=1 cargo test
```

A failing test saves what it drew and a diff (changed pixels in red) to
`target/snapshots`; in CI they are uploaded as the `snapshots-<OS>` artifact.

## Website

The website at [vkardin.github.io/snapt](https://vkardin.github.io/snapt/) is a static page
in [`site/`](site), in English (`index.html`), Russian (`ru/`) and Ukrainian (`uk/`). The pages share
`style.css` and `app.js`; when you change the text, change all three. Preview it with
`python3 -m http.server -d site` and open http://localhost:8000. Changes to `site/`
are published to GitHub Pages when they land on `main`.

## Guidelines

- Keep Snapt small and fast: think twice before adding a dependency.
- One topic per pull request, with a short description of what changed and why.
- Test on the platforms you can. Mention in the pull request which ones you checked.

By contributing, you agree that your work is released under the [MIT License](LICENSE).
