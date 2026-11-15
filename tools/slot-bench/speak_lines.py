"""The subtitle lines of a video as pictures, for the sheets of `speak_sheets.py`.

    uv run speak_lines.py <ffmpeg> <ffprobe> <video> <subtitle> <frames> <out_dir>

`<subtitle>` is the stream's place among the video's subtitle streams (0 for
the first); it may be a picture stream (PGS) or text. `<frames>` is the
number of frames the project has for the video: nothing is decoded when the
file has another number.

A line lasts from the packet that shows it to the next packet of the stream
(the one that clears it, or the next line). For each line one frame from its
middle is taken, once with the subtitles drawn on and once without; where
the two differ is the line, wherever in the frame it stands, and that box is
cut out of the picture with the subtitles (at a width of 960). Written:
`line_NNNN.jpg` and `lines.tsv` (`line start end start_s end_s width height`,
frames; width 0 when no drawn line was found there).

The video is decoded once, every frame from frame 0.
"""
import os
import subprocess
import sys

import numpy as np
from PIL import Image

WIDTH, MARGIN, SEEN = 960, 6, 48


def probe(ffprobe, video):
    out = subprocess.run(
        [ffprobe, "-v", "error", "-select_streams", "v:0", "-count_packets", "-show_entries",
         "stream=width,height,r_frame_rate,nb_read_packets", "-of", "default=nw=1", video],
        stdout=subprocess.PIPE, check=True, text=True).stdout
    got = dict(row.split("=", 1) for row in out.splitlines() if "=" in row)
    num, den = got["r_frame_rate"].split("/")
    return int(got["width"]), int(got["height"]), int(num) / int(den), int(got["nb_read_packets"])


def lines(ffprobe, video, nth):
    """`(start_s, end_s)` of every line: a big packet shows one, the next packet ends it."""
    out = subprocess.run(
        [ffprobe, "-v", "error", "-select_streams", f"s:{nth}", "-show_entries", "packet=pts_time,size",
         "-of", "csv=p=0", video], stdout=subprocess.PIPE, check=True, text=True).stdout
    packets = [(float(a), int(b)) for a, b in (row.split(",")[:2] for row in out.split("\n") if row.strip())]
    found, open_at = [], None
    for at, size in packets:
        if open_at is not None:
            found.append((open_at, at))
        open_at = at if size > 100 else None
    return found


def main():
    ffmpeg, ffprobe, video, nth, frames, out = sys.argv[1:7]
    w, h, fps, packets = probe(ffprobe, video)
    assert packets == int(frames), f"the file has {packets} frames, the project {frames}: not the file the project was made from"
    spans = lines(ffprobe, video, int(nth))
    small = h * WIDTH // w // 2 * 2
    graph = f"[0:v:0]split[a][b];[a][0:s:{nth}]overlay[with];[with][b]vstack,scale={WIDTH}:{2 * small}"
    # Two frames to try for each line: at a third and at two thirds of it.
    want = {}
    for k, (a, b) in enumerate(spans):
        for share in (1 / 3, 2 / 3):
            want.setdefault(int((a + (b - a) * share) * fps), []).append(k)
    cmd = [ffmpeg, "-v", "error", "-i", video, "-filter_complex", graph, "-fps_mode", "passthrough",
           "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE)
    size, n, cut = WIDTH * 2 * small * 3, 0, {}
    os.makedirs(out, exist_ok=True)
    while True:
        buf = p.stdout.read(size)
        if len(buf) < size:
            break
        for k in want.get(n, ()):
            if k in cut:
                continue
            both = np.frombuffer(buf, np.uint8).reshape(2 * small, WIDTH, 3)
            drawn, plain = both[:small], both[small:]
            apart = np.abs(drawn.astype(np.int16) - plain.astype(np.int16)).max(axis=2) > SEEN
            ys, xs = np.nonzero(apart)
            if len(ys) < 40:
                continue
            x0, x1 = max(0, xs.min() - MARGIN), min(WIDTH, xs.max() + 1 + MARGIN)
            y0, y1 = max(0, ys.min() - MARGIN), min(small, ys.max() + 1 + MARGIN)
            Image.fromarray(drawn[y0:y1, x0:x1]).save(os.path.join(out, f"line_{k:04d}.jpg"), quality=90)
            cut[k] = (x1 - x0, y1 - y0)
        n += 1
    p.wait()
    # The filter that draws the subtitles may hand out the last frame once more.
    assert int(frames) <= n <= int(frames) + 1, f"decoded {n} frames, the file has {frames}"
    with open(os.path.join(out, "lines.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("line\tstart\tend\tstart_s\tend_s\twidth\theight\n")
        for k, (a, b) in enumerate(spans):
            width, height = cut.get(k, (0, 0))
            f.write(f"{k}\t{int(a * fps)}\t{int(b * fps)}\t{a:.3f}\t{b:.3f}\t{width}\t{height}\n")
    print(f"{len(spans)} lines, {len(cut)} of them cut out as pictures, in {out}")


main()
