"""The expressions check set: its labels, and the ready-made models scored against them.

    uv run expr_check.py join  <check_dir>
    uv run expr_check.py final <check_dir> <project_copy_dir> <out.tsv>
    uv run expr_check.py score <crops_dir> <scores_dir> <check.tsv> [<sound.tsv> <fps_num> <fps_den>]

`<check_dir>` is what expr_select.py and crop_sheets.py made (`tracks.tsv`,
`list.tsv`, `sheets/index.tsv`), with the two labellers' files in `a/` and
`b/` (`part*.tsv`: `crop labels sure`).

`join` compares the two, prints how far they agree and writes
`joined.tsv` and `disputed/list.tsv`: the crops they answered differently,
for crop_sheets.py and a third labeller (`c/part*.tsv`, numbered as on the
disputed sheets).

`final` writes the label file of `bench/expressions`: per crop its track,
frame and face box (shares of the frame) and the track's character, from the
project copy; its group and weight, each labeller's answer and the label.
Where two agree that is the label. Else, with the third: a face that all
three judged gets the labels two of them gave (`none` when there is no such
label); a face that only two judged is `unclear`, since they did not agree;
`noface` wants two votes; anything else is `unclear`.

`score` rates every output of every model against every label, per crop and
per track (a track shows a label when one of its judged crops does; its
score is the highest of its judged crops). `unclear` crops are left out,
`noface` crops count as showing nothing. All numbers are weighted, so that
they stand for the whole episode: average precision (AP), the precision at
the highest threshold that still finds half of the label (P@R50, the gate of
`bench/models/DECISIONS.md` row 5: at least 0.80, and the lower end of its
95 % interval at least 0.70), and the largest recall with a precision of
0.80 (R@P80). `few` in place of a verdict: fewer than 20 tracks show the
label, or fewer than 5 of them are of the random group, which is too little
to say anything (`tracks` and `rest` are those two counts; a track's crops
are near copies of each other, so crops are not counted). The interval is a
bootstrap: the characters of the project are drawn with all their tracks (a
track without a character on its own), within each group, since one
person's faces are not independent of each other. It says how much the
number hangs on which people and tracks the episode happens to hold, not
how another title would do. `mean:<label>` is the mean of the percentile
ranks of the label's candidate outputs. Per label the outputs of highest AP
are printed (`TOP=` how many, 3), then the best candidate and the mean if
they are not among them; `*` marks one named as a candidate before the
labels existed. `unclear` is the share of `unclear` crops among all crops
at or above the threshold: what the output would also flag, and no labeller
could judge. Then, per label, for the mean of its candidates (fixed before
the labels, so nothing is picked here): whether a crop gains from its
track's other crops, and, with a sound table of sound_events.py, what the
soundtrack adds.
"""
import csv
import glob
import os
import sqlite3
import sys
from collections import Counter

import numpy as np

import expr_scores as es

SPECIAL = ("none", "unclear", "noface")
DET = np.dtype([("frame", "<u4"), ("x1", "<f4"), ("y1", "<f4"), ("x2", "<f4"), ("y2", "<f4"),
                ("score", "<f4"), ("norm", "<f4"), ("quality", "<f4")])
SOUND = {"laugh": "laughing", "cry": "crying", "shout": "shouting"}
DRAWS, FEW, FEW_REST = 1000, 20, 5


