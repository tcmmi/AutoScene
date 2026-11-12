"""Contact sheets of an episode's shots in time order, for labelling scenes.

    uv run shot_sheets.py <ffmpeg> <video> <project_copy_dir> <media> <out_dir>

`<media>` is the media's place in the project (0 for the first); the folder
may also hold bench inputs (`bb.segments.tsv`, `bb.info.json`). One picture
per shot from its middle; a shot of 8 s or more gets two (at 1/4 and 3/4,
the second marked `b`). Under each picture: the shot's number, when it starts
(m:ss) and how long it is; `FADE` or `DISS` when black, a fade or a dissolve
lies before it. Thirty pictures per sheet (`sheet_000.jpg`, …), and
`shots.tsv` (`shot start end`, frames) to turn shot numbers back into frames.

The video is decoded once, every frame from frame 0, and the run stops when
the number of frames is not the project's.
"""
import json
import os
import sqlite3
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

W, H, STRIP = 320, 180, 24
COLS, ROWS = 6, 5


def segments(project, nth):
    """`(start, end, kind)` rows, the frame count and the frame rate: from a copy of a project, or
    from a folder of bench inputs (an episode that is in no project)."""
    if not os.path.isfile(f"{project}/project.db"):
        rows = [line.split("\t") for line in open(f"{project}/bb.segments.tsv", encoding="utf-8") if line.strip()]
        rate = json.load(open(f"{project}/bb.info.json", encoding="utf-8"))["video"]["avgFrameRate"]
        return [(int(r[0]), int(r[1]), r[2].strip()) for r in rows], int(rows[-1][1]), rate["num"] / rate["den"]
    db = sqlite3.connect(f"file:{project}/project.db?mode=ro", uri=True)
    media = db.execute(
        "SELECT id, frames, CAST(fps_num AS REAL) / fps_den FROM media WHERE shots = 'ready' ORDER BY position, id"
    ).fetchall()
    mid, frames, fps = media[nth]
    rows = db.execute("SELECT start, end, kind FROM segments WHERE media_id = ? ORDER BY start", (mid,)).fetchall()
    return rows, frames, fps


def shots(project, nth):
    segs, frames, fps = segments(project, nth)
    rows, before = [], ""
    for start, end, kind in segs:
        if kind == "shot":
            rows.append((start, end, before))
            before = ""
        elif kind == "dissolve":
            before = before or "DISS"
        elif kind in ("black", "fade_in", "fade_out"):
            before = "FADE"
    return rows, frames, fps


def decode(ffmpeg, video, want, total):
    out = {}
    cmd = [ffmpeg, "-v", "error", "-i", video, "-map", "0:v:0", "-vf", f"scale={W}:{H}:flags=bicubic",
           "-fps_mode", "passthrough", "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE)
    size, n = W * H * 3, 0
    while True:
        buf = p.stdout.read(size)
        if len(buf) < size:
            break
        if n in want:
            out[n] = np.frombuffer(buf, np.uint8).reshape(H, W, 3).copy()
        n += 1
    p.wait()
    assert n == total, f"decoded {n} frames, the project has {total}: not the file the project was made from"
    return out


def main():
    ffmpeg, video, project, nth, out = sys.argv[1:6]
    rows, total, fps = shots(project, int(nth))
    cells = []  # (frame, caption)
    for i, (start, end, before) in enumerate(rows):
        n, secs = end - start, (end - start) / fps
        at = f"{int(start / fps) // 60}:{int(start / fps) % 60:02d}"
        mark = f" {before}" if before else ""
        if secs >= 8:
            cells.append((start + n // 4, f"{i}  {at}  {secs:.0f}s{mark}"))
            cells.append((start + 3 * n // 4, f"{i}b"))
        else:
            cells.append((start + n // 2, f"{i}  {at}  {secs:.0f}s{mark}"))
    pictures = decode(ffmpeg, video, {f for f, _ in cells}, total)
    os.makedirs(out, exist_ok=True)
    font = ImageFont.load_default(size=17)
    per = COLS * ROWS
    for s in range(0, len(cells), per):
        sheet = Image.new("RGB", (COLS * W, ROWS * (H + STRIP)), (16, 16, 16))
        draw = ImageDraw.Draw(sheet)
        for k, (frame, text) in enumerate(cells[s:s + per]):
            x, y = (k % COLS) * W, (k // COLS) * (H + STRIP)
            sheet.paste(Image.fromarray(pictures[frame]), (x, y))
            colour = (255, 200, 60) if text.endswith(("FADE", "DISS")) else (235, 235, 235)
            draw.text((x + 4, y + H + 2), text, fill=colour, font=font)
        sheet.save(os.path.join(out, f"sheet_{s // per:03d}.jpg"), quality=85)
    with open(os.path.join(out, "shots.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("shot\tstart\tend\n")
        for i, (start, end, _) in enumerate(rows):
            f.write(f"{i}\t{start}\t{end}\n")
    print(f"{len(rows)} shots, {len(cells)} pictures, {(len(cells) + per - 1) // per} sheets in {out}")


main()
