# Packages

Manifests for the package managers that distribute Snapt besides the
[GitHub Releases](https://github.com/vkardin/snapt/releases). They all install the files that
the release workflow builds, except Flatpak and the `snapt` AUR package, which build from the
tagged source.

| Channel | Files | Users install with |
| --- | --- | --- |
| winget (Windows) | [`winget/`](winget) | `winget install vkardin.Snapt` |
| Homebrew (macOS) | [`homebrew/snapt.rb`](homebrew/snapt.rb) | `brew install vkardin/snapt/snapt` |
| AUR (Arch Linux) | [`aur/snapt-bin`](aur/snapt-bin), [`aur/snapt`](aur/snapt) | `yay -S snapt-bin` |
| Flathub (Linux) | [`flatpak/`](flatpak) | `flatpak install flathub io.github.vkardin.snapt` |

## After each release

```sh
python3 packaging/update.py 1.4.0
git diff packaging
```

The script reads the checksums from the release's `SHA256SUMS.txt` and rewrites the versions,
URLs and hashes in every file here. Then publish each channel as below.

## Publishing

Each channel lives outside this repository and needs an account of the maintainer, so the
first submission is done by hand.

### winget

Copy the three files into `manifests/v/vkardin/Snapt/<version>/` of a fork of
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) and open a pull request;
`winget validate --manifest packaging/winget` checks them first. Microsoft's bots test the
installer, and a moderator merges it, usually within a few days.
Later versions can be sent with `wingetcreate update vkardin.Snapt --version <version> --urls <exe url> --submit`
or the [winget-releaser](https://github.com/vedantmgoyal9/winget-releaser) action with a
token of the maintainer's account.

### Homebrew

Unsigned apps can't go into the main `homebrew/cask` repository, so Snapt has its own tap:
create a public repository `vkardin/homebrew-snapt` with `Casks/snapt.rb`. Users then run
`brew install vkardin/snapt/snapt`. As the app is not notarized, macOS still asks on the first
launch; the cask's caveat tells how to allow it. Once Snapt is notarized (see
[Code signing](#code-signing)), the cask can be offered to `homebrew/cask`.

### AUR

`snapt-bin` repackages the `.deb`; `snapt` builds from source. With an
[AUR account](https://aur.archlinux.org/register) and an SSH key added to it:

```sh
git clone ssh://aur@aur.archlinux.org/snapt-bin.git
cp packaging/aur/snapt-bin/{PKGBUILD,.SRCINFO} snapt-bin/
cd snapt-bin && makepkg -si && git add -A && git commit -m "Update to <version>" && git push
```

The same for `snapt`. After editing a `PKGBUILD` by hand, regenerate `.SRCINFO` with
`makepkg --printsrcinfo > .SRCINFO`.

### Flathub

Flathub builds offline, so the crates are listed in `cargo-sources.json`, generated with
[flatpak-cargo-generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo):

```sh
cd packaging/flatpak
python3 flatpak-cargo-generator.py ../../Cargo.lock -o cargo-sources.json
flatpak-builder --user --install --force-clean build io.github.vkardin.snapt.yml
flatpak run io.github.vkardin.snapt
```

To publish, follow [Flathub's submission guide](https://docs.flathub.org/docs/for-app-authors/submission):
a pull request to the `new-pr` branch of [flathub/flathub](https://github.com/flathub/flathub)
with the manifest, `cargo-sources.json`, the `.desktop` and the metainfo files. After review,
Flathub creates `flathub/io.github.vkardin.snapt`, where later versions are updated.
The app ID `io.github.vkardin.snapt` is verified through the `vkardin` GitHub account.

In the sandbox, Snapt takes screenshots, gets its shortcut and starts at login through the
desktop portals, and its own updates are turned off. Text recognition needs Tesseract, which
the sandbox doesn't have, and replacing the GNOME screenshot key needs `gsettings`, which it
can't reach either.

## Code signing

Without a signature Windows SmartScreen warns about the installer and macOS Gatekeeper refuses
to open the app until the user allows it. Options, cheapest first:

**Windows**

- **[SignPath Foundation](https://signpath.org)**: free code signing for open-source projects.
  After an application, a GitHub Actions step sends the built installers to SignPath, and
  they come back signed by "SignPath Foundation" with Snapt named in the certificate.
  Requirements: an OSI license (MIT is fine), builds only from CI, and a short review of the
  project. Best fit for Snapt.
- **[Certum Open Source Code Signing](https://shop.certum.eu/open-source-code-signing.html)**:
  a certificate in the maintainer's own name for about €30–70 a year, with the key in Certum's
  cloud (SimplySign) or on a smart card. Signing in CI is possible but awkward with SimplySign.
- **[Azure Trusted Signing](https://learn.microsoft.com/azure/trusted-signing/)**: about $10 a month and fast to set up in CI, but individual developers can
  only use it from some countries; check before relying on it.

Since 2024, no certificate removes the SmartScreen warning on day one: reputation grows with
downloads of signed files. winget doesn't need a signature.

**macOS**

- **Apple Developer Program**, $99 a year: a Developer ID certificate to sign the app and
  notarization, after which macOS opens it without warnings. cargo-packager signs and notarizes
  when the release workflow gets the certificate and an App Store Connect API key as secrets
  (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_API_KEY`, `APPLE_API_ISSUER`,
  `APPLE_API_KEY_PATH`). It is also what the main Homebrew cask repository needs.
- Without it, builds stay ad-hoc signed, as now.

**All platforms, free**

- [GitHub artifact attestations](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations):
  one step in the release workflow (`actions/attest-build-provenance`) records that each file
  was built by this repository's workflow, and anyone can check it with
  `gh attestation verify <file> --repo vkardin/snapt`. It doesn't silence SmartScreen or
  Gatekeeper, but proves where a download came from.
