"""Two checks of iCartoonFace before it trains anything (`bench/models/README.md`, "Training data").

    uv run audit.py names <idInfo.txt> <out.tsv>
        The identities' names as a table (`id name page`), to find the bench franchises' characters by name.

    uv run audit.py sheets <rectrain.zip> <out_dir> [--items 300] [--seed 7] [--exclude <file>] [--drop <file>] [--heads <heads.tsv>]
        The label check: `--items` pictures drawn at random, each on a row beside six other pictures of its
        identity (`<out_dir>/sheet_NN.jpg`, ten rows a sheet; `items.tsv` says which picture is where). A
        row is wrong when its first picture shows another character than the rest.

    uv run audit.py identity <rectrain.zip> <out.jpg> <identity> [<identity> …] [--heads <heads.tsv>]
        A contact sheet of identities (a row each), to look at the ones a check points at.
"""
import ast
import random
import sys
import urllib.parse
from pathlib import Path

from PIL import Image, ImageDraw

import data

CELL = 128


def names(info, out):
    rows = []
    for line in Path(info).read_text(encoding="utf-8").splitlines():
        if line.strip():
            d = ast.literal_eval(line)
            page = urllib.parse.unquote((d.get("url") or "").split("/item/")[-1].split("?")[0]) if d.get("url") else ""
            rows.append(f"{d['id'].rsplit('_', 1)[-1]}\t{d['name']}\t{page if '/item/' in (d.get('url') or '') else ''}")
    Path(out).write_text("id\tname\tpage\n" + "\n".join(rows) + "\n", encoding="utf-8", newline="\n")
    print(f"{out}: {len(rows)} identities")


def cell(z, member, box):
    import io

    return data.crop(Image.open(io.BytesIO(z.read(member))).convert("RGB"), box).resize((CELL, CELL), Image.BILINEAR)


def by_identity(rows):
    out = {}
    for member, name, box in rows:
        out.setdefault(name, []).append((member, box))
    return out


def sheets(path, out, items, seed, exclude, heads=None, drop=None):
    import zipfile

    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    rows = [r for r in data.index(path, heads, drop) if r[1] not in exclude]
    groups = by_identity(rows)
    rng = random.Random(seed)
    picked = rng.sample(rows, items)
    log = ["item\tsheet\trow\tidentity\tpicture"]
    with zipfile.ZipFile(path) as z:
        for s in range(0, items, 10):
            page = Image.new("RGB", (40 + 7 * CELL + 16, 10 * (CELL + 6)), "black")
            draw = ImageDraw.Draw(page)
            for r, (member, name, box) in enumerate(picked[s : s + 10]):
                y = r * (CELL + 6)
                draw.text((4, y + 4), str(s + r), fill="yellow")
                page.paste(cell(z, member, box), (40, y))
                others = [x for x in groups[name] if x[0] != member]
                for k, (m, b) in enumerate(rng.sample(others, min(6, len(others)))):
                    page.paste(cell(z, m, b), (40 + 16 + (k + 1) * CELL, y))
                log.append(f"{s + r}\t{s // 10:02d}\t{r}\t{name.rsplit('_', 1)[-1]}\t{member.rsplit('/', 1)[-1]}")
            page.save(out / f"sheet_{s // 10:02d}.jpg", quality=88)
    (out / "items.tsv").write_text("\n".join(log) + "\n", encoding="utf-8", newline="\n")
    print(f"{out}: {items} items on {(items + 9) // 10} sheets")


def identity(path, out, wanted, heads=None):
    import zipfile

    groups = by_identity(data.index(path, heads))
    page = Image.new("RGB", (60 + 10 * CELL, len(wanted) * (CELL + 6)), "black")
    draw = ImageDraw.Draw(page)
    with zipfile.ZipFile(path) as z:
        for r, short in enumerate(wanted):
            name = f"personai_icartoonface_rectrain_{int(short):05d}"
            draw.text((4, r * (CELL + 6) + 4), f"{int(short):05d}", fill="yellow")
            got = groups[name]
            for k, (m, b) in enumerate(got[:: max(1, len(got) // 10)][:10]):
                page.paste(cell(z, m, b), (60 + k * CELL, r * (CELL + 6)))
    page.save(out, quality=88)
    print(f"{out}: {len(wanted)} identities")


def main():
    a = sys.argv[1:]
    if not a:
        sys.exit(__doc__)
    flag = lambda name, default: a[a.index(name) + 1] if name in a else default
    if "--heads" in a:
        heads = flag("--heads", None)
        del a[a.index("--heads") : a.index("--heads") + 2]
    else:
        heads = None
    if a[0] == "names":
        names(a[1], a[2])
    elif a[0] == "sheets":
        exclude = data.read_exclude(flag("--exclude", None))
        sheets(a[1], a[2], int(flag("--items", 300)), int(flag("--seed", 7)), exclude, heads, flag("--drop", None))
    elif a[0] == "identity":
        identity(a[1], a[2], a[3:], heads)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
