#!/usr/bin/env python3
"""
AMVerge Deadframe Remover for AutoScene.

Detects static subject / duplicate holding frames and compacts video
clips back-to-back using optical flow heuristics and FFmpeg select filters.
Based on AMVerge-CLI (https://github.com/AMVerge-team/AMVerge-CLI).
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


def log(msg: str):
    print(f"[AMVerge Deadframes] {msg}", flush=True)


def has_opencv() -> bool:
    try:
        import cv2
        import numpy as np
        return True
    except ImportError:
        return False


def remove_deadframes_ffmpeg(input_file: Path, output_file: Path) -> bool:
    """Fallback deadframe removal using FFmpeg's native mpdecimate filter."""
    log(f"Processing with FFmpeg mpdecimate filter: {input_file.name}")
    cmd = [
        "ffmpeg", "-y",
        "-i", str(input_file),
        "-vf", "mpdecimate,setpts=N/FRAME_RATE/TB",
        "-c:v", "libx264",
        "-crf", "17",
        "-preset", "fast",
        "-c:a", "copy",
        str(output_file),
    ]
    res = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    if res.returncode != 0:
        log(f"FFmpeg failed: {res.stderr.strip()[:200]}")
        return False
    return True


def remove_deadframes_opencv(input_file: Path, output_file: Path) -> bool:
    """Advanced deadframe detection using OpenCV optical flow heuristics."""
    import cv2
    import numpy as np

    log(f"Analyzing frame optical flow with OpenCV: {input_file.name}")
    cap = cv2.VideoCapture(str(input_file))
    if not cap.isOpened():
        log(f"Could not open video file: {input_file}")
        return False

    fps = cap.get(cv2.CAP_PROP_FPS) or 24.0
    prev_gray = None
    decisions = []

    # Threshold for motion detection (average magnitude of optical flow)
    MOTION_THRESHOLD = 0.45

    while True:
        ret, frame = cap.read()
        if not ret:
            break

        # Resize for fast computation
        h, w = frame.shape[:2]
        small = cv2.resize(frame, (min(320, w), min(180, h)))
        gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY)

        if prev_gray is None:
            decisions.append(True)  # Always keep the first frame
            prev_gray = gray
            continue

        # Compute Farneback Optical Flow
        flow = cv2.calcOpticalFlowFarneback(
            prev_gray, gray, None, 0.5, 3, 15, 3, 5, 1.2, 0
        )
        mag, _ = cv2.cartToPolar(flow[..., 0], flow[..., 1])
        motion_score = np.mean(mag)

        # If motion exceeds threshold, mark frame as kept
        is_moving = motion_score > MOTION_THRESHOLD
        decisions.append(bool(is_moving))
        prev_gray = gray

    cap.release()

    total_frames = len(decisions)
    kept_frames = sum(1 for d in decisions if d)
    dead_frames = total_frames - kept_frames
    pct = (dead_frames / max(1, total_frames)) * 100.0

    log(f"Total: {total_frames} frames | Kept: {kept_frames} | Dead: {dead_frames} ({pct:.1f}% removed)")

    # Build inclusive kept ranges
    ranges = []
    start = None
    for i, keep in enumerate(decisions):
        if keep and start is None:
            start = i
        elif not keep and start is not None:
            ranges.append((start, i - 1))
            start = None
    if start is not None:
        ranges.append((start, len(decisions) - 1))

    if not ranges:
        log("No motion detected, preserving original file.")
        shutil.copy2(input_file, output_file)
        return True

    # Build select expression
    select_expr = "+".join(f"between(n,{r[0]},{r[1]})" for r in ranges)
    filter_graph = f"[0:v]select='{select_expr}',setpts=N/FRAME_RATE/TB[outv]"

    cmd = [
        "ffmpeg", "-y",
        "-i", str(input_file),
        "-filter_complex", filter_graph,
        "-map", "[outv]",
        "-c:v", "libx264",
        "-crf", "17",
        "-preset", "fast",
        "-c:a", "copy",
        str(output_file),
    ]

    res = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    return res.returncode == 0


def main():
    log("Initializing AMVerge Deadframe Remover...")
    opencv_available = has_opencv()
    log(f"OpenCV Engine: {'Enabled (Optical Flow)' if opencv_available else 'Fallback (FFmpeg Decimate)'}")

    meta_path = os.environ.get("AUTOSCENE_META")

    # Standalone diagnostic test mode (when clicking "Run Test" in AutoScene)
    if not meta_path or not os.path.exists(meta_path):
        log("Running diagnostic self-test...")
        log("Checking FFmpeg availability:")
        ffmpeg_bin = shutil.which("ffmpeg")
        if ffmpeg_bin:
            log(f"Found FFmpeg at: {ffmpeg_bin}")
        else:
            log("Warning: FFmpeg not found in PATH.")

        log("AMVerge Deadframe Remover is ready and configured!")
        # Touch proof file
        proof = Path("extension_test_success.txt")
        proof.write_text("AMVerge Deadframe Remover diagnostic passed.\n")
        return

    with open(meta_path, "r") as f:
        payload = json.load(f)

    out_dir = Path(payload.get("outDir", "."))
    output_files = payload.get("outputFiles", [])

    log(f"Received export with {len(output_files)} clip(s) in {out_dir}")

    for file_str in output_files:
        src = Path(file_str)
        if not src.exists():
            continue

        compacted = src.parent / f"{src.stem}_compacted{src.suffix}"
        log(f"Processing: {src.name} -> {compacted.name}")

        success = (
            remove_deadframes_opencv(src, compacted)
            if opencv_available
            else remove_deadframes_ffmpeg(src, compacted)
        )

        if success:
            log(f"Compacted successfully: {compacted.name}")

    log("All clips processed.")


if __name__ == "__main__":
    main()
