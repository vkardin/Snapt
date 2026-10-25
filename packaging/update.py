#!/usr/bin/env python3
"""Points every package manifest in this folder at a release.

    python3 packaging/update.py 1.4.0

Reads the checksums from the release's SHA256SUMS.txt, hashes the source
tarball and LICENSE, and rewrites the versions, URLs and hashes in the
winget, Homebrew, AUR and Flatpak files. Check the diff, then publish each
one as packaging/README.md describes.
"""

import datetime
import hashlib
import json
import pathlib
import re
import sys
import urllib.request

REPO = "vkardin/snapt"
HERE = pathlib.Path(__file__).resolve().parent


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "snapt-packaging"})
    with urllib.request.urlopen(request) as response:
        return response.read()


def sha256(url: str) -> str:
    return hashlib.sha256(fetch(url)).hexdigest()


def edit(path: pathlib.Path, *pairs: tuple[str, str]) -> None:
    text = path.read_text()
    for pattern, replacement in pairs:
        text, count = re.subn(pattern, replacement, text, flags=re.M)
        if count == 0:
            sys.exit(f"{path.name}: nothing matches {pattern!r}")
    path.write_text(text)
    print(f"updated {path.relative_to(HERE)}")


def main() -> None:
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+", sys.argv[1]):
        sys.exit(__doc__)
    v = sys.argv[1]
    base = f"https://github.com/{REPO}/releases/download/v{v}"
    sums = {}
    for line in fetch(f"{base}/SHA256SUMS.txt").decode().splitlines():
        digest, name = line.split()
        sums[name.lstrip("*")] = digest

    def asset(name: str) -> str:
        if name not in sums:
            sys.exit(f"the release has no {name}")
        return sums[name]

    tag = json.loads(fetch(f"https://api.github.com/repos/{REPO}/git/ref/tags/v{v}"))
    commit = tag["object"]["sha"]
    if tag["object"]["type"] == "tag":
        commit = json.loads(fetch(tag["object"]["url"]))["object"]["sha"]
    tarball = sha256(f"https://github.com/{REPO}/archive/refs/tags/v{v}.tar.gz")
    license_sum = sha256(f"https://raw.githubusercontent.com/{REPO}/v{v}/LICENSE")
    today = datetime.date.today().isoformat()

    version = (r"^(PackageVersion: ).*$", rf"\g<1>{v}")
    edit(HERE / "winget/vkardin.Snapt.yaml", version)
    edit(
        HERE / "winget/vkardin.Snapt.installer.yaml",
        version,
        (r"^(ReleaseDate: ).*$", rf"\g<1>{today}"),
        (r"^(    InstallerUrl: ).*$", rf"\g<1>{base}/snapt_{v}_x64-setup.exe"),
        (r"^(    InstallerSha256: ).*$", rf"\g<1>{asset(f'snapt_{v}_x64-setup.exe').upper()}"),
    )
    edit(
        HERE / "winget/vkardin.Snapt.locale.en-US.yaml",
        version,
        (r"^(ReleaseNotesUrl: .*/tag/v).*$", rf"\g<1>{v}"),
    )
    edit(
        HERE / "homebrew/snapt.rb",
        (r'^(  version ").*(")$', rf"\g<1>{v}\g<2>"),
        (r'(arm:   ")[0-9a-f]{64}', rf"\g<1>{asset(f'Snapt_{v}_aarch64.dmg')}"),
        (r'(intel: ")[0-9a-f]{64}', rf"\g<1>{asset(f'Snapt_{v}_x64.dmg')}"),
    )
    for name, sums_line in [
        ("snapt-bin", rf"\g<1>{asset(f'snapt_{v}_amd64.deb')}\g<2>"),
        ("snapt", rf"\g<1>{tarball}\g<2>"),
    ]:
        pkgbuild = [(r"^(pkgver=).*$", rf"\g<1>{v}"), (r"^(pkgrel=).*$", r"\g<1>1")]
        srcinfo = [
            (r"^(\tpkgver = ).*$", rf"\g<1>{v}"),
            (r"^(\tpkgrel = ).*$", r"\g<1>1"),
            (r"\d+\.\d+\.\d+", v),
        ]
        if name == "snapt-bin":
            pkgbuild += [
                (r"^(sha256sums=\(')[0-9a-f]{64}(')$", sums_line),
                (r"^(            ')[0-9a-f]{64}('\))$", rf"\g<1>{license_sum}\g<2>"),
            ]
            srcinfo += [
                (r"^(\tsha256sums = )[0-9a-f]{64}()\n(\tsha256sums = )[0-9a-f]{64}$",
                 rf"\g<1>{asset(f'snapt_{v}_amd64.deb')}\n\g<3>{license_sum}"),
            ]
        else:
            pkgbuild += [(r"^(sha256sums=\(')[0-9a-f]{64}('\))$", sums_line)]
            srcinfo += [(r"^(\tsha256sums = )[0-9a-f]{64}()$", sums_line)]
        edit(HERE / f"aur/{name}/PKGBUILD", *pkgbuild)
        edit(HERE / f"aur/{name}/.SRCINFO", *srcinfo)
    edit(
        HERE / "flatpak/io.github.vkardin.snapt.yml",
        (r"^(        tag: v).*$", rf"\g<1>{v}"),
        (r"^(        commit: ).*$", rf"\g<1>{commit}"),
    )
    metainfo = HERE / "flatpak/io.github.vkardin.snapt.metainfo.xml"
    if f'<release version="{v}"' not in metainfo.read_text():
        edit(metainfo, (r"^(  <releases>)$", rf'\g<1>\n    <release version="{v}" date="{today}"/>'))


if __name__ == "__main__":
    main()
