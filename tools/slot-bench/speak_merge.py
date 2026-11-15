"""Two readers' shot labels (`speak_sheets.py`'s sheets) as one file for `conversation_bench`.

    uv run speak_merge.py <sheets/rows.tsv> <reader_a_dir> <reader_b_dir> <cast.tsv> <out.shots.tsv> [<a.tsv> <b.tsv>]

Each reader's folder holds `part_*.tsv` files (`shot faces speaks`, names with
commas between them, `-` for nobody). Checked: every shot of `rows.tsv` has
one line per reader and no other shot has one; every name is a label of
`<cast.tsv>` or `other`. Mended, and counted in what is printed: a speaker
who is not among the reader's faces is added to them; speakers of a shot
with no heard speech are dropped. Nothing else is changed.

Written: `frame speech_s faces_a faces_b speaks_a speaks_b` per shot, and
with `<a.tsv> <b.tsv>` each reader's lines as one file (`shot faces speaks`).
"""
import glob
import os
import sys


def names(field):
    out = sorted({n.strip().lower() for n in field.split(",")} - {"", "-"})
    return out


def reader(folder, shots, labels, heard):
    rows, mended = {}, {"speaker added to the faces": 0, "speakers of a silent shot dropped": 0}
    for path in sorted(glob.glob(os.path.join(folder, "part_*.tsv"))):
        lines = [line.rstrip("\r\n") for line in open(path, encoding="utf-8") if line.strip()]
        assert lines[0].split("\t") == ["shot", "faces", "speaks"], f"{path}: header `{lines[0]}`"
        for line in lines[1:]:
            f = line.split("\t")
            assert len(f) == 3, f"{path}: three fields, not `{line}`"
            shot, faces, speaks = int(f[0]), names(f[1]), names(f[2])
            assert shot in shots, f"{path}: shot {shot} is not on the sheets"
            assert shot not in rows, f"{path}: shot {shot} has a line already"
            unknown = [n for n in faces + speaks if n not in labels and n != "other"]
            assert not unknown, f"{path}: shot {shot}: {unknown} are no labels of the cast sheet"
            if speaks and heard[shot] <= 0:
                speaks = []
                mended["speakers of a silent shot dropped"] += 1
            for n in speaks:
                if n not in faces:
                    faces = sorted(faces + [n])
                    mended["speaker added to the faces"] += 1
            rows[shot] = (faces, speaks)
    missing = sorted(set(shots) - set(rows))
    assert not missing, f"{folder}: no line for shots {missing[:20]} ({len(missing)} in all)"
    return rows, mended


def main():
    rows_path, a_dir, b_dir, cast_path, out = sys.argv[1:6]
    shots, heard = {}, {}
    for line in list(open(rows_path, encoding="utf-8"))[1:]:
        _, shot, start, _, speech = line.rstrip("\n").split("\t")
        shots[int(shot)] = int(start)
        heard[int(shot)] = float(speech)
    labels = {line.split("\t")[0] for line in list(open(cast_path, encoding="utf-8"))[1:] if line.strip()}
    a, mended_a = reader(a_dir, shots, labels, heard)
    b, mended_b = reader(b_dir, shots, labels, heard)
    show = lambda v: ",".join(v) or "-"
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("frame\tspeech_s\tfaces_a\tfaces_b\tspeaks_a\tspeaks_b\n")
        for shot in sorted(shots):
            f.write(f"{shots[shot]}\t{heard[shot]:.2f}\t{show(a[shot][0])}\t{show(b[shot][0])}\t{show(a[shot][1])}\t{show(b[shot][1])}\n")
    for path, rows in zip(sys.argv[6:8], (a, b)):
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write("shot\tfaces\tspeaks\n")
            for shot in sorted(rows):
                f.write(f"{shot}\t{show(rows[shot][0])}\t{show(rows[shot][1])}\n")
    print(f"{len(shots)} shots in {out}; mended for reader a: {mended_a}; for reader b: {mended_b}")


main()
