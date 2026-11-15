"""Cast sheets for the readers of `speak_sheets.py`: who is who in an episode.

    uv run speak_cast.py project <ffmpeg> <video> <project_copy_dir> <media> <out_dir>
    uv run speak_cast.py labels <ffmpeg> <video> <identity labels.tsv> <frames> <out_dir>
    uv run speak_cast.py sheet <out_dir> <cast.tsv> <cast_prefix>

`project` and `labels` cut face pictures of the people to choose from:
`project` for every stored character with three face tracks or more in the
episode (its four best tracks, each one's best detection), `labels` for every
name of an identity label file with three tracks or more (its four tracks
with the biggest faces, at the middle box). They write `<key>_<k>.jpg` and
`candidates.tsv` (`key name tracks seconds`), and `candidates_<n>.jpg` to
look at; `key` is `c<number>` for a stored character and the name for a label.

`sheet` draws the readers' sheet from a table made by eye with the columns
`label keys note`: one cell per label with up to three pictures (of its keys
in turn) and the label under them, fifteen labels per page
(`<cast_prefix>_0.jpg`, …).

The video is decoded once, every frame from frame 0 at its own size.
"""
import os
import sqlite3
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

GROW, SIZE, PER, MIN_TRACKS = 1.8, 192, 4, 3
DET = np.dtype([("frame", "<u4"), ("x1", "<f4"), ("y1", "<f4"), ("x2", "<f4"), ("y2", "<f4"),
                ("score", "<f4"), ("norm", "<f4"), ("quality", "<f4")])


def from_project(project, nth):
    """`{key: (name, tracks, seconds, [(frame, box), ...])}` and the frame count."""
    db = sqlite3.connect(f"file:{project}/project.db?mode=ro", uri=True)
    mid, frames, fps = db.execute(
        "SELECT id, frames, CAST(fps_num AS REAL) / fps_den FROM media ORDER BY position, id").fetchall()[nth]
    rows = db.execute(
        "SELECT c.number, c.name, t.quality, t.dets, t.end - t.start FROM tracks t JOIN characters c ON c.id = t.character_id"
        " WHERE t.media_id = ? AND t.not_face = 0 AND t.via_body = 0 AND t.assign IN ('auto', 'pinned')"
        " ORDER BY c.number, t.quality DESC, t.id", (mid,)).fetchall()
    out = {}
    for number, name, _, dets, length in rows:
        entry = out.setdefault(f"c{number}", [name or f"Character {number}", 0, 0.0, []])
        entry[1] += 1
        entry[2] += length / fps
        if len(entry[3]) < PER:
            dets = np.frombuffer(dets, DET)
            d = dets[np.argmax(dets["quality"])]
            entry[3].append((int(d["frame"]), (float(d["x1"]), float(d["y1"]), float(d["x2"]), float(d["y2"]))))
    return {k: tuple(v) for k, v in out.items() if v[1] >= MIN_TRACKS}, frames


