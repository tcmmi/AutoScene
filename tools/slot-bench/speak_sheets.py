"""Sheets of an episode's shots, for labelling who is seen and who speaks in each.

    uv run speak_sheets.py <ffmpeg> <video> <project_copy_dir> <media> <sound.tsv> <out_dir> [<scenes.tsv> odd]

`<media>` is the media's place in the project (0 for the first); `<sound.tsv>`
is `sound_events.py`'s file for the same video. With `<scenes.tsv> odd` only
the shots of every second labelled scene are drawn (the first, the third, …).

Every shot gets a row of six pictures spread evenly over it; a shot of 6 s or
more gets two rows, one of 12 s or more three. Under a shot's first picture:
its number, when it starts (m:ss), how long it is and how many seconds of
speech are heard in it (the 0.48 s steps whose speech score is 0.5 or more).
Five rows per sheet (`sheet_000.jpg`, …); a shot is never cut by a sheet's
end. `rows.tsv` lists `sheet shot start end speech_s` (frames).

`LINES=<dir>` (the folder `speak_lines.py` wrote) puts the subtitle lines
shown during a shot under its pictures, in their order: a line that is on
screen for a quarter of a second of the shot, or for half of the line or of
the shot. `<<` before a line says that it began before the shot, `>>` after
it that it goes on after the shot. The sheets are then as high as five rows
at most and hold as many shots as fit.

The video is decoded once, every frame from frame 0, and the run stops when
the number of frames is not the project's.
"""
import os
import sqlite3
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

W, H, STRIP = 320, 180, 24
COLS, ROWS = 6, 5
HOP, HEARD = 0.48, 0.5


def shots(project, nth):
    db = sqlite3.connect(f"file:{project}/project.db?mode=ro", uri=True)
    media = db.execute(
        "SELECT id, frames, CAST(fps_num AS REAL) / fps_den FROM media WHERE shots = 'ready' ORDER BY position, id"
    ).fetchall()
    mid, frames, fps = media[nth]
    rows = db.execute(
        "SELECT start, end FROM segments WHERE media_id = ? AND kind = 'shot' ORDER BY start", (mid,)
    ).fetchall()
    return rows, frames, fps


def odd_scenes(path, rows):
    """Whether each shot lies in an odd-numbered labelled scene (the first scene is 1)."""
    starts = []
    for line in list(open(path, encoding="utf-8"))[1:]:
        if line.strip():
            frame = int(line.split("\t")[0])
            shot = min(range(len(rows)), key=lambda i: abs(rows[i][0] - frame))
            assert abs(rows[shot][0] - frame) <= 12, f"scene at frame {frame}: no shot starts there"
            starts.append(shot)
    assert starts == sorted(set(starts)) and starts[0] == 0, "scene starts: in order, the first at the first shot"
    scene, out = 0, []
    for i in range(len(rows)):
        if scene < len(starts) and i == starts[scene]:
            scene += 1
        out.append(scene % 2 == 1)
    return out


def speech(path, rows, fps):
    """Seconds of heard speech per shot."""
    heard = [float(line.split("\t")[4]) >= HEARD for line in list(open(path, encoding="utf-8"))[1:] if line.strip()]
    head = open(path, encoding="utf-8").readline().rstrip("\n").split("\t")
    assert head[4] == "speech", "sound_events.py's columns"
    out = []
    for start, end in rows:
        a, b = start / fps, end / fps
        total = 0.0
        for k in range(int(a / HOP), min(len(heard), int(b / HOP) + 1)):
            if heard[k]:
                total += max(0.0, min(b, (k + 1) * HOP) - max(a, k * HOP))
        out.append(total)
    return out


MARK, GAP = 30, 8


