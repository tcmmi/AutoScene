"""Laughter, crying and shouting on a video's soundtrack, with YAMNet.

    uv run sound_events.py <ffmpeg> <video> <yamnet.onnx> <yamnet_class_map.csv> <out.tsv>

The second vote of the expressions slot (`bench/models/DECISIONS.md`, row 5):
a face that laughs, cries or shouts can be heard. The first audio stream is
decoded to 16 kHz mono and goes through YAMNet in pieces of five minutes; the
model answers every 0.48 s for the 0.96 s that begin there. Written per
answer: `sec` and one column per group below (the highest score among the
group's AudioSet classes), plus `speech` and `music` for context.
"""
import csv
import subprocess
import sys

import numpy as np
import onnxruntime as ort

RATE, HOP = 16000, 0.48
GROUPS = {
    "laughing": ["Laughter", "Giggle", "Snicker", "Belly laugh", "Chuckle, chortle", "Baby laughter"],
    "crying": ["Crying, sobbing", "Whimper", "Wail, moan", "Baby cry, infant cry"],
    "shouting": ["Shout", "Yell", "Bellow", "Screaming", "Children shouting", "Whoop"],
    "speech": ["Speech"],
    "music": ["Music"],
}


def main():
    ffmpeg, video, model, class_map, out = sys.argv[1:6]
    names = [row[2] for row in list(csv.reader(open(class_map, encoding="utf-8")))[1:]]
    index = {group: [names.index(n) for n in classes] for group, classes in GROUPS.items()}
    cmd = [ffmpeg, "-v", "error", "-i", video, "-map", "0:a:0", "-ac", "1", "-ar", str(RATE), "-f", "f32le", "pipe:1"]
    sound = np.frombuffer(subprocess.run(cmd, stdout=subprocess.PIPE, check=True).stdout, np.float32)
    net = ort.InferenceSession(model, providers=["CPUExecutionProvider"])
    # Pieces that begin on the model's own grid, so that their answers follow one another.
    piece = int(round(300 / HOP)) * int(RATE * HOP)
    scores = np.concatenate([net.run(None, {"waveform": sound[at:at + piece]})[0] for at in range(0, len(sound), piece)])
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("sec\t" + "\t".join(GROUPS) + "\n")
        for k, row in enumerate(scores):
            f.write(f"{k * HOP:.2f}\t" + "\t".join(f"{row[index[g]].max():.3f}" for g in GROUPS) + "\n")
    print(f"{len(sound) / RATE:.0f} s of sound, {len(scores)} answers in {out}")
    for group in list(GROUPS)[:3]:
        best = scores[:, index[group]].max(axis=1)
        print(f"  {group}: {(best >= 0.5).sum()} answers at 0.5 or more, {(best >= 0.2).sum()} at 0.2 or more")


main()
