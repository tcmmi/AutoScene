"""Repeated frames per shot: FFmpeg's `mpdecimate` against the signals the app stores (`bench/motion`).

Run from the repository root (Python 3.14 for its zstd; nothing is downloaded when the cache has numpy and pillow):

    uv run --no-project --python 3.14 --with numpy --with pillow tools/motion-bench/held.py <command> ...

    reference <bench/motion> <ffmpeg> [key ...]
        Decodes each episode of `episodes.tsv` once (every frame, in order, at its own size) through three
        `mpdecimate` settings and writes which frames each drops to `ref/<key>.txt`. Reads the video the
        bench input's `bb.path` names; writes nothing beside it.
    compare <bench/motion>
        Reads the stored signals and shots of every episode and the reference files, writes `shots.tsv` (one
        row per scored shot) and `tables.md` (every table of the README).
    pairs <bench/motion> <ffmpeg> <work dir> reference|errors
        Draws the frame pairs of the eye check (`reference`: four strata of the reference; `errors`: frames
        where the chosen candidate and the default reference differ), writes `eye.<which>.tsv` (the key, kept
        from the reader) and renders one page per pair into `<work dir>/<which>/` under a shuffled number.
    read <bench/motion> reference|errors <answers.tsv> [<answers.tsv> ...]
        Joins readers' answers (`page verdict note`) to the key and prints the counts per stratum.

The signals are read from a bench input (`bb.sig`, `bb.segments.tsv`) or from a copy of a project
(`project.db`, tables `shot_signals` and `segments`), opened read-only. Never from a library.
"""

import random
import re
import sqlite3
import subprocess
import sys
from compression import zstd
from pathlib import Path

import numpy as np

# The reference: mpdecimate's `hi`, `lo`, `frac` (README, "Reference").
SETTINGS = {"default": (768, 320, 0.33), "strict": (256, 128, 0.10), "loose": (1536, 640, 0.33)}
# The candidate (README, "Candidate").
C1_BARS = [0.02, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5, 0.75, 1.0, 1.5]
C2_GRID = [(floor, bar, rho) for floor in (0.02, 0.05) for bar in (0.3, 0.5, 1.0) for rho in (0.25, 0.5)]
MIN_FRAMES = 12  # shorter shots are counted, not scored
NEAR = 0.10  # a held share this close to the reference's counts as the same
FLAGS = (0.10, 0.50)  # "has repeated frames" from this held share
SEED = 20261115
FOOTAGE = ("anime", "3d", "live")
CADENCES = ("1", "2", "3", "4+")

# signals.rs: format byte 3, then per frame 4 + LAGS + 3 little-endian floats and the colour signature.
FLOATS, COLORS, FORMAT = 11, 48, 3
RECORD = FLOATS * 4 + COLORS


def episodes(bench):
    lines = (Path(bench) / "episodes.tsv").read_text(encoding="utf-8").splitlines()
    head = lines[0].split("\t")
    return [dict(zip(head, line.split("\t"))) for line in lines[1:] if line]


def video_of(row):
    return (Path(row["video"]) / "bb.path").read_text(encoding="utf-8").strip()


def unpack(blob):
    raw = zstd.decompress(blob)
    assert raw[0] == FORMAT and (len(raw) - 1) % RECORD == 0, "not a signals blob of format 3"
    records = np.frombuffer(raw, dtype=np.uint8, offset=1).reshape(-1, RECORD)
    return np.ascontiguousarray(records[:, : FLOATS * 4]).view("<f4").reshape(-1, FLOATS)


def read_only(db):
    """A copy of a project, opened so that nothing beside it is written: `mode=ro` alone still touches the
    side files of a write-ahead-log database. An immutable open does not read that log, so it must be empty."""
    log = Path(str(db) + "-wal")
    assert not log.exists() or log.stat().st_size == 0, f"{log} is not empty: checkpoint the copy first"
    return sqlite3.connect(f"file:{Path(db).as_posix()}?mode=ro&immutable=1", uri=True)


def load_signals(source):
    """`diff[0..4]` of every frame: the mean absolute RGB difference to the 1 to 4 frames before, 0..255."""
    if source.endswith(".db"):
        db = read_only(source)
        media = [m for (m,) in db.execute("select distinct media_id from shot_signals")]
        assert len(media) == 1, "the project copy holds one media"
        chunks = db.execute("select start, data from shot_signals where media_id = ? order by start", media).fetchall()
        parts, at = [], 0
        for start, data in chunks:
            assert start == at, f"signal chunks leave a gap at {at}"
            parts.append(unpack(data))
            at += len(parts[-1])
        floats = np.concatenate(parts)
    else:
        floats = unpack((Path(source) / "bb.sig").read_bytes())
    return floats[:, 4:8].astype(np.float64)


def load_shots(source):
    """Frames `[start, end)` of every segment of kind `shot`, as the fusion made them."""
    if source.endswith(".db"):
        db = read_only(source)
        rows = db.execute("select start, end, kind from segments order by start").fetchall()
    else:
        rows = [line.split("\t") for line in (Path(source) / "bb.segments.tsv").read_text(encoding="utf-8").splitlines() if line]
    return [(int(a), int(b)) for a, b, kind in rows if kind == "shot"]


# -- the reference ------------------------------------------------------------