def spoken(folder, rows, fps):
    """Per shot: where its subtitle lines go under its pictures, `[(file, x, y, before, after)]`, and
    how high that part is (0 without lines)."""
    out = [([], 0) for _ in rows]
    if not folder:
        return out
    lines = []
    for row in list(open(os.path.join(folder, "lines.tsv"), encoding="utf-8"))[1:]:
        k, start, end, _, _, width, height = row.rstrip("\n").split("\t")
        if int(width) > 0:
            lines.append((int(start), int(end), os.path.join(folder, f"line_{int(k):04d}.jpg"), int(width), int(height)))
    for i, (start, end) in enumerate(rows):
        placed, x, y, tall = [], MARK, GAP, 0
        for a, b, file, width, height in lines:
            both = min(end, b) - max(start, a)
            if both <= 0 or (both < 0.25 * fps and 2 * both < b - a and 2 * both < end - start):
                continue
            if x + width + MARK > COLS * W and x > MARK:
                x, y, tall = MARK, y + tall + GAP, 0
            placed.append((file, x, y, a < start - 2, b > end + 2))
            x, tall = x + width + 2 * MARK, max(tall, height)
        out[i] = (placed, y + tall + GAP if placed else 0)
    return out


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
    ffmpeg, video, project, nth, sound, out = sys.argv[1:7]
    rows, total, fps = shots(project, int(nth))
    keep = [True] * len(rows)
    if len(sys.argv) > 7:
        assert sys.argv[8] == "odd", "only `<scenes.tsv> odd` is known"
        keep = odd_scenes(sys.argv[7], rows)
    heard = speech(sound, rows, fps)
    blocks = []  # (shot, [row of frames, ...])
    for i, (start, end) in enumerate(rows):
        if not keep[i]:
            continue
        n, secs = end - start, (end - start) / fps
        lines = 1 if secs < 6 else 2 if secs < 12 else 3
        count = min(n, lines * COLS)
        frames = sorted({start + int((k + 0.5) * n / count) for k in range(count)})
        blocks.append((i, [frames[r:r + COLS] for r in range(0, len(frames), COLS)]))
    pictures = decode(ffmpeg, video, {f for _, lines in blocks for line in lines for f in line}, total)
    os.makedirs(out, exist_ok=True)
    font = ImageFont.load_default(size=17)
    said = spoken(os.environ.get("LINES"), rows, fps)
    high = ROWS * (H + STRIP)
    sheets, at = [[]], 0
    for shot, lines in blocks:
        need = len(lines) * (H + STRIP) + said[shot][1]
        if at + need > high and sheets[-1]:
            sheets.append([])
            at = 0
        sheets[-1].append((shot, lines))
        at += need
    with open(os.path.join(out, "rows.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("sheet\tshot\tstart\tend\tspeech_s\n")
        for s, sheet in enumerate(sheets):
            used = sum(len(lines) * (H + STRIP) + said[shot][1] for shot, lines in sheet)
            page = Image.new("RGB", (COLS * W, max(high, used)), (16, 16, 16))
            draw = ImageDraw.Draw(page)
            y = 0
            for shot, lines in sheet:
                start, end = rows[shot]
                when = int(start / fps)
                text = f"shot {shot}    {when // 60}:{when % 60:02d}    {(end - start) / fps:.1f} s long    speech {heard[shot]:.1f} s"
                for r, line in enumerate(lines):
                    for k, frame in enumerate(line):
                        page.paste(Image.fromarray(pictures[frame]), (k * W, y))
                    draw.text((4, y + H + 2), text if r == 0 else f"shot {shot} goes on", fill=(255, 220, 90), font=font)
                    y += H + STRIP
                for file, x, dy, before, after in said[shot][0]:
                    piece = Image.open(file)
                    if before:
                        draw.text((x - MARK + 2, y + dy + 4), "<<", fill=(120, 200, 255), font=font)
                    page.paste(piece, (x, y + dy))
                    if after:
                        draw.text((x + piece.width + 4, y + dy + 4), ">>", fill=(120, 200, 255), font=font)
                y += said[shot][1]
                f.write(f"{s}\t{shot}\t{start}\t{end}\t{heard[shot]:.2f}\n")
            page.save(os.path.join(out, f"sheet_{s:03d}.jpg"), quality=88)
    print(f"{len(blocks)} shots on {len(sheets)} sheets in {out}")


main()
