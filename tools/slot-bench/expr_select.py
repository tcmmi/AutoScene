"""The check set of the expressions slot: which face tracks get labels.

    uv run expr_select.py <crops_dir> <scores_dir> <out_dir> [random_tracks] [seed]

Tracks are drawn, not crops, and every crop of a drawn track is labelled, so
that a label can be scored per crop and per track. Two groups:

- `top`: every track among the highest of an output that could stand for a
  label (`CANDIDATES` in expr_scores.py; a track's value is its highest
  crop's): the DEPTH highest tracks of each such output, all of them. This is
  what finds the faces of rare labels at all.
- `rest`: `random_tracks` (150) of all other tracks, drawn at random. Each
  stands for (other tracks / drawn tracks) tracks: its `weight`.

With the weights the labelled tracks stand for the whole episode, whatever
output is scored later, so the models that picked the `top` group gain
nothing from it.

Writes `<out_dir>/tracks.tsv` (`track group weight by`: `by` is the first
output that put a track into `top`) and `<out_dir>/list.tsv` (`file track
frame`, the crops of the drawn tracks in a shuffled order) for crop_sheets.py.
The labellers get the sheets only.
"""
import os
import random
import sys

import numpy as np

import expr_scores as es

# How many of an output's highest tracks are taken: more for the labels that are rare.
DEPTH = {"smile": 3, "laugh": 6, "cry": 6, "angry": 3, "shout": 6, "surprised": 6, "downcast": 3, "blush": 6,
         "eyes_closed": 3, "mouth_open": 3}


def main():
    crops_dir, scores_dir, out = sys.argv[1:4]
    n_rest = int(sys.argv[4]) if len(sys.argv) > 4 else 150
    rng = random.Random(int(sys.argv[5]) if len(sys.argv) > 5 else 1)
    rows = es.crops(crops_dir)
    values = es.outputs(scores_dir, [r["file"] for r in rows])
    tracks = sorted({int(r["track"]) for r in rows})
    place = {t: i for i, t in enumerate(tracks)}
    at = np.array([place[int(r["track"])] for r in rows])
    by = {}
    for label, names in es.CANDIDATES.items():
        for name in names:
            top = np.full(len(tracks), -np.inf)
            np.maximum.at(top, at, values[name])
            for i in np.argsort(-top, kind="stable")[:DEPTH[label]]:
                by.setdefault(tracks[i], name)
    others = [t for t in tracks if t not in by]
    drawn = sorted(rng.sample(others, min(n_rest, len(others))))
    weight = len(others) / len(drawn)
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, "tracks.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("track\tgroup\tweight\tby\n")
        for t in sorted(by):
            f.write(f"{t}\ttop\t1\t{by[t]}\n")
        for t in drawn:
            f.write(f"{t}\trest\t{weight:.6g}\t\n")
    chosen = set(by) | set(drawn)
    listed = [r for r in rows if int(r["track"]) in chosen]
    rng.shuffle(listed)
    with open(os.path.join(out, "list.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("file\ttrack\tframe\n")
        for r in listed:
            f.write(f"{os.path.abspath(os.path.join(crops_dir, r['file']))}\t{r['track']}\t{r['frame']}\n")
    print(f"{len(tracks)} tracks: {len(by)} in top, {len(drawn)} of the other {len(others)} drawn (weight {weight:.2f});"
          f" {len(listed)} crops to label")


main()