def reference(bench, ffmpeg, keys):
    out = Path(bench) / "ref"
    out.mkdir(exist_ok=True)
    for row in episodes(bench):
        if keys and row["key"] not in keys:
            continue
        video = video_of(row)
        names = list(SETTINGS)
        graph = "[0:v:0]setpts=N,showinfo=checksum=0,split=3" + "".join(f"[i{k}]" for k in range(3)) + ";"
        graph += ";".join(
            f"[i{k}]mpdecimate=hi={SETTINGS[n][0]}:lo={SETTINGS[n][1]}:frac={SETTINGS[n][2]},showinfo=checksum=0[o{k}]" for k, n in enumerate(names)
        )
        cmd = [ffmpeg, "-hide_banner", "-nostdin", "-nostats", "-loglevel", "info", "-i", video, "-an", "-sn", "-dn", "-filter_complex", graph]
        for k in range(3):
            cmd += ["-map", f"[o{k}]", "-fps_mode", "passthrough", "-f", "null", "-"]
        seen = {}  # showinfo instance -> the frame numbers that reached it
        line_of = re.compile(r"Parsed_showinfo_(\d+) @ [^\]]*\] n: *(\d+) pts: *(\d+) ")
        proc = subprocess.Popen(cmd, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL, text=True, encoding="utf-8", errors="replace")
        version = ""
        tail = []
        for line in proc.stderr:
            hit = line_of.search(line)
            if hit:
                seen.setdefault(int(hit.group(1)), []).append(int(hit.group(3)))
            else:
                tail = (tail + [line.rstrip()])[-8:]
        assert proc.wait() == 0, "ffmpeg failed:\n" + "\n".join(tail)
        version = subprocess.run([ffmpeg, "-hide_banner", "-version"], capture_output=True, text=True).stdout.splitlines()[0]
        order = sorted(seen)  # the instances in the order of the graph: all frames, then one per setting
        assert len(order) == 4, f"expected four showinfo instances, saw {order}"
        every = seen[order[0]]
        frames = len(every)
        assert every == list(range(frames)), "the frames before the split are not 0, 1, 2, ..."
        text = [f"# {row['key']}\tframes {frames}\t{version}\tevery decoded frame in order; a set bit is a frame mpdecimate dropped, frame f at bit f % 8 of byte f // 8"]
        for name, instance in zip(names, order[1:]):
            kept = np.zeros(frames, dtype=bool)
            kept[seen[instance]] = True
            assert kept[0], "mpdecimate keeps the first frame"
            dropped = ~kept
            hi, lo, frac = SETTINGS[name]
            text.append(f"[{name}]\thi={hi} lo={lo} frac={frac}\tdropped {int(dropped.sum())}")
            hexed = np.packbits(dropped, bitorder="little").tobytes().hex()
            text += [hexed[i : i + 64] for i in range(0, len(hexed), 64)]
        (out / f"{row['key']}.txt").write_text("\n".join(text) + "\n", encoding="utf-8", newline="\n")
        print(f"{row['key']}: {frames} frames; dropped " + ", ".join(f"{n} {int((~np.isin(np.arange(frames), seen[i])).sum())}" for n, i in zip(names, order[1:])), flush=True)


def load_reference(bench, key):
    """Per setting, for every frame: did mpdecimate drop it?"""
    lines = (Path(bench) / "ref" / f"{key}.txt").read_text(encoding="utf-8").splitlines()
    frames = int(lines[0].split("\t")[1].split()[1])
    masks, name, hexed = {}, None, []

    def close():
        if name:
            bits = np.unpackbits(np.frombuffer(bytes.fromhex("".join(hexed)), dtype=np.uint8), bitorder="little")
            masks[name] = bits[:frames].astype(bool)

    for line in lines[1:]:
        if line.startswith("["):
            close()
            name, hexed = line[1 : line.index("]")], []
        else:
            hexed.append(line)
    close()
    for name, mask in masks.items():
        stated = int(next(line for line in lines if line.startswith(f"[{name}]")).split("dropped ")[1])
        assert int(mask.sum()) == stated, f"{key} {name}: the mask holds {int(mask.sum())} drops, the header says {stated}"
    return frames, masks


# -- the candidate ------------------------------------------------------------


def c1(d1, bar):
    return d1 < bar


def c2(d1, floor, bar, rho):
    """`d1`: the differences of a shot's frames from its second on. Held: under the floor, or under the
    bar and under `rho` times the largest difference of the up to four frames around it in the shot."""
    n = len(d1)
    around = np.zeros(n)
    for step in (-2, -1, 1, 2):
        shifted = np.zeros(n)
        if step < 0:
            shifted[-step:] = d1[:step]
        else:
            shifted[:-step] = d1[step:]
        around = np.maximum(around, shifted)
    return (d1 < floor) | ((d1 < bar) & (d1 < rho * around))


def candidates():
    """Name and function of every setting, C1 first and each by rising bar: the order ties are broken in."""
    out = [(f"C1 {bar:g}", lambda d, bar=bar: c1(d, bar)) for bar in C1_BARS]
    out += [(f"C2 {floor:g}/{bar:g}/{rho:g}", lambda d, f=floor, b=bar, r=rho: c2(d, f, b, r)) for floor, bar, rho in C2_GRID]
    return out


def cadence(held):
    """`held`: per frame of a shot from its second on. The shot is cut into steps, a new frame and the
    held frames after it; the cadence is the step length (1, 2, 3, 4+) that holds the most frames."""
    new = np.flatnonzero(np.concatenate(([True], ~held)))  # the shot's first frame is new
    steps = np.diff(np.concatenate((new, [len(held) + 1])))
    weight = [int(steps[steps == 1].sum()), int(steps[steps == 2].sum()), int(steps[steps == 3].sum()), int(steps[steps >= 4].sum())]
    return CADENCES[int(np.argmax(weight))]


