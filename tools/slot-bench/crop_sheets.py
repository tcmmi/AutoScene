"""Contact sheets of face crops, for labelling what the faces show.

    uv run crop_sheets.py <list.tsv> <out_dir>

`list.tsv` has a header and one row per crop; its first column is the crop's
file (an absolute path, or one beside the list). Thirty crops per sheet
(`sheet_000.jpg`, …), each with its number under it, in the list's order, and
`index.tsv` (`crop file`) to turn numbers back into files. The list's order
is the labeller's order: shuffle it first, so that nothing about a model's
score can be read from a crop's place.
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFont

SIDE, STRIP, COLS, ROWS = 256, 26, 6, 5


def main():
    listing, out = sys.argv[1:3]
    rows = [line.rstrip("\n").split("\t") for line in open(listing, encoding="utf-8") if line.strip()][1:]
    files = [r[0] if os.path.isabs(r[0]) else os.path.join(os.path.dirname(listing), r[0]) for r in rows]
    os.makedirs(out, exist_ok=True)
    font = ImageFont.load_default(size=20)
    per = COLS * ROWS
    for s in range(0, len(files), per):
        sheet = Image.new("RGB", (COLS * SIDE, ROWS * (SIDE + STRIP)), (16, 16, 16))
        draw = ImageDraw.Draw(sheet)
        for k, path in enumerate(files[s:s + per]):
            x, y = (k % COLS) * SIDE, (k // COLS) * (SIDE + STRIP)
            sheet.paste(Image.open(path).convert("RGB").resize((SIDE, SIDE), Image.BILINEAR), (x, y))
            draw.text((x + 6, y + SIDE + 1), str(s + k), fill=(235, 235, 235), font=font)
        sheet.save(os.path.join(out, f"sheet_{s // per:03d}.jpg"), quality=90)
    with open(os.path.join(out, "index.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("crop\tfile\n")
        for k, path in enumerate(files):
            f.write(f"{k}\t{path}\n")
    print(f"{len(files)} crops on {(len(files) + per - 1) // per} sheets in {out}")


main()
