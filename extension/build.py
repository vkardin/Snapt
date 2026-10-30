#!/usr/bin/env python3
"""Builds the browser extension for Chrome and Firefox.

    python3 extension/build.py

writes an unpacked extension to extension/dist/chrome and extension/dist/firefox,
ready for "Load unpacked" / "Load Temporary Add-on", and a zip of each for the
Chrome Web Store and addons.mozilla.org.
"""

import json
import shutil
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SRC = ROOT / "src"
DIST = ROOT / "dist"

# Files only Chrome uses: Firefox's event page has a DOM of its own.
CHROME_ONLY = {"offscreen.html", "offscreen.js"}

FIREFOX_ID = "snapt@vkardin.github.io"


def chrome(manifest):
    return manifest


def firefox(manifest):
    manifest["background"] = {"scripts": ["convert.js", "background.js"]}
    manifest["permissions"] = [p for p in manifest["permissions"] if p != "offscreen"]
    manifest["browser_specific_settings"] = {
        "gecko": {
            "id": FIREFOX_ID,
            "strict_min_version": "128.0",
            "data_collection_permissions": {"required": ["none"]},
        }
    }
    return manifest


def build(name, adapt, skip):
    out = DIST / name
    shutil.rmtree(out, ignore_errors=True)
    shutil.copytree(SRC, out, ignore=lambda _dir, names: [n for n in names if n in skip])
    manifest = adapt(json.loads((SRC / "manifest.json").read_text(encoding="utf-8")))
    (out / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    archive = DIST / f"snapt-{name}-{manifest['version']}.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
        for path in sorted(out.rglob("*")):
            if path.is_file():
                zf.write(path, path.relative_to(out).as_posix())
    print(f"{out.relative_to(ROOT.parent)}  ->  {archive.relative_to(ROOT.parent)}")


def main():
    DIST.mkdir(exist_ok=True)
    build("chrome", chrome, set())
    build("firefox", firefox, CHROME_ONLY)


if __name__ == "__main__":
    main()