# -- the comparison -----------------------------------------------------------


def spearman(a, b):
    def rank(x):
        order = np.argsort(x, kind="stable")
        ranks = np.empty(len(x))
        ranks[order] = np.arange(len(x))
        for value in np.unique(x):  # ties share their mean rank
            same = x == value
            ranks[same] = ranks[same].mean()
        return ranks

    return pearson(rank(np.asarray(a)), rank(np.asarray(b)))


def pearson(a, b):
    a, b = np.asarray(a, dtype=float), np.asarray(b, dtype=float)
    if len(a) < 3 or a.std() == 0 or b.std() == 0:
        return float("nan")
    return float(np.corrcoef(a, b)[0, 1])


def fmt(x, digits=3):
    return "–" if x is None or (isinstance(x, float) and np.isnan(x)) else f"{x:.{digits}f}"


def ratio(a, b):
    return a / b if b else float("nan")


def load_episode(bench, row):
    d = load_signals(row["signals"])
    frames, ref = load_reference(bench, row["key"])
    shots = load_shots(row["signals"])
    return d, frames, ref, shots


def frame_rate(row):
    """The video's average frame rate as its bench input states it (`bb.info.json`)."""
    import json

    info = json.loads((Path(row["video"]) / "bb.info.json").read_text(encoding="utf-8"))
    rate = info["video"]["avgFrameRate"]
    return f"{rate['num'] / rate['den']:.3f}"


def alignment(d1, ref):
    """Mean stored difference of the frames the default reference drops, with the reference shifted."""
    dropped = np.flatnonzero(ref["default"])
    out = {}
    for shift in (-2, -1, 0, 1, 2):
        at = dropped + shift
        at = at[(at >= 0) & (at < len(d1))]
        out[shift] = float(d1[at].mean()) if len(at) else float("nan")
    return out


