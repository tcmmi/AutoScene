"""Face crops of an episode's face tracks, for testing expression models.

    uv run face_crops.py <ffmpeg> <video> <project_copy_dir> <media> <out_dir>

`<media>` is the media's place in the project (0 for the first, in the order
of `position, id`); `<video>` is the file its tracks were found on (the
proxy). For every track that is a face, up to three of its detections: the
track is cut into three equal runs of detections and the one of highest
quality in each run is taken. Each crop is a square 1.6 times the face box's
longer side around the box's centre, black where it leaves the frame, saved
as a 256x256 JPEG (`t<track>_f<frame>.jpg`). `crops.tsv` lists them: file,
track, frame, character (its name, else `Character <number>`, else empty)
and the face box inside the crop as shares of the crop (x1 y1 x2 y2).

The video is decoded once, every frame from frame 0 at its own size, and
nothing is written when the number of frames is not the project's.
"""
import io
import os
import sqlite3
import subprocess
import sys

import numpy as np
from PIL import Image

GROW, SIZE, PER_TRACK = 1.6, 256, 3
DET = np.dtype([("frame", "<u4"), ("x1", "<f4"), ("y1", "<f4"), ("x2", "<f4"), ("y2", "<f4"),
                ("score", "<f4"), ("norm", "<f4"), ("quality", "<f4")])


def picks(project, nth):
    """(frame, track, character, box) for the chosen detections, and the media's frame count."""
    db = sqlite3.connect(f"file:{project}/project.db?mode=ro", uri=True)
    mid, frames = db.execute("SELECT id, frames FROM media ORDER BY position, id").fetchall()[nth]
    rows = db.execute(
        "SELECT t.id, t.dets, c.name, c.number FROM tracks t LEFT JOIN characters c ON c.id = t.character_id"
        " WHERE t.media_id = ? AND t.not_face = 0 ORDER BY t.id", (mid,))
    out = []
    for track, dets, name, number in rows:
        dets = np.sort(np.frombuffer(dets, DET), order="frame")
        who = name or (f"Character {number}" if number is not None else "")
        for run in np.array_split(dets, min(PER_TRACK, len(dets))):
            d = run[np.argmax(run["quality"])]
            out.append((int(d["frame"]), track, who, (d["x1"], d["y1"], d["x2"], d["y2"])))
    return out, frames


def decode(ffmpeg, video):
    """Every frame of the first picture stream, in order, as RGB at its own size."""
    cmd = [ffmpeg, "-v", "error", "-i", video, "-map", "0:v:0", "-fps_mode", "passthrough",
           "-pix_fmt", "rgb24", "-c:v", "ppm", "-f", "image2pipe", "pipe:1"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE)
    while p.stdout.readline():  # "P6"
        w, h = map(int, p.stdout.readline().split())
        p.stdout.readline()  # "255"
        yield np.frombuffer(p.stdout.read(w * h * 3), np.uint8).reshape(h, w, 3)
    assert p.wait() == 0, "ffmpeg failed"


def crop(frame, box):
    """The square crop around a face box (shares of the frame), and the box as shares of the crop."""
    h, w = frame.shape[:2]
    x1, y1, x2, y2 = box[0] * w, box[1] * h, box[2] * w, box[3] * h
    side = max(1, round(GROW * max(x2 - x1, y2 - y1)))
    left, top = round((x1 + x2 - side) / 2), round((y1 + y2 - side) / 2)
    out = np.zeros((side, side, 3), np.uint8)
    a, b, c, d = max(left, 0), max(top, 0), min(left + side, w), min(top + side, h)
    if c > a and d > b:
        out[b - top:d - top, a - left:c - left] = frame[b:d, a:c]
    pic = Image.fromarray(out).resize((SIZE, SIZE), Image.LANCZOS)
    return pic, ((x1 - left) / side, (y1 - top) / side, (x2 - left) / side, (y2 - top) / side)


def main():
    ffmpeg, video, project, nth, out = sys.argv[1:6]
    chosen, total = picks(project, int(nth))
    by_frame = {}
    for c in chosen:
        by_frame.setdefault(c[0], []).append(c)
    done, n = [], -1
    for n, frame in enumerate(decode(ffmpeg, video)):
        for at, track, who, box in by_frame.get(n, ()):
            pic, inner = crop(frame, box)
            buf = io.BytesIO()
            pic.save(buf, "JPEG", quality=95)
            done.append((f"t{track}_f{at}.jpg", track, at, who, inner, buf.getvalue()))
    assert n + 1 == total, f"decoded {n + 1} frames, the project has {total}: not the file the project was made from"
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, "crops.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("file\ttrack\tframe\tcharacter\tx1\ty1\tx2\ty2\n")
        for name, track, at, who, inner, jpeg in sorted(done, key=lambda d: (d[1], d[2])):
            with open(os.path.join(out, name), "wb") as g:
                g.write(jpeg)
            f.write(f"{name}\t{track}\t{at}\t{who.replace(chr(9), ' ')}\t" + "\t".join(f"{v:.4f}" for v in inner) + "\n")
    print(f"{len({d[1] for d in done})} tracks, {len(done)} crops in {out}")


main()