def from_labels(path, fps=24000 / 1001):
    out = {}
    for line in list(open(path, encoding="utf-8"))[1:]:
        f = line.rstrip("\n").split("\t")
        if len(f) < 4 or f[2] in ("?", "?e", "?u", "x", "n") or not f[3].strip():
            continue
        boxes = [b.split(":") for b in f[3].split(" ") if b]
        frame, at = boxes[len(boxes) // 2]
        box = tuple(float(v) for v in at.split(","))
        entry = out.setdefault(f[2], [f[2], 0, 0.0, []])
        entry[1] += 1
        entry[2] += (int(f[1]) - int(f[0])) / fps
        entry[3].append((int(frame), box))
    for entry in out.values():
        entry[3] = sorted(entry[3], key=lambda p: -(p[1][3] - p[1][1]))[:PER]
    return {k: tuple(v) for k, v in out.items() if v[1] >= MIN_TRACKS}


def decode(ffmpeg, video):
    cmd = [ffmpeg, "-v", "error", "-i", video, "-map", "0:v:0", "-fps_mode", "passthrough",
           "-pix_fmt", "rgb24", "-c:v", "ppm", "-f", "image2pipe", "pipe:1"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE)
    while p.stdout.readline():  # "P6"
        w, h = map(int, p.stdout.readline().split())
        p.stdout.readline()  # "255"
        yield np.frombuffer(p.stdout.read(w * h * 3), np.uint8).reshape(h, w, 3)
    assert p.wait() == 0, "ffmpeg failed"


def crop(frame, box):
    h, w = frame.shape[:2]
    x1, y1, x2, y2 = box[0] * w, box[1] * h, box[2] * w, box[3] * h
    side = max(1, round(GROW * max(x2 - x1, y2 - y1)))
    left, top = round((x1 + x2 - side) / 2), round((y1 + y2 - side) / 2)
    out = np.zeros((side, side, 3), np.uint8)
    a, b, c, d = max(left, 0), max(top, 0), min(left + side, w), min(top + side, h)
    if c > a and d > b:
        out[b - top:d - top, a - left:c - left] = frame[b:d, a:c]
    return Image.fromarray(out).resize((SIZE, SIZE), Image.LANCZOS)


def candidates(people, ffmpeg, video, total, out):
    os.makedirs(out, exist_ok=True)
    want = {}
    for key, (_, _, _, picks) in people.items():
        for k, (frame, box) in enumerate(picks):
            want.setdefault(frame, []).append((key, k, box))
    n = -1
    for n, frame in enumerate(decode(ffmpeg, video)):
        for key, k, box in want.get(n, ()):
            crop(frame, box).save(os.path.join(out, f"{key}_{k}.jpg"), quality=92)
    assert total is None or n + 1 == total, f"decoded {n + 1} frames, expected {total}"
    order = sorted(people, key=lambda k: -people[k][2])
    with open(os.path.join(out, "candidates.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("key\tname\ttracks\tseconds\n")
        for key in order:
            name, tracks, secs, _ = people[key]
            f.write(f"{key}\t{name}\t{tracks}\t{secs:.1f}\n")
    font = ImageFont.load_default(size=16)
    per, strip = 8, 22
    for s in range(0, len(order), per):
        page = Image.new("RGB", (PER * SIZE, per * (SIZE + strip)), (16, 16, 16))
        draw = ImageDraw.Draw(page)
        for r, key in enumerate(order[s:s + per]):
            name, tracks, secs, picks = people[key]
            for k in range(len(picks)):
                page.paste(Image.open(os.path.join(out, f"{key}_{k}.jpg")), (k * SIZE, r * (SIZE + strip)))
            draw.text((4, r * (SIZE + strip) + SIZE + 2), f"{key}  {name}  {tracks} tracks  {secs:.0f} s",
                      fill=(255, 220, 90), font=font)
        page.save(os.path.join(out, f"candidates_{s // per}.jpg"), quality=88)
    print(f"{len(order)} people in {out}")


def sheet(folder, table, out):
    rows = [line.rstrip("\n").split("\t") for line in list(open(table, encoding="utf-8"))[1:] if line.strip()]
    cols, lines, strip, each = 3, 5, 26, 3
    font = ImageFont.load_default(size=20)
    per = cols * lines
    for p in range(0, len(rows), per):
        page = Image.new("RGB", (cols * each * SIZE + (cols - 1) * 16, lines * (SIZE + strip)), (16, 16, 16))
        draw = ImageDraw.Draw(page)
        for i, row in enumerate(rows[p:p + per]):
            label, keys = row[0], [k for k in row[1].split(",") if k]
            files = []
            for k in range(PER):  # the keys in turn: the first picture of each, then the second, …
                files += [os.path.join(folder, f"{key}_{k}.jpg") for key in keys]
            files = [f for f in files if os.path.isfile(f)][:each]
            assert files, f"{label}: no picture of {keys} in {folder}"
            x, y = (i % cols) * (each * SIZE + 16), (i // cols) * (SIZE + strip)
            for k, file in enumerate(files):
                page.paste(Image.open(file), (x + k * SIZE, y))
            draw.text((x + 4, y + SIZE + 1), label, fill=(255, 220, 90), font=font)
        page.save(f"{out}_{p // per}.jpg", quality=90)
    print(f"{len(rows)} labels on {(len(rows) + per - 1) // per} pages, {out}_<n>.jpg")


def main():
    mode = sys.argv[1]
    if mode == "project":
        ffmpeg, video, project, nth, out = sys.argv[2:7]
        people, total = from_project(project, int(nth))
        candidates(people, ffmpeg, video, total, out)
    elif mode == "labels":
        ffmpeg, video, labels, frames, out = sys.argv[2:7]
        candidates(from_labels(labels), ffmpeg, video, int(frames), out)
    else:
        assert mode == "sheet", "project, labels or sheet"
        sheet(*sys.argv[2:5])


main()