def compare(bench):
    bench = Path(bench)
    rows_of = episodes(bench)
    names = [name for name, _ in candidates()]
    shots = []  # one dict per scored shot
    short = {}
    frame_counts = {}  # (key, candidate, reference) -> [held by both, by the candidate only, by the reference only]
    per_episode = {}
    aligned = {}
    per_frame = {}  # footage -> per scored shot: the stored differences and the three references' drops
    all_frames = {}  # footage -> those, joined
    strict_drops, rates = {}, {}
    for row in rows_of:
        key = row["key"]
        d, frames, ref, segs = load_episode(bench, row)
        per_episode[key] = {"frames": frames, "signals": len(d), "shots": len(segs)}
        if frames != len(d):
            aligned[key] = (False, f"{frames} frames decoded, {len(d)} in the signals", {})
            continue
        shifts = alignment(d[:, 0], ref)
        ok = all(shifts[0] < shifts[s] for s in shifts if s != 0)
        aligned[key] = (ok, "", shifts)
        strict_drops[key] = np.flatnonzero(ref["strict"])
        rates[key] = frame_rate(row)
        short[key] = 0
        for a, b in segs:
            if b - a < MIN_FRAMES:
                short[key] += 1
                continue
            d1 = d[a + 1 : b, 0]
            per_frame.setdefault(row["footage"], []).append((d1, ref["strict"][a + 1 : b], ref["default"][a + 1 : b], ref["loose"][a + 1 : b]))
            shot = {"key": key, "footage": row["footage"], "role": row["role"], "start": a, "end": b, "n": b - a - 1, "motion": float(d1.mean())}
            for r, mask in ref.items():
                held = mask[a + 1 : b]
                shot[f"ref {r}"] = float(held.mean())
                shot[f"k ref {r}"] = int(held.sum())
                shot[f"cad {r}"] = cadence(held)
            for name, rule in candidates():
                held = rule(d1)
                shot[name] = float(held.mean())
                shot[f"k {name}"] = int(held.sum())
                shot[f"cad {name}"] = cadence(held)
                for r, mask in ref.items():
                    truth = mask[a + 1 : b]
                    c = frame_counts.setdefault((key, name, r), [0, 0, 0])
                    c[0] += int((held & truth).sum())
                    c[1] += int((held & ~truth).sum())
                    c[2] += int((~held & truth).sum())
            shots.append(shot)

    def pick(footage=None, role=None, key=None):
        return [s for s in shots if (footage is None or s["footage"] == footage) and (role is None or s["role"] == role) and (key is None or s["key"] == key)]

    def close(s, one, other):
        """Are two held shares of a shot within 0.10? Counted in whole frames (row 3): on rounded shares a
        shot exactly on the edge falls either way."""
        key = lambda x: f"k {x}"
        return 10 * abs(s[key(one)] - s[key(other)]) <= s["n"]

    assert NEAR == 0.10

    def near(group, name, r="default"):
        return ratio(sum(close(s, name, f"ref {r}") for s in group), len(group))

    def near_frames(group, name, r="default"):
        return ratio(sum(s["n"] for s in group if close(s, name, f"ref {r}")), sum(s["n"] for s in group))

    def flag(group, name, r, at):
        """Precision, recall, the share the reference flags, and the share of unflagged shots left unflagged."""
        cand = np.array([s[name] >= at for s in group])
        truth = np.array([s[f"ref {r}"] >= at for s in group])
        tp, fp, fn, tn = int((cand & truth).sum()), int((cand & ~truth).sum()), int((~cand & truth).sum()), int((~cand & ~truth).sum())
        return ratio(tp, tp + fp), ratio(tp, tp + fn), ratio(tp + fn, len(group)), ratio(tn, tn + fp), (tp, fp, fn, tn)

    # The setting, chosen per footage type on its fit episodes.
    chosen = {}
    fit_table = {}
    for footage in FOOTAGE:
        group = pick(footage, "fit")
        if not group:
            continue
        scores = [(near(group, name), name) for name in names]
        fit_table[footage] = scores
        best = max(score for score, _ in scores)
        chosen[footage] = next(name for score, name in scores if score == best)  # the first in the order of ties

    out = ["# Tables of the motion bench", "", "Written by `tools/motion-bench/held.py compare`; the README says what they mean.", ""]

    out += ["## Episodes", "", "| Key | Footage | Role | Frames | Shots | Scored (12 frames or more) | Frames in scored shots | Repeats there: default / strict / loose | Shots with repeats (default, share ≥ 0.10 / ≥ 0.50) | Cadence by the default reference: 1 / 2 / 3 / 4+ |", "|---|---|---|---|---|---|---|---|---|---|"]
    for row in rows_of:
        key = row["key"]
        group = pick(key=key)
        if not group:
            out.append(f"| `{key}` | {row['footage']} | {row['role']} | {per_episode[key]['frames']} | {per_episode[key]['shots']} | not aligned | | | | |")
            continue
        n = sum(s["n"] for s in group)
        shares = " / ".join(fmt(sum(s[f"ref {r}"] * s["n"] for s in group) / n) for r in SETTINGS)
        flagged = " / ".join(fmt(ratio(sum(s["ref default"] >= at for s in group), len(group))) for at in FLAGS)
        cads = " / ".join(str(sum(s["cad default"] == c for s in group)) for c in CADENCES)
        out.append(f"| `{key}` | {row['footage']} | {row['role']} | {per_episode[key]['frames']} | {per_episode[key]['shots']} | {len(group)} ({short[key]} shorter) | {n} | {shares} | {flagged} | {cads} |")

    out += ["", "## Alignment", "", "Mean stored `diff[0]` of the frames the default reference drops, with the reference shifted by a number of frames. Lowest at 0 = aligned.", "", "| Key | Frames decoded / in the signals | −2 | −1 | 0 | +1 | +2 | Aligned |", "|---|---|---|---|---|---|---|---|"]
    for row in rows_of:
        key = row["key"]
        ok, why, shifts = aligned[key]
        cells = " | ".join(fmt(shifts.get(s)) for s in (-2, -1, 0, 1, 2)) if shifts else " | | | | "
        out.append(f"| `{key}` | {per_episode[key]['frames']} / {per_episode[key]['signals']} | {cells} | {'yes' if ok else 'NO ' + why} |")

    out += ["", "## The setting, chosen on the fit episodes", "", "Share of the fit episodes' shots whose held share is within 0.10 of the default reference's.", "", "| Setting | " + " | ".join(f"{f} (fit)" for f in fit_table) + " |", "|---|" + "---|" * len(fit_table)]
    for i, name in enumerate(names):
        cells = " | ".join((f"**{fmt(fit_table[f][i][0])}**" if chosen[f] == name else fmt(fit_table[f][i][0])) for f in fit_table)
        out.append(f"| {name} | {cells} |")
    out += ["", "Chosen: " + "; ".join(f"{f}: {chosen[f]}" for f in chosen) + ".", ""]

    def result_row(label, group, name):
        cells = []
        for r in SETTINGS:
            p, rec, base, spec, counts = flag(group, name, r, FLAGS[0])
            p5, rec5, base5, spec5, _ = flag(group, name, r, FLAGS[1])
            cells.append(
                f"| {label} | {name} | {r} | {len(group)} | {fmt(near(group, name, r))} | {fmt(near_frames(group, name, r))} | "
                f"{fmt(pearson([s[name] for s in group], [s[f'ref {r}'] for s in group]))} | {fmt(spearman([s[name] for s in group], [s[f'ref {r}'] for s in group]))} | "
                f"{fmt(p)} / {fmt(rec)} | {fmt(base)} | {fmt(spec)} ({counts[3]} of {counts[3] + counts[1]}) | {fmt(p5)} / {fmt(rec5)} | {fmt(base5)} | {fmt(spec5)} |"
            )
        return cells

    head = ["| Shots | Setting | Reference | n | Held share within 0.10 | ... by frames | Pearson | Spearman | Flag ≥ 0.10: precision / recall | ... reference flags | ... unflagged left unflagged | Flag ≥ 0.50: precision / recall | ... reference flags | ... unflagged left unflagged |", "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    out += ["## Results with the chosen setting", ""] + head
    for footage in FOOTAGE:
        for name in ([chosen[footage]] if footage in chosen else list(dict.fromkeys(chosen.values()))):
            for role in ("check", "fit"):
                group = pick(footage, role)
                if group:
                    out += result_row(f"{footage}, {role}", group, name)
            for row in rows_of:
                if row["footage"] == footage:
                    group = pick(key=row["key"])
                    if group:
                        out += result_row(f"`{row['key']}` ({row['role']})", group, name)

    for r in SETTINGS:  # (the default reference is the plan's table; strict and loose were added by row 2)
        out += ["", f"## Every bar of C1 and every setting of C2, check episodes, {r} reference", ""]
        out += ["| Setting | " + " | ".join(f"{f}: within 0.10 | {f}: flag ≥ 0.10 precision / recall | {f}: unflagged left unflagged" for f in FOOTAGE) + " |", "|---|" + "---|---|---|" * len(FOOTAGE)]
        for name in names:
            cells = []
            for footage in FOOTAGE:
                group = pick(footage, "check")
                p, rec, base, spec, counts = flag(group, name, r, FLAGS[0])
                cells.append(f"{fmt(near(group, name, r))} | {fmt(p)} / {fmt(rec)} | {fmt(spec)} ({counts[3]} of {counts[3] + counts[1]})")
            out.append(f"| {name} | " + " | ".join(cells) + " |")
        best = {}
        for footage in fit_table:
            scores = [(near(pick(footage, "fit"), name, r), name) for name in names]
            top = max(score for score, _ in scores)
            best[footage] = next(f"{name} ({fmt(score)})" for score, name in scores if score == top)
        out += ["", f"On the fit episodes the {r} reference would choose: " + "; ".join(f"{f}: {best[f]}" for f in best) + "."]

    # Added by row 2, after the first numbers: how far the references are from each other, and what the
    # stored difference is in each band between them.
    out += ["", "## The references against each other (row 2)", "", "Share of shots whose held share by one reference is within 0.10 of the other's, and how one's flag (share ≥ 0.10) does against the other's. All episodes.", "", "| Footage | Pair | n | Held share within 0.10 | Pearson | Flag ≥ 0.10: precision / recall of the first against the second |", "|---|---|---|---|---|---|"]
    for footage in FOOTAGE:
        group = pick(footage)
        for one, other in (("strict", "default"), ("loose", "default"), ("strict", "loose")):
            a, b = np.array([s[f"ref {one}"] for s in group]), np.array([s[f"ref {other}"] for s in group])
            fa, fb = a >= FLAGS[0], b >= FLAGS[0]
            within = ratio(sum(close(s, f"ref {one}", f"ref {other}") for s in group), len(group))
            out.append(f"| {footage} | {one} against {other} | {len(group)} | {fmt(within)} | {fmt(pearson(a, b))} | {fmt(ratio(int((fa & fb).sum()), int(fa.sum())))} / {fmt(ratio(int((fa & fb).sum()), int(fb.sum())))} |")

    def auc(low, high):
        """How often a value of `low` lies under a value of `high` (ties count half)."""
        both = np.concatenate((low, high))
        order = np.argsort(both, kind="stable")
        ranks = np.empty(len(both))
        ranks[order] = np.arange(1, len(both) + 1)
        values, inverse, counts = np.unique(both, return_inverse=True, return_counts=True)
        sums = np.zeros(len(values))
        np.add.at(sums, inverse, ranks)
        ranks = (sums / counts)[inverse]
        return float((ranks[len(low) :].sum() - len(high) * (len(high) + 1) / 2) / (len(low) * len(high)))

    out += ["", "## The stored difference in each band of the reference (row 2)", "", "`diff[0]` of the frames of scored shots, all episodes. The last column: how often a frame of the band has a lower stored difference than a frame kept by all three (1 = the stored number alone could tell them apart, 0.5 = not at all).", "", "| Footage | Band | Frames | 10 % | Median | 90 % | 99 % | Lower than a frame kept by all three |", "|---|---|---|---|---|---|---|---|"]
    for footage in FOOTAGE:
        d1 = np.concatenate([x[0] for x in per_frame[footage]])
        strict, default, loose = (np.concatenate([x[i] for x in per_frame[footage]]) for i in (1, 2, 3))
        bands = {
            "dropped by strict": strict,
            "dropped by default, kept by strict": default & ~strict,
            "dropped by loose, kept by default": loose & ~default,
            "kept by all three": ~(strict | default | loose),
        }
        kept = d1[bands["kept by all three"]]
        for band, mask in bands.items():
            values = d1[mask]
            share = "" if band == "kept by all three" else fmt(auc(values, kept))
            out.append(f"| {footage} | {band} | {len(values)} | " + " | ".join(fmt(float(np.quantile(values, q))) for q in (0.1, 0.5, 0.9, 0.99)) + f" | {share} |")
        all_frames[footage] = (d1, strict, default, loose)

    # Added by row 3, after the review.
    out += ["", "## Every frame: dropped against kept, per reference (row 3)", "", "How often a frame the reference drops has a lower stored `diff[0]` than a frame the same reference keeps, over all frames of scored shots (the table above compares only the two clear bands).", "", "| Footage | Frames | Default | Strict | Loose |", "|---|---|---|---|---|"]
    for footage in FOOTAGE:
        d1, strict, default, loose = all_frames[footage]
        out.append(f"| {footage} | {len(d1)} | " + " | ".join(fmt(auc(d1[m], d1[~m])) for m in (default, strict, loose)) + " |")

    out += ["", "## How far the held share is off, and to which side (row 3)", "", "Check episodes, the chosen setting. The candidate's held share less the reference's, per shot.", "", "| Footage | Setting | Reference | n | Mean | Over by more than 0.10 | Under by more than 0.10 | Half the shots within | 90 % of the shots within |", "|---|---|---|---|---|---|---|---|---|"]
    for footage in FOOTAGE:
        group = pick(footage, "check")
        for name in ([chosen[footage]] if footage in chosen else list(dict.fromkeys(chosen.values()))):
            for r in SETTINGS:
                signed = np.array([s[name] - s[f"ref {r}"] for s in group])
                over = ratio(sum(10 * (s[f"k {name}"] - s[f"k ref {r}"]) > s["n"] for s in group), len(group))
                under = ratio(sum(10 * (s[f"k ref {r}"] - s[f"k {name}"]) > s["n"] for s in group), len(group))
                out.append(f"| {footage} | {name} | {r} | {len(group)} | {signed.mean():+.3f} | {fmt(over)} | {fmt(under)} | {fmt(float(np.quantile(np.abs(signed), 0.5)))} | {fmt(float(np.quantile(np.abs(signed), 0.9)))} |")

    if "anime" in chosen:
        out += ["", "## Anime check episodes without `op_7` (row 3)", "", "`op_7` is a file at 25 frames a second; see the next table.", ""] + head
        out += result_row("anime, check without `op_7`", [s for s in pick("anime", "check") if s["key"] != "op_7"], chosen["anime"])

    out += ["", "## Drops 24 or 25 frames apart (row 3)", "", "Of the gaps between one frame the strict reference drops and the next, how many are exactly 24 or 25 frames: a file that repeats a frame once a second to fill 25 frames a second shows many.", "", "| Key | Frames a second (bench input) | Strict drops | Gaps of 24 or 25 | Gaps of 23 or 26 |", "|---|---|---|---|---|"]
    for row in rows_of:
        key = row["key"]
        if key not in strict_drops:
            continue
        gaps = np.diff(strict_drops[key])
        out.append(f"| `{key}` | {rates.get(key, '')} | {len(strict_drops[key])} | {int(((gaps == 24) | (gaps == 25)).sum())} | {int(((gaps == 23) | (gaps == 26)).sum())} |")

    out += ["", "## Cadence, chosen setting against the default reference (check episodes)", "", "Rows: the reference's cadence; columns: the candidate's. Counts of shots.", ""]
    for footage in FOOTAGE:
        for name in ([chosen[footage]] if footage in chosen else list(dict.fromkeys(chosen.values()))):
            group = pick(footage, "check")
            same = ratio(sum(s[f"cad {name}"] == s["cad default"] for s in group), len(group))
            out += [f"{footage}, {name}: the same cadence in {fmt(same)} of {len(group)} shots.", "", "| Reference \\ candidate | " + " | ".join(CADENCES) + " |", "|---|" + "---|" * len(CADENCES)]
            for c in CADENCES:
                out.append(f"| {c} | " + " | ".join(str(sum(s["cad default"] == c and s[f"cad {name}"] == k for s in group)) for k in CADENCES) + " |")
            out.append("")

    out += ["## Per frame (diagnostic), chosen setting, check episodes", "", "| Footage | Setting | Reference | Frames | Held by the reference | Held by the candidate | Precision | Recall |", "|---|---|---|---|---|---|---|---|"]
    for footage in FOOTAGE:
        for name in ([chosen[footage]] if footage in chosen else list(dict.fromkeys(chosen.values()))):
            keys = [row["key"] for row in rows_of if row["footage"] == footage and row["role"] == "check" and pick(key=row["key"])]
            total = sum(s["n"] for s in pick(footage, "check"))
            for r in SETTINGS:
                both, cand_only, ref_only = (sum(frame_counts[(k, name, r)][i] for k in keys) for i in range(3))
                out.append(f"| {footage} | {name} | {r} | {total} | {both + ref_only} | {both + cand_only} | {fmt(ratio(both, both + cand_only))} | {fmt(ratio(both, both + ref_only))} |")

    out += ["", "## By-product: a plain motion score per shot", "", "Mean stored `diff[0]` of the shot's frames from its second on (0..255). No claim is made from it.", "", "| Shots | n | 5 % | 25 % | Median | 75 % | 95 % | Mean |", "|---|---|---|---|---|---|---|---|"]
    for footage in FOOTAGE:
        group = pick(footage)
        scores = np.array([s["motion"] for s in group])
        out.append(f"| {footage}, all episodes | {len(group)} | " + " | ".join(fmt(float(np.quantile(scores, q)), 2) for q in (0.05, 0.25, 0.5, 0.75, 0.95)) + f" | {fmt(float(scores.mean()), 2)} |")
    for row in rows_of:
        group = pick(key=row["key"])
        if group:
            scores = np.array([s["motion"] for s in group])
            out.append(f"| `{row['key']}` | {len(group)} | " + " | ".join(fmt(float(np.quantile(scores, q)), 2) for q in (0.05, 0.25, 0.5, 0.75, 0.95)) + f" | {fmt(float(scores.mean()), 2)} |")

    out += ["", "## The draft gate", "", "| Footage | Setting | Shots within 0.10 (≥ 0.90) | Flag ≥ 0.10 precision (≥ 0.90) | Recall (≥ 0.90) | Unflagged shots flagged (live action: ≤ 0.02) | Gate |", "|---|---|---|---|---|---|---|"]
    for footage in FOOTAGE:
        group = pick(footage, "check")
        for name in ([chosen[footage]] if footage in chosen else list(dict.fromkeys(chosen.values()))):
            p, rec, base, spec, counts = flag(group, name, "default", FLAGS[0])
            wrongly = ratio(counts[1], counts[1] + counts[3])
            if footage == "live":
                passed = wrongly <= 0.02
            else:
                passed = near(group, name) >= 0.90 and p >= 0.90 and rec >= 0.90
            out.append(
                f"| {footage} | {name} | {fmt(near(group, name))} | {fmt(p, 4)} ({counts[0]} of {counts[0] + counts[1]}) | {fmt(rec, 4)} ({counts[0]} of {counts[0] + counts[2]}) | "
                f"{fmt(wrongly)} ({counts[1]} of {counts[1] + counts[3]}) | {'pass' if passed else 'fail'} |"
            )

    (bench / "tables.md").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    # One row per scored shot: enough to redo every table that is not per frame.
    # (The settings of C2 are left out unless one was chosen: `compare` works them out again.)
    kept = [name for name in names if name.startswith("C1") or name in chosen.values()]
    head = ["key", "start", "end", "motion"] + [f"ref {r}" for r in SETTINGS] + [f"cad {r}" for r in SETTINGS] + kept + [f"cad {n}" for n in dict.fromkeys(chosen.values())]
    lines = ["\t".join(h.replace(" ", "_") for h in head)]
    for s in shots:
        lines.append("\t".join(str(s[h]) if isinstance(s[h], (str, int)) else f"{s[h]:.3f}" for h in head))
    (bench / "shots.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    (bench / "chosen.tsv").write_text("footage\tsetting\n" + "".join(f"{f}\t{n}\n" for f, n in chosen.items()), encoding="utf-8", newline="\n")
    print("\n".join(out))


# -- the eye check ------------------------------------------------------------


def chosen_of(bench):
    lines = (Path(bench) / "chosen.tsv").read_text(encoding="utf-8").splitlines()[1:]
    return dict(line.split("\t") for line in lines)


def draw(bench, which):
    """The pairs of the eye check: `(footage, stratum, key, frame)`, the frame being compared with the one before."""
    rng = random.Random(SEED if which == "reference" else SEED + 1)
    rules = dict(candidates())
    pools = {}
    for row in episodes(bench):
        d, frames, ref, segs = load_episode(bench, row)
        if frames != len(d):
            continue
        inside = np.zeros(frames, dtype=bool)  # frames of scored shots, the first of each left out
        held = {}
        for a, b in segs:
            if b - a >= MIN_FRAMES:
                inside[a + 1 : b] = True
                if which == "errors":
                    names = [chosen_of(bench)[row["footage"]]] if row["footage"] in chosen_of(bench) else list(dict.fromkeys(chosen_of(bench).values()))
                    for name in names:
                        held.setdefault(name, np.zeros(frames, dtype=bool))[a + 1 : b] = rules[name](d[a + 1 : b, 0])
        if which == "reference":
            strata = {
                "dropped by strict": ref["strict"],
                "dropped by default, kept by strict": ref["default"] & ~ref["strict"],
                "dropped by loose, kept by default": ref["loose"] & ~ref["default"],
                "kept by all three": ~(ref["strict"] | ref["default"] | ref["loose"]),
            }
        else:
            strata = {}
            for name, mask in held.items():
                strata[f"held to {name}, new to the reference"] = mask & ~ref["default"]
                strata[f"new to {name}, a repeat to the reference"] = ~mask & ref["default"]
        for stratum, mask in strata.items():
            pools.setdefault((row["footage"], stratum), []).extend((row["key"], int(f)) for f in np.flatnonzero(mask & inside))
    per = {"anime": 12, "3d": 6, "live": 6}
    out = []
    for (footage, stratum), pool in sorted(pools.items()):
        for key, frame in rng.sample(pool, min(per[footage], len(pool))):
            out.append((footage, stratum, key, frame, len(pool)))
    return out


def pairs(bench, ffmpeg, work, which):
    from PIL import Image

    bench, work = Path(bench), Path(work) / which
    work.mkdir(parents=True, exist_ok=True)
    drawn = draw(bench, which)
    order = list(range(len(drawn)))
    random.Random(SEED + 7).shuffle(order)  # the page numbers say nothing about the stratum
    page_of = {i: n + 1 for n, i in enumerate(order)}
    rows_of = {row["key"]: row for row in episodes(bench)}
    by_key = {}
    for i, (_, _, key, frame, _) in enumerate(drawn):
        by_key.setdefault(key, []).append((frame, i))
    for key, wanted in by_key.items():
        frames = sorted({f for frame, _ in wanted for f in (frame - 1, frame)})
        raw = work / f"raw_{key}"
        raw.mkdir(exist_ok=True)
        select = "+".join(f"eq(n\\,{f})" for f in frames)
        cmd = [ffmpeg, "-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i", video_of(rows_of[key]), "-an", "-sn", "-dn",
               "-vf", f"select='{select}'", "-fps_mode", "passthrough", "-pix_fmt", "rgb24", str(raw / "%04d.png")]
        subprocess.run(cmd, check=True)
        files = sorted(raw.glob("*.png"))
        assert len(files) == len(frames), f"{key}: {len(files)} pictures for {len(frames)} frames"
        picture = {f: files[n] for n, f in enumerate(frames)}
        for frame, i in wanted:
            before, after = (np.asarray(Image.open(picture[f]).convert("RGB"), dtype=np.int16) for f in (frame - 1, frame))
            render(before, after, page_of[i]).save(work / f"page_{page_of[i]:03d}.png")
        print(f"{key}: {len(wanted)} pairs", flush=True)
    lines = ["page\tfootage\tstratum\tkey\tframe\tpool"]
    for i, (footage, stratum, key, frame, pool) in enumerate(drawn):
        lines.append(f"{page_of[i]}\t{footage}\t{stratum}\t{key}\t{frame}\t{pool}")
    lines[1:] = sorted(lines[1:], key=lambda line: int(line.split("\t")[0]))
    (bench / f"eye.{which}.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(drawn)} pages in {work}")


def render(before, after, number):
    """One page: the frame before, the frame, their difference eight times stronger; below, the 256 px
    patch where they differ most, twice as large, the same three ways."""
    from PIL import Image, ImageDraw

    diff = np.abs(after - before)
    strong = np.clip(diff * 8, 0, 255).astype(np.uint8)
    h, w = diff.shape[:2]
    # Where the pictures differ most: the 32 px cell with the largest summed difference.
    cell = 32
    grid = diff.sum(axis=2)[: h // cell * cell, : w // cell * cell].reshape(h // cell, cell, w // cell, cell).sum(axis=(1, 3))
    gy, gx = np.unravel_index(int(np.argmax(grid)), grid.shape)
    side = min(256, h, w)
    top = int(np.clip(gy * cell + cell // 2 - side // 2, 0, h - side))
    left = int(np.clip(gx * cell + cell // 2 - side // 2, 0, w - side))
    small = (512, round(512 * h / w))

    def whole(a, keep_peaks=False):
        image = Image.fromarray(a.astype(np.uint8))
        if keep_peaks:  # so a small difference is not averaged away: the largest value of each block
            k = max(1, w // small[0])
            trimmed = a[: h // k * k, : w // k * k].reshape(h // k, k, w // k, k, 3).max(axis=(1, 3))
            image = Image.fromarray(trimmed.astype(np.uint8))
        return image.resize(small, Image.BILINEAR)

    def patch(a):
        return Image.fromarray(a[top : top + side, left : left + side].astype(np.uint8)).resize((512, 512), Image.NEAREST)

    page = Image.new("RGB", (512 * 3 + 16, small[1] + 512 + 44), (24, 24, 24))
    draw_on = ImageDraw.Draw(page)
    draw_on.text((4, 2), f"page {number}    top: frame before | frame | difference x8        bottom: the patch that differs most (2x), the same three", fill=(230, 230, 230))
    for k, a in enumerate((before, after)):
        image = whole(a)
        ImageDraw.Draw(image).rectangle([left * small[0] / w, top * small[1] / h, (left + side) * small[0] / w, (top + side) * small[1] / h], outline=(255, 255, 0))
        page.paste(image, (k * 520, 18))
        page.paste(patch(a), (k * 520, small[1] + 26))
    page.paste(whole(strong, keep_peaks=True), (2 * 520, 18))
    page.paste(patch(strong), (2 * 520, small[1] + 26))
    return page


def read(bench, which, answers):
    key = {}
    lines = (Path(bench) / f"eye.{which}.tsv").read_text(encoding="utf-8").splitlines()
    head = lines[0].split("\t")[:6]  # the key's own columns; answers joined before are written anew
    for line in lines[1:]:
        cells = dict(zip(head, line.split("\t")))
        key[int(cells["page"])] = cells
    readers = []
    for paths in answers:  # one reader's files, joined by `+` when the pages were read in parts
        got = {}
        for path in paths.split("+"):
            for line in Path(path).read_text(encoding="utf-8").splitlines():
                cells = line.split("\t")
                if cells and cells[0].strip().isdigit():
                    got[int(cells[0])] = (cells[1].strip().lower(), cells[2].strip() if len(cells) > 2 else "")
        missing = sorted(set(key) - set(got))
        assert not missing, f"{paths}: no answer for pages {missing}"
        readers.append(got)
    out = ["\t".join(head) + "".join(f"\treader_{n + 1}\tnote_{n + 1}" for n in range(len(readers)))]
    counts = {}
    for page in sorted(key):
        cells = key[page]
        verdicts = [r.get(page, ("", "")) for r in readers]
        out.append("\t".join(cells[h] for h in head) + "".join(f"\t{v}\t{note}" for v, note in verdicts))
        for n, (v, _) in enumerate(verdicts):
            counts.setdefault((cells["footage"], cells["stratum"]), {}).setdefault(n, {}).setdefault(v, 0)
            counts[(cells["footage"], cells["stratum"])][n][v] += 1
    (Path(bench) / f"eye.{which}.tsv").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    print("| Footage | Stratum | " + " | ".join(f"Reader {n + 1}: repeat / changed / cannot tell" for n in range(len(readers))) + " |")
    print("|---|---|" + "---|" * len(readers))
    for (footage, stratum), by_reader in sorted(counts.items()):
        cells = [" / ".join(str(by_reader.get(n, {}).get(v, 0)) for v in ("repeat", "changed", "cannot tell")) for n in range(len(readers))]
        print(f"| {footage} | {stratum} | " + " | ".join(cells) + " |")


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    a = sys.argv[1:]
    tool = lambda path: str(Path(path).resolve())  # Windows starts no program from a relative path with slashes
    if len(a) >= 3 and a[0] == "reference":
        reference(a[1], tool(a[2]), a[3:])
    elif len(a) == 2 and a[0] == "compare":
        compare(a[1])
    elif len(a) == 5 and a[0] == "pairs" and a[4] in ("reference", "errors"):
        pairs(a[1], tool(a[2]), a[3], a[4])
    elif len(a) >= 4 and a[0] == "read" and a[2] in ("reference", "errors"):
        read(a[1], a[2], a[3:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
