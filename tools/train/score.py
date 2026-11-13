"""Score an embedder on the anime tuning projects (`bench/anime`): the identity bench groups the tracks of the
Standard pass, with the embedder's vectors in place of the pass's own, and scores the main cast per track.

    uv run score.py <name> [<dir with <project>.emb>] [--against <name>] [NAME=value …]

Without a directory the pass's own embeddings are scored (the Standard scan). `<name>` names the result files
in `$TRAIN_WORK/score` (default `E:/as-ownmodels/train/score`): `<name>.<project>.txt` and `.items.tsv`.
`--against <name>` compares with an earlier result, paired per figure (bootstrap). `NAME=value` pairs go to
the scorer (`ADAPT=0`, `LINK=0.3`, …). The set-up is the one the tuning numbers of `bench/anime/tuning.md`
were made with: figure level, main cast from 15 s, 2,000 bootstrap draws.

Where things are, from the environment: `ANIME_RUN` (the joined tracks `<project>.<ARM>.tracks` and
`<project>.segments.tsv`; default `F:/as-ownmodels/anime/run`), `ARM` (`std3`), `IDENTITY_BENCH` (the
example of `as-analysis`, built in release; default `E:/as-ownmodels/train/bin/identity_bench.exe`).
"""
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BENCH = REPO / "bench" / "anime"
RUN = Path(os.environ.get("ANIME_RUN", "F:/as-ownmodels/anime/run"))
WORK = Path(os.environ.get("TRAIN_WORK", "E:/as-ownmodels/train"))
EXE = os.environ.get("IDENTITY_BENCH", str(WORK / "bin" / "identity_bench.exe"))
ARM = os.environ.get("ARM", "std3")
# The tuning projects: their episodes in project order, and the first frame of each episode after the first
# on the project's timeline (the frames of the episodes before it, as `bench_inputs` counted them).
PROJECTS = {
    "jjk": (["jjk_s2e03", "jjk_s2e04"], [34407]),
    "rezero": (["rz_31", "rz_33"], [37641]),
    "onepiece": (["op_6", "op_7", "op_a"], [34304, 70073]),
}
LINE = re.compile(r"^main cast \(.*?\)\s+tracks P ([\d.]+) R ([\d.]+) F ([\d.]+)")


def score(name, project, emb, settings):
    out = WORK / "score"
    out.mkdir(parents=True, exist_ok=True)
    # Only what the scorer is meant to see: a setting left in the shell must not change a score.
    env = {k: os.environ[k] for k in ("PATH", "SYSTEMROOT", "TEMP", "TMP") if k in os.environ}
    env.update(
        ANIME="1", GRID="6,10", MAIN_SECS="15", BOOT="2000", FPS="24000/1001", LEVEL="figure",
        GALLERY=str(BENCH / f"{project}.gallery.tsv"), SPANS=str(BENCH / f"{project}.spans.tsv"),
        SEGMENTS=str(RUN / f"{project}.segments.tsv"), EPISODES=",".join(map(str, PROJECTS[project][1])),
        ITEMS=str(out / f"{name}.{project}.items.tsv"),
    )
    if emb is not None:
        env["EMB_OVERRIDE"] = str(emb)
    env.update(settings)
    cmd = [EXE, "score", str(RUN / f"{project}.{ARM}.tracks"), str(BENCH / f"{project}.labels.tsv")]
    done = subprocess.run(cmd, env=env, capture_output=True, text=True, encoding="utf-8", errors="replace")
    (out / f"{name}.{project}.txt").write_text(done.stdout, encoding="utf-8", newline="\n")
    assert done.returncode == 0, done.stderr[-2000:]
    found = next((m for m in map(LINE.match, done.stdout.splitlines()) if m), None)
    assert found, f"no main-cast line in {name}.{project}.txt"
    return tuple(float(x) for x in found.groups())


def main():
    sys.stdout.reconfigure(encoding="utf-8")  # the scorer's own lines hold signs a Windows console can't write
    args = sys.argv[1:]
    against = None
    if "--against" in args:
        i = args.index("--against")
        against = args[i + 1]
        del args[i : i + 2]
    settings = dict(a.split("=", 1) for a in args if "=" in a)
    args = [a for a in args if "=" not in a]
    if not args:
        sys.exit(__doc__)
    name, embs = args[0], (Path(args[1]) if len(args) > 1 else None)
    rows = []
    for project in PROJECTS:
        emb = None if embs is None else embs / f"{project}.emb"
        rows.append((project, *score(name, project, emb, settings)))
        print("%-9s P %.3f  R %.3f  F %.3f" % rows[-1], flush=True)
    print("%-9s P %.3f  R %.3f  F %.3f" % ("macro", *(sum(r[k] for r in rows) / len(rows) for k in (1, 2, 3))))
    if against:
        for project in PROJECTS:
            items = [str(WORK / "score" / f"{n}.{project}.items.tsv") for n in (against, name)]
            done = subprocess.run([EXE, "paired", *items], env=dict(os.environ, BOOT="2000"), capture_output=True, text=True, encoding="utf-8", errors="replace")
            print(f"-- {project}: {name} against {against}")
            print(done.stdout.strip() or done.stderr.strip())


if __name__ == "__main__":
    main()
