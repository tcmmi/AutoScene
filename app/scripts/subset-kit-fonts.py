"""Writes the Romanian letters outside Latin-1 into the plugin kit's fonts.

The kit inlines the app's three faces, Latin only. Romanian needs ă (U+0102-0103) and
ș ț (U+0218-021B) besides, which sit in the faces' latin-ext files; â and î are in
Latin-1. Those files are 12-133 KB each, so this cuts each down to the six glyphs
into src/plugin-kit/fonts/, which fonts.css then names.

Run it when @fontsource packages are bumped. The tools are pinned, so the same
fonts give the same bytes (pip install fonttools==4.64.0 brotli==1.2.1):
    python app/scripts/subset-kit-fonts.py
It also writes src/plugin-kit/fonts/sources.json: the @fontsource versions the
files were cut from and their hashes. The kit test fails when package.json names
other versions, so a bump cannot keep old Romanian glyphs beside new Latin ones.
"""

import hashlib
import json
from importlib.metadata import version
from pathlib import Path

from fontTools import subset

TOOLS = {"fonttools": "4.64.0", "brotli": "1.2.1"}
for tool, pinned in TOOLS.items():
    if version(tool) != pinned:
        raise SystemExit(f"{tool} {version(tool)} is installed, this script is pinned to {pinned}")

ROOT = Path(__file__).resolve().parent.parent
MODULES = ROOT / "node_modules"
OUT = ROOT / "src" / "plugin-kit" / "fonts"
UNICODES = "U+0102-0103,U+0218-021B"

SOURCES = {
    "inter-ro-opsz-normal": "@fontsource-variable/inter/files/inter-latin-ext-opsz-normal.woff2",
    "instrument-serif-ro-400-normal": "@fontsource/instrument-serif/files/instrument-serif-latin-ext-400-normal.woff2",
    "instrument-serif-ro-400-italic": "@fontsource/instrument-serif/files/instrument-serif-latin-ext-400-italic.woff2",
    "geist-mono-ro-wght-normal": "@fontsource-variable/geist-mono/files/geist-mono-latin-ext-wght-normal.woff2",
}

OUT.mkdir(parents=True, exist_ok=True)
files = {}
for name, source in SOURCES.items():
    options = subset.Options()
    options.flavor = "woff2"
    options.layout_features = ["kern", "mark", "mkmk"]
    options.notdef_outline = False
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    font = subset.load_font(str(MODULES / source), options)
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=subset.parse_unicodes(UNICODES))
    subsetter.subset(font)
    target = OUT / f"{name}.woff2"
    subset.save_font(font, str(target), options)
    print(f"{target.name}: {target.stat().st_size} bytes")
    files[target.name] = hashlib.sha256(target.read_bytes()).hexdigest()

packages = sorted({source.split("/files/")[0] for source in SOURCES.values()})
manifest = {
    "tools": TOOLS,
    "fontsource": {pkg: json.loads((MODULES / pkg / "package.json").read_text(encoding="utf8"))["version"] for pkg in packages},
    "sha256": files,
}
(OUT / "sources.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf8", newline="\n")
