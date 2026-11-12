"""The scenes bench over several titles: is there one threshold for all?

    uv run scene_titles.py <scenes_bench.exe> <titles.tsv> [NAME=value …]

`titles.tsv` has a header and one row per episode: `name copy media scenes
labels emb` (the project copy's folder, the media's place in it, the scene
labels, the identity labels or `-`, the embedding file or `-`). Further
`NAME=value` arguments go to `scenes_bench score` as its settings (`SHORT`,
`P`, `W`); with an embedding the cues are the embedding alone unless `W` says
otherwise.

Per title: a character's scenes (mean F, stored cast) at the title's own best
threshold, at the one threshold that is best over all titles, and at the
threshold that is best over the OTHER titles (what a new title would get).
"""
import os
import subprocess
import sys


def curve(exe, row, settings):
    env = dict(os.environ, CURVE="1", MEDIA=row["media"], **settings)
    if row["emb"] != "-":
        env["EMB"] = "1:" + row["emb"]
        env.setdefault("W", "0,0")
    out = subprocess.run([exe, "score", row["copy"], row["scenes"], row["labels"]], env=env, capture_output=True, text=True)
    assert out.returncode == 0, out.stderr[-2000:]
    points = {}
    for line in out.stdout.splitlines():
        f = line.split("\t")
        if f[0] == "curve" and f[1] != "threshold":
            points[f[1]] = (float(f[3]), float(f[4]), int(f[5]))
    ap = next(line.split()[-1] for line in out.stdout.splitlines() if line.startswith("boundary scores"))
    return points, float(ap)


def main():
    exe, titles = sys.argv[1:3]
    settings = dict(a.split("=", 1) for a in sys.argv[3:])
    lines = [line.rstrip("\n").split("\t") for line in open(titles, encoding="utf-8") if line.strip()]
    rows = [dict(zip(lines[0], line)) for line in lines[1:]]
    curves = {r["name"]: curve(exe, r, settings) for r in rows}
    names = list(curves)
    thresholds = list(curves[names[0]][0])
    mean = lambda among, t: sum(curves[n][0][t][0] for n in among) / len(among)
    shared = max(thresholds, key=lambda t: mean(names, t))
    print(f"settings {settings or 'default'}; one threshold for all: {shared}")
    print(f"{'title':14} {'AP':>6} {'own best':>16} {'shared':>8} {'others`':>16} {'scenes own/shared':>18}")
    total = [0.0, 0.0, 0.0]
    for n in names:
        points, ap = curves[n]
        own = max(thresholds, key=lambda t: points[t][0])
        others = [m for m in names if m != n]
        left = max(thresholds, key=lambda t: mean(others, t)) if others else own
        got = (points[own][0], points[shared][0], points[left][0])
        total = [a + b for a, b in zip(total, got)]
        print(f"{n:14} {ap:6.3f} {got[0]:8.3f} @{own} {got[1]:8.3f} {got[2]:8.3f} @{left} {points[own][2]:>8} / {points[shared][2]}")
    print(f"{'mean':14} {'':6} {total[0] / len(names):8.3f} {'':6} {total[1] / len(names):8.3f} {total[2] / len(names):8.3f}")


main()
