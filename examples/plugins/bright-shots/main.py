# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Bright Shots: an example AutoScene plugin for the scenepack builder.

Its shot filter measures how bright the middle of each shot is with
AutoScene's FFmpeg, and leaves out the shots darker than the setting
"Darkest to keep". Its pack action saves a pack as a cut sheet, a CSV file
with a row per clip, before anything is rendered. autoscene.py, beside it,
speaks the protocol; docs/PLUGINS.md describes it.
"""

import csv
import re
import subprocess
import sys
from fractions import Fraction
from pathlib import Path

import autoscene

# The middle frame, small and in 8 bits whatever the episode's format, then
# its average brightness on stdout: lavfi.signalstats.YAVG=<16 to 235>.
MEASURE = "scale=320:-2,format=yuv420p,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-"
BLACK, WHITE = 16, 235


def main() -> int:
    p = autoscene.start()
    action = p.first["action"]
    if action["kind"] == "shotFilter" and action["id"] == "bright-enough":
        judge(p)
    elif action["kind"] == "packAction" and action["id"] == "cut-sheet":
        save_cut_sheet(p)
    else:
        p.error(f"Bright Shots has no action {action['id']}.")
        return 1
    return 0


def judge(p: autoscene.Plugin) -> None:
    """Says for each of the run's shots whether it is bright enough to keep."""
    episode = p.first["shots"]["episodes"][0]  # A run gets one episode.
    shots = p.first["shots"]["shots"]
    darkest = p.settings.get("darkest", 15)
    rate = frame_rate(episode["fps"])
    for i, shot in enumerate(shots):
        if p.cancelled.is_set():
            sys.exit(1)
        p.progress(i / len(shots), f"Measuring shot {i + 1} of {len(shots)}")
        light = brightness(p.first["tools"]["ffmpeg"], episode["path"], shot, rate)
        if light is None:
            # A shot left without a word would fail the check: this one stays.
            p.log(f"Couldn't measure the shot at frame {shot['start']}, so it stays.", "warn")
        p.verdict(shot["key"], light is None or light >= darkest)
    p.result(f"Measured {len(shots)} shots.")


def brightness(ffmpeg: str, path: str, shot: dict, rate: Fraction | None) -> float | None:
    """How bright the middle of `shot` is, from 0 to 100, if FFmpeg can tell."""
    if rate is None:
        return None
    # A time only in a file of a constant rate: in one that varies or has a
    # hole, FFmpeg finds another frame. See "Frames and time" in the guide.
    middle = (shot["start"] + shot["end"]) // 2 / rate
    command = [ffmpeg, "-hide_banner", "-nostdin", "-ss", f"{float(middle):.3f}", "-i", path]
    command += ["-an", "-sn", "-frames:v", "1", "-vf", MEASURE, "-f", "null", "-"]
    measured = subprocess.run(command, capture_output=True, encoding="utf-8", errors="replace")
    found = re.search(r"YAVG=([\d.]+)", measured.stdout)
    if found is None:
        return None
    return min(max((float(found[1]) - BLACK) / (WHITE - BLACK), 0), 1) * 100


def save_cut_sheet(p: autoscene.Plugin) -> None:
    """Writes the pack's clips to `output`, which takes the name the person chose once the run ends well."""
    pack = p.first["pack"]
    episodes = {e["mediaId"]: e for e in pack["episodes"]}
    try:
        # With a byte order mark, so a spreadsheet reads the names as UTF-8.
        with open(p.first["output"], "w", newline="", encoding="utf-8-sig") as sheet:
            rows = csv.writer(sheet)
            rows.writerow(["Clip", "Episode", "In", "Out", "Frames"])
            for clip in pack["clips"]:
                episode = episodes[clip["mediaId"]]
                rate = frame_rate(episode["fps"])
                start, end = clip["start"], clip["end"]
                name, stem = cell(clip["name"]), cell(Path(episode["path"]).stem)
                rows.writerow([name, stem, time(start, rate), time(end, rate), end - start])
    except OSError as e:
        p.error("Couldn't write the cut sheet.", str(e))
        sys.exit(1)
    count = len(pack["clips"])
    p.result("Saved 1 clip." if count == 1 else f"Saved {count} clips.")


def cell(name: str) -> str:
    """A name as a spreadsheet's text: one that starts with = + - @ would run as a formula."""
    return f"'{name}" if name.startswith(("=", "+", "-", "@", "\t", "\r")) else name


def frame_rate(fps: str | None) -> Fraction | None:
    """An episode's frame rate, `24000/1001`, if it is known."""
    return Fraction(fps) if fps else None


def time(frame: int, rate: Fraction | None) -> str:
    """A frame as `0:01:02.042`, right in a file of a constant rate; empty without a frame rate."""
    if rate is None:
        return ""
    millis = round(frame / rate * 1000)
    return f"{millis // 3600000}:{millis // 60000 % 60:02}:{millis // 1000 % 60:02}.{millis % 1000:03}"


if __name__ == "__main__":
    sys.exit(main())