def rows_of(path):
    return list(csv.DictReader(open(path, encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE))


def answer(text):
    """A labeller's answer in one spelling: `none`, `unclear`, `noface`, or the labels in the list's order."""
    names = {t.strip() for t in text.split(",") if t.strip()}
    if len(names) == 1 and next(iter(names)) in SPECIAL:
        return next(iter(names))
    assert names and names <= set(es.LABELS), f"not an answer: {text!r}"
    return ",".join(n for n in es.LABELS if n in names)


def answers(folder, n):
    got = {}
    for path in sorted(glob.glob(os.path.join(folder, "part*.tsv"))):
        for r in rows_of(path):
            k = int(r["crop"])
            assert k not in got, f"{path}: crop {k} twice"
            got[k] = answer(r["labels"])
    assert sorted(got) == list(range(n)), f"{folder}: {len(got)} answers for {n} crops"
    return [got[k] for k in range(n)]


def shows(a, label):
    return a not in SPECIAL and label in a.split(",")


def join(check):
    listed, groups = rows_of(os.path.join(check, "list.tsv")), {r["track"]: r for r in rows_of(os.path.join(check, "tracks.tsv"))}
    index = rows_of(os.path.join(check, "sheets", "index.tsv"))
    assert [r["file"] for r in index] == [r["file"] for r in listed], "the sheets are not of this list"
    a, b = answers(os.path.join(check, "a"), len(listed)), answers(os.path.join(check, "b"), len(listed))
    with open(os.path.join(check, "joined.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("crop\tfile\ttrack\tframe\tgroup\tweight\ta\tb\n")
        for k, r in enumerate(listed):
            g = groups[r["track"]]
            f.write(f"{k}\t{os.path.basename(r['file'])}\t{r['track']}\t{r['frame']}\t{g['group']}\t{g['weight']}\t{a[k]}\t{b[k]}\n")
    apart = [k for k in range(len(listed)) if a[k] != b[k]]
    os.makedirs(os.path.join(check, "disputed"), exist_ok=True)
    order = np.random.default_rng(2).permutation(apart)
    with open(os.path.join(check, "disputed", "list.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("file\tcrop\n")
        for k in order:
            f.write(f"{listed[k]['file']}\t{k}\n")
    n = len(listed)
    print(f"{n} crops, {n - len(apart)} answered alike ({(n - len(apart)) / n:.1%}), {len(apart)} for the third labeller")
    print("answer\ta\tb\tboth\tkappa")
    for name in es.LABELS + ["unclear", "noface"]:
        x = np.array([shows(v, name) or v == name for v in a])
        y = np.array([shows(v, name) or v == name for v in b])
        chance = x.mean() * y.mean() + (1 - x.mean()) * (1 - y.mean())
        kappa = ((x == y).mean() - chance) / (1 - chance) if chance < 1 else float("nan")
        print(f"{name}\t{x.sum()}\t{y.sum()}\t{(x & y).sum()}\t{kappa:.2f}")


def settle(votes):
    if len(set(votes)) == 1:
        return votes[0]
    assert len(votes) == 3, "two answers apart and no third"
    judged = [v for v in votes if v not in ("unclear", "noface")]
    if len(judged) == 2:  # the two who judged the face are the two who differ, or one of them and the third
        return judged[0] if judged[0] == judged[1] else "unclear"
    if len(judged) == 3:
        given = Counter(name for v in judged if v != "none" for name in v.split(","))
        return ",".join(n for n in es.LABELS if given[n] >= 2) or "none"
    return "noface" if votes.count("noface") >= 2 else "unclear"


def final(check, project, out):
    joined = rows_of(os.path.join(check, "joined.tsv"))
    apart = [int(r["crop"]) for r in rows_of(os.path.join(check, "disputed", "list.tsv"))]
    third = dict(zip(apart, answers(os.path.join(check, "c"), len(apart)))) if apart else {}
    db = sqlite3.connect(f"file:{project}/project.db?mode=ro", uri=True)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("file\ttrack\tframe\tx1\ty1\tx2\ty2\tcharacter\tgroup\tweight\ta\tb\tc\tlabels\n")
        for r in sorted(joined, key=lambda r: (int(r["track"]), int(r["frame"]))):
            dets, name, number = db.execute(
                "SELECT t.dets, c.name, c.number FROM tracks t LEFT JOIN characters c ON c.id = t.character_id WHERE t.id = ?",
                (int(r["track"]),)).fetchone()
            dets = np.frombuffer(dets, DET)
            who = (name or (f"Character {number}" if number is not None else "")).replace("\t", " ")
            d = dets[dets["frame"] == int(r["frame"])][0]
            k = int(r["crop"])
            votes = [r["a"], r["b"]] + ([third[k]] if k in third else [])
            assert (k in third) == (r["a"] != r["b"])
            box = "\t".join(f"{d[c]:.4f}" for c in ("x1", "y1", "x2", "y2"))
            f.write(f"{r['file']}\t{r['track']}\t{r['frame']}\t{box}\t{who}\t{r['group']}\t{r['weight']}\t{r['a']}\t{r['b']}"
                    f"\t{third.get(k, '')}\t{settle(votes)}\n")
    labels = Counter(settle([r["a"], r["b"]] + ([third[int(r["crop"])]] if int(r["crop"]) in third else [])) for r in joined)
    print(f"{len(joined)} crops in {out}: {len(third)} went to the third labeller;"
          f" {labels['unclear']} unclear, {labels['noface']} no face, {labels['none']} showing nothing")


def measures(s, y, w):
    """AP, P@R50, R@P80 and the threshold of P@R50 of scores `s` against 0/1 `y`, once per row of weights `w`."""
    order = np.argsort(-s, kind="stable")
    s, y, w = s[order], y[order], np.atleast_2d(w)[:, order]
    ends = np.append(s[1:] != s[:-1], True)  # equal scores stand or fall together
    hit, all_ = np.cumsum(w * y, 1)[:, ends], np.cumsum(w, 1)[:, ends]
    with np.errstate(invalid="ignore", divide="ignore"):
        precision, recall = hit / all_, hit / hit[:, -1:]
        ap = np.nansum(np.diff(recall, prepend=0, axis=1) * np.nan_to_num(precision), 1)
        half = np.argmax(recall >= 0.5, 1)
        at_half = precision[np.arange(len(w)), half]
        reach = np.where(precision >= 0.8, recall, 0).max(1)
    nan = np.where(hit[:, -1] > 0, 1.0, np.nan)  # a draw without the label says nothing
    return ap * nan, at_half * nan, reach * nan, s[ends][half]


def percentile(x):
    """Each value's place among all of them, 0 to 1; equal values share the mean of their places."""
    places = np.empty(len(x))
    places[np.argsort(x, kind="stable")] = np.arange(len(x))
    _, same, count = np.unique(x, return_inverse=True, return_counts=True)
    return (np.bincount(same, places) / count)[same] / (len(x) - 1)


def table(path):
    """The check set: per crop its file, track, group, weight, label and who it is (the character, else the track)."""
    return [(r["file"], int(r["track"]), r["group"], float(r["weight"]), r["labels"], r["character"] or f"track {r['track']}")
            for r in rows_of(path)]


def add_means(values):
    """`mean:<label>` beside the outputs: the mean place of a label's candidates among all crops of the episode."""
    for label, names in es.CANDIDATES.items():
        values[f"mean:{label}"] = np.mean([percentile(values[n]) for n in names], 0)
    return values


def track_draws(tracks, group, who):
    """DRAWS bootstrap draws: how often each track counts, when the characters (`who`) are drawn with
    replacement within each group and a character brings all its tracks of that group."""
    rng = np.random.default_rng(3)
    mult = np.zeros((DRAWS, len(tracks)))
    for g in sorted(set(group.values())):
        members = [i for i, t in enumerate(tracks) if group[t] == g]
        people = sorted({who[tracks[i]] for i in members})
        drawn = rng.multinomial(len(people), np.full(len(people), 1 / len(people)), DRAWS)
        mult[:, members] = drawn[:, [people.index(who[tracks[i]]) for i in members]]
    return mult


def verdict(tracks, rest, half, low):
    """The gate of row 5; a label that too few tracks show (or too few of the random group) is not judged."""
    return "few" if tracks < FEW or rest < FEW_REST else "pass" if half >= 0.8 and low >= 0.7 else "no"


def score(crops_dir, scores_dir, check_path, sound=None):
    crops = es.crops(crops_dir)
    files = [r["file"] for r in crops]
    values = es.outputs(scores_dir, files)
    track_of = np.array([int(r["track"]) for r in crops])
    add_means(values)
    candidates = {label: set(names) | {f"mean:{label}"} for label, names in es.CANDIDATES.items()}
    candidates["smile+laugh"] = candidates["smile"] | candidates["laugh"]
    check = table(check_path)
    place = {f: i for i, f in enumerate(files)}
    tracks = sorted({c[1] for c in check})
    group = {c[1]: c[2] for c in check}
    weight = {c[1]: c[3] for c in check}
    mult = track_draws(tracks, group, {c[1]: c[5] for c in check})
    at = {t: i for i, t in enumerate(tracks)}
    states = Counter()
    for c in check:
        states[c[4] if c[4] in ("unclear", "noface") else "judged"] += c[3]
    total = sum(states.values())
    print(f"{len(check)} crops of {len(tracks)} tracks; of the episode's crops {states['unclear'] / total:.1%} are unclear"
          f" and {states['noface'] / total:.1%} hold no face")
    judged = [c for c in check if c[4] != "unclear"]
    labels = es.LABELS + ["smile+laugh"]

    def truth(c, label):
        return any(shows(c[4], part) for part in label.split("+"))

    levels = {}
    # per crop
    rows = np.array([place[c[0]] for c in judged])
    levels["crop"] = (
        {n: v[rows] for n, v in values.items()},
        {label: np.array([truth(c, label) for c in judged], float) for label in labels},
        np.array([c[3] for c in judged]), np.array([at[c[1]] for c in judged]))
    # per track: the highest of its judged crops, against any of them showing the label
    seen = sorted({c[1] for c in judged})
    members = {t: np.flatnonzero(track_of == t) for t in seen}
    own = {t: np.array([place[c[0]] for c in judged if c[1] == t]) for t in seen}
    levels["track"] = (
        {n: np.array([v[own[t]].max() for t in seen]) for n, v in values.items()},
        {label: np.array([any(truth(c, label) for c in judged if c[1] == t) for t in seen], float) for label in labels},
        np.array([weight[t] for t in seen]), np.array([at[t] for t in seen]))
    top = int(os.environ.get("TOP", "3"))
    murky = [c for c in check if c[4] == "unclear"]
    murky_at, murky_w = np.array([place[c[0]] for c in murky], int), np.array([c[3] for c in murky])
    in_rest = np.array([group[t] == "rest" for t in seen])
    for level, (v, y, w, unit) in levels.items():
        print(f"\nper {level}\nlabel\tshown\ttracks\trest\tshare\toutput\tAP\tP@R50\t95 %\tR@P80\tthreshold\tgate\tunclear")
        for label in labels:
            n, share = int(y[label].sum()), (w * y[label]).sum() / w.sum()
            showing = levels["track"][1][label] > 0
            n_tracks, n_rest = int(showing.sum()), int((showing & in_rest).sum())
            if n == 0:
                print(f"{label}\t0\t0\t0\t0\t-")
                continue
            point = {name: measures(s, y[label], w) for name, s in v.items()}
            ranked = sorted(point, key=lambda name: -point[name][0][0])
            named = next((name for name in ranked if name in candidates[label] and not name.startswith("mean:")), None)
            shown = ranked[:top] + [name for name in [named, f"mean:{label}"] if name in point and name not in ranked[:top]]
            for name in shown:
                ap, half, reach, threshold = (m[0] for m in point[name])
                draws = measures(v[name], y[label], w * mult[:, unit])[1]
                low, high = np.nanpercentile(draws, [2.5, 97.5])
                gate = verdict(n_tracks, n_rest, half, low)
                mark = "*" if name in candidates[label] else ""
                dim = "-"
                if level == "crop":
                    above = (murky_w * (values[name][murky_at] >= threshold)).sum()
                    dim = f"{above / (above + (w * (v[name] >= threshold)).sum()):.0%}"
                print(f"{label}\t{n}\t{n_tracks}\t{n_rest}\t{share:.1%}\t{name}{mark}\t{ap:.3f}\t{half:.2f}\t{low:.2f} to {high:.2f}\t{reach:.2f}"
                      f"\t{threshold:.4g}\t{gate}\t{dim}")
    # Does a crop gain from the other crops of its track?
    v, y, w, unit = levels["crop"]
    print("\nper crop, the track's other crops added\nlabel\toutput\tAP alone\tAP track mean\tAP track highest")
    pooled = {}
    for label in labels:
        name = f"mean:{label.split('+')[0]}"
        if not y[label].any():
            continue
        full = values[name]
        for how in ("mean", "max"):
            per_track = {t: getattr(full[m], how)() for t, m in members.items()}
            pooled[how] = np.array([per_track[c[1]] for c in judged])
        print(f"{label}\t{name}\t{measures(v[name], y[label], w)[0][0]:.3f}\t{measures(pooled['mean'], y[label], w)[0][0]:.3f}"
              f"\t{measures(pooled['max'], y[label], w)[0][0]:.3f}")
    if sound:
        heard = rows_of(sound[0])
        secs = np.array([float(r["sec"]) for r in heard])
        fps = int(sound[1]) / int(sound[2])
        when = np.array([int(crops[place[c[0]]]["frame"]) / fps for c in judged])
        print("\nthe soundtrack within half a second of the crop\nlabel\toutput\tAP face\tAP sound\tAP both"
              "\tshown and heard (0.1)\tnot shown but heard")
        for label, column in SOUND.items():
            name = f"mean:{label}"
            if not y[label].any():
                continue
            loud = np.array([float(r[column]) for r in heard])
            near = np.array([loud[(secs + 0.96 >= t - 0.5) & (secs <= t + 0.5)].max(initial=0) for t in when])
            both = (percentile(v[name]) + percentile(near)) / 2
            yes = y[label] > 0
            print(f"{label}\t{name}\t{measures(v[name], y[label], w)[0][0]:.3f}\t{measures(near, y[label], w)[0][0]:.3f}"
                  f"\t{measures(both, y[label], w)[0][0]:.3f}\t{(w * yes * (near >= 0.1)).sum() / (w * yes).sum():.0%}"
                  f"\t{(w * ~yes * (near >= 0.1)).sum() / (w * ~yes).sum():.1%}")


def main():
    what, args = sys.argv[1], sys.argv[2:]
    if what == "join":
        join(args[0])
    elif what == "final":
        final(*args[:3])
    elif what == "score":
        score(args[0], args[1], args[2], args[3:6] if len(args) >= 6 else None)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
