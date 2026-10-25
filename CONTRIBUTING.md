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

## Website

The website at [vkardin.github.io/snapt](https://vkardin.github.io/snapt/) is a single static
page in [`site/`](site). Open `site/index.html` in a browser to preview it. Changes to `site/`
are published to GitHub Pages when they land on `main`.

## Guidelines

- Keep Snapt small and fast: think twice before adding a dependency.
- One topic per pull request, with a short description of what changed and why.
- Test on the platforms you can. Mention in the pull request which ones you checked.

By contributing, you agree that your work is released under the [MIT License](LICENSE).
