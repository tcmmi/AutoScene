"""Joins two labellers' scene starts of one episode, or writes the final labels.

    uv run scene_merge.py join <a.tsv> <b.tsv> <out_prefix>
    uv run scene_merge.py final <out_prefix> <shots.tsv> <scenes.tsv>

`join` reads two files of `shot sure note` rows (a scene's first shot) and
writes `<out_prefix>.agreed.tsv` (the starts both name) and
`<out_prefix>.disputes.tsv` (`shot who sure note`: the starts only one names;
two that lie one shot apart are one dispute with both rows). An adjudicator
then writes `<out_prefix>.settled.tsv` (`shot sure note`: the disputed starts
that stand).

`final` joins the agreed and the settled starts and writes the label file of
`bench/scenes` (`frame sure note`), a scene's first shot named by its first
frame (`shots.tsv` of `shot_sheets.py` turns numbers into frames).
"""
import sys


def read(path):
    rows = [line.rstrip("\n").split("\t") for line in open(path, encoding="utf-8") if line.strip()]
    assert rows[0][0] == "shot", f"{path}: the header starts with `shot`"
    out = {int(r[0]): (r[1].strip(), r[2].strip() if len(r) > 2 else "") for r in rows[1:]}
    assert list(out) == sorted(out), f"{path}: shot numbers must rise"
    return out


def write(path, header, rows):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + "\n")
        for r in rows:
            f.write("\t".join(str(x) for x in r) + "\n")


def join(a_path, b_path, out):
    a, b = read(a_path), read(b_path)
    agreed = sorted(set(a) & set(b) | {0})
    sure = lambda s: "yes" if a.get(s, ("yes",))[0] == "yes" and b.get(s, ("yes",))[0] == "yes" else "maybe"
    write(out + ".agreed.tsv", "shot\tsure\tnote", [(s, sure(s), a.get(s, b.get(s, ("", "start")))[1]) for s in agreed])
    disputes = [(s, "a", *a[s]) for s in a if s not in b and s != 0] + [(s, "b", *b[s]) for s in b if s not in a and s != 0]
    write(out + ".disputes.tsv", "shot\twho\tsure\tnote", sorted(disputes))
    both = len(set(a) & set(b))
    print(f"a {len(a)} scenes, b {len(b)}; {both} starts agreed, {len(disputes)} disputed "
          f"(agreement F {2 * both / (len(a) + len(b)):.3f})")


def final(out, shots_path, scenes_path):
    starts = {**read(out + ".agreed.tsv"), **read(out + ".settled.tsv")}
    first = {}
    for line in list(open(shots_path, encoding="utf-8"))[1:]:
        shot, start, _ = line.split("\t")
        first[int(shot)] = int(start)
    write(scenes_path, "frame\tsure\tnote", [(first[s], *starts[s]) for s in sorted(starts)])
    print(f"{scenes_path}: {len(starts)} scenes")


if sys.argv[1] == "join":
    join(*sys.argv[2:5])
else:
    final(*sys.argv[2:5])
