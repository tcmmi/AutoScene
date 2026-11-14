"""Recipe A, second stage, the data: pairs of heads from the tracks an anime pass found in footage that trains
(`bench/models/README.md`, "Track triplets"; row 19 of `DECISIONS.md` says why each rule is there). No names
are needed: two heads of one track are one character (the positive), a head of a track seen in the same
samples is another one (the negative).

    uv run triplets.py build <tracks dir> <pairs dir> --titles <order.tsv> --fuse <fuse_signals.exe> [--first <n>]
                             [--drop <table>[,<table>…]] [--partial] [--kind h6] [--apart 1.0] [--held 8.0]
                             [--slip 50.0] [--shape 1.3] [--cut 0.1] [--together 2] [--cover 0.2] [--alike 0.5]
                             [--dev 0.1]
        `<order.tsv>` lists the footage (`n id seconds sha256_of_id channel title`); the titles with `n` below
        `--first` are taken (all without it), and every one of them must be mined (`--partial`: the ones that
        are). `<tracks dir>` holds, per title, `in/<title>/bb.info.json` and `bb.sig` (the shot signals) and
        the outputs of `anime_crops` (`crops/<title>.<kind>.crops`, `.crops.tsv`, `.pairs.tsv`; run with
        `HEADS=` and `APART=` a second in frames). Written to `<pairs dir>`:
        `positives.tsv`  two heads of one track at least `--apart` seconds apart, without the pairs that
                         teach nothing or may be two characters: one held drawing (the pixels of the two
                         crops differ by less than `--held` of 255 on average), a shot change between the
                         two (a frame between them, three from either end left out, whose shot scores reach
                         `--cut`: a track that went on through a cut or a dissolve), and a box that slipped
                         onto another drawing (the pixels differ by `--slip` or more, or the box's shape, its
                         width over its height, changed by a factor of `--shape` or more: a head keeps the
                         shape of its box over a second or two; row 20 of `DECISIONS.md`);
        `negatives.tsv`  two tracks seen together in at least `--together` samples, without the pairs that
                         may be one character: a second box on one head (the smaller box covered by more
                         than `--cover`), and a pair the pass's own embedder (CCIP) calls alike (cosine above
                         `--alike`: mirror images, clones);
        `titles.tsv`     per title its show (a key made from its name: trailers of one show share it), its
                         side (`train`, or `dev` for a share of `--dev` of the shows, by the key alone), its
                         counts, and what each rule took.
        The shot signals are read through `fuse_signals --tsv` and kept as `in/<title>/bb.raw.tsv`.
        `--drop` names tables whose first column lists titles that never train (real people, openings and
        endings, bench characters found by `leaks`); a name that is no title of the list is refused.

    uv run triplets.py look <tracks dir> <pairs dir> <sheets dir>
        Every title on a row, eight of its heads from as many tracks: to see which titles are not drawn
        animation at all.

    uv run triplets.py leaks <tracks dir> <pairs dir> <checkpoint.pt> <out.tsv> <project>=<tracks.emb>,<names.tsv> …
        Bench characters in the footage, by how they look: every head through the embedder, and per track its
        head most like a bench figure's track (`<tracks.emb>`: the embedder's vectors of a tuning project's
        tracks, `embed.py`; `<names.tsv>`: their labels, as `leaks.py near` reads them). `<out.tsv>` has every
        track, most alike first; the sheets beside it (`<out>.NN.jpg`) show every title's most alike head,
        most alike title first, sixty a sheet.

    uv run triplets.py sheets <tracks dir> <pairs dir> <sheets dir> <key.tsv> [--pairs 300] [--seed 7]
                              [--media <dir of <title>.mp4> --ffmpeg <ffmpeg.exe>]
        The check of the pairs (at most 2 % wrong among 300): half positives, half negatives, drawn as
        training draws them, shuffled, ten a sheet (`sheet_NN.jpg`). `<key.tsv>` says what each is and belongs
        somewhere the readers don't look. A reader answers per pair `s` (one character), `d` (two) or `?`
        (cannot be told). With `--media`, each pair also shows the two frames its heads were cut from, the
        left head's box in yellow and the right one's in cyan (six pairs a sheet): two crops alone do not
        show that two heads alike stood in one picture as two figures (row 20).

    uv run triplets.py audit <key.tsv> <answers.tsv> [<answers.tsv> …]
        The readers' answers (`pair answer` per line; a later reader may answer some pairs only) against the
        key: a positive is wrong when it shows two characters, a negative when it shows one. A pair is wrong
        when more of the readers who could tell say so than not, and split when as many say so as not.
"""
import hashlib
import json
import math
import random
import re
import sys
import unicodedata
from pathlib import Path

import numpy as np

import crops

KIND = "h6"
# Frames at either end of a positive pair that the shot-change rule leaves out.
EDGE = 3


def order_of(path, first=None):
    """The footage list: `[(n, title id, name)]`, the rows with `n` below `first` (all without it)."""
    rows = read_table(path)
    return [(int(r["n"]), r["id"], r["title"]) for r in rows if first is None or int(r["n"]) < first]


def complete(root, kind, title):
    """`anime_crops` is done for the title: its three files are there."""
    base = Path(root) / "crops" / f"{title}.{kind}"
    return all(Path(str(base) + end).exists() for end in (".crops", ".crops.tsv", ".pairs.tsv"))


def fps_of(root, title):
    rate = json.loads((Path(root) / "in" / title / "bb.info.json").read_text(encoding="utf-8"))["video"]["rFrameRate"]
    return rate["num"] / rate["den"]


WORDS = r"official|trailer|teaser|pv|cm|promo|promotion|preview|clip|opening|ending|theme|season|part|cour|episode|ep|dub|sub|english|web|tv|anime|アニメ|予告|本予告|特報|ティザー|第"


def show_key(name):
    """What a trailer's name says the show is, as far as a rule can tell: the text in the first Japanese title
    brackets, or the name up to its first separator or trailer word; letters only. Trailers of one show should
    share it (they must not sit on both sides); two shows sharing one only costs a little of either side."""
    t = unicodedata.normalize("NFKC", name).lower()
    quoted = re.search(r"[『「]([^』」]+)[』」]", t)
    if quoted:
        t = quoted.group(1)
    else:
        t = re.sub(r"^(\s*[\[(【<][^\])】>]*[\])】>])+", "", t)
        t = re.split(r"\s[|/:\-–—~]\s|[|｜【\[(]|\b(?:" + WORDS + r")\b", t)[0]
    t = re.sub(r"(?:season|part|cour|第)\s*\d+\S*|\d+(?:st|nd|rd|th)\b|\b[ivx]+\b", " ", t)
    key = "".join(c for c in t if c.isalpha())
    return key[:24] or "".join(c for c in unicodedata.normalize("NFKC", name).lower() if c.isalnum())[:24]


def side(show, dev):
    """`dev` or `train`, from the show's key alone: more titles move none from one side to the other."""
    place = int.from_bytes(hashlib.sha256(f"stage2:{show}".encode()).digest()[:8], "big")
    return "dev" if place % 10_000 < dev * 10_000 else "train"


def read_table(path):
    with open(path, encoding="utf-8") as f:
        names = f.readline().rstrip("\n").split("\t")
        return [dict(zip(names, line.rstrip("\n").split("\t"))) for line in f if line.strip()]


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 22), b""):
            h.update(block)
    return h.hexdigest()


def tables_of(out):
    """The pair tables' hashes: what a run was drawn from, to tell a rebuild from the tables it began with."""
    return {name: file_sha256(Path(out) / name) for name in ("positives.tsv", "negatives.tsv", "titles.tsv")}


def shot_scores(root, title, fuse):
    """Per frame, the greater of the shot detector's two scores (`fuse_signals <sig> --tsv 0 <all>`: `f
    single many …`); the dump is kept beside the signals."""
    import subprocess

    raw = Path(root) / "in" / title / "bb.raw.tsv"
    if not raw.exists():
        done = subprocess.run([fuse, str(raw.with_name("bb.sig")), "--anime", "--tsv", "0", "1000000000"], capture_output=True, text=True)
        assert done.returncode == 0 and done.stdout.startswith("f\tsingle\tmany"), (title, done.stderr[-500:])
        raw.write_text(done.stdout, encoding="utf-8", newline="\n")
    lines = raw.read_text(encoding="utf-8").splitlines()[1:]
    table = np.array([[float(v) for v in line.split("\t")[:3]] for line in lines], dtype=np.float32).reshape(-1, 3)
    assert (table[:, 0] == np.arange(len(table))).all(), f"{raw}: frames out of order"
    return np.maximum(table[:, 1], table[:, 2])


def box_shape(row):
    """A head box's width over its height, in the frame's own units (only ratios of two of them are used, and
    the frame's own shape drops out of those)."""
    return (float(row["x2"]) - float(row["x1"])) / max(float(row["y2"]) - float(row["y1"]), 1e-9)


def build(root, out, listing, first, partial, kind, apart, held, slip, cut, together, cover, alike, dev, drop, fuse, shape=1.3):
    root, out = Path(root), Path(out)
    out.mkdir(parents=True, exist_ok=True)
    everything = {title for _, title, _ in order_of(listing)}
    dropped = {}
    for table in (drop or "").split(","):
        if table:
            for line in Path(table).read_text(encoding="utf-8").splitlines():
                cell = line.split("\t")
                if cell[0].strip() and not line.startswith("#") and cell[0] != "title":
                    assert cell[0] in everything, f"{table}: {cell[0]!r} is no title of {listing}"
                    dropped[cell[0]] = cell[1] if len(cell) > 1 else ""
    wanted = [(title, name) for _, title, name in order_of(listing, first) if title not in dropped]
    missing = {title for title, _ in wanted if not complete(root, kind, title)}
    if missing and not partial:
        sys.exit(f"{len(missing)} of the {len(wanted)} titles are not mined yet (some: {sorted(missing)[:5]}); --partial takes the ones that are")
    wanted = [(title, name) for title, name in wanted if title not in missing]
    pos = ["title\ttrack\ta\tb\tframes\tdifference"]
    neg = ["title\ta\tb\tsamples\tiou\tcover\tcosine"]
    table = ["title\tshow\tside\ttracks\tcrops\tpositives\tnegatives\tanchors\tnear\theld\tcut\tslip\tbrief\tone_head\talike\tno_crop\tshape"]
    for title, name in wanted:
        prefix = str(root / "crops" / f"{title}.{kind}")
        tracks, track, _, pixels, rows = crops.read(prefix)
        least = apart * fps_of(root, title) - 0.5
        change = shot_scores(root, title, fuse) if len(rows) else np.zeros(0, dtype=np.float32)
        by_track = {}
        for i, t in enumerate(track.tolist()):
            by_track.setdefault(t, []).append(i)
        n_pos = n_near = n_held = n_cut = n_slip = n_shape = 0
        with_pos = set()
        for t, mine in by_track.items():
            for x in range(len(mine)):
                for y in range(x + 1, len(mine)):
                    a, b = mine[x], mine[y]
                    fa, fb = sorted((int(rows[a]["frame"]), int(rows[b]["frame"])))
                    frames = fb - fa
                    if frames < least:
                        n_near += 1
                        continue
                    difference = float(np.abs(pixels[a].astype(np.int16) - pixels[b].astype(np.int16)).mean())
                    if difference < held:
                        n_held += 1
                        continue
                    # The detector's score for a change sits on a few frames around it: the three frames at
                    # either end are the shot's own first and last, where a whole-shot track begins and ends.
                    if float(change[fa + EDGE : fb + 1 - EDGE].max()) >= cut:
                        n_cut += 1
                        continue
                    if difference >= slip:
                        n_slip += 1
                        continue
                    # A box that went over to a neighbour, a body or a thing mostly has another shape there.
                    turned = box_shape(rows[a]) / box_shape(rows[b])
                    if max(turned, 1 / max(turned, 1e-9)) >= shape:
                        n_shape += 1
                        continue
                    pos.append(f"{title}\t{t}\t{a}\t{b}\t{frames}\t{difference:.2f}")
                    with_pos.add(t)
                    n_pos += 1
        n_neg = n_brief = n_one = n_alike = n_none = 0
        anchors = set()
        for r in read_table(prefix + ".pairs.tsv"):
            a, b = int(r["a"]), int(r["b"])
            if a not in by_track or b not in by_track:
                n_none += 1
            elif int(r["samples"]) < together:
                n_brief += 1
            elif float(r["cover"]) > cover:
                n_one += 1
            elif not float(r["cosine"]) <= alike:  # a pair without a cosine goes too
                n_alike += 1
            else:
                neg.append(f"{title}\t{a}\t{b}\t{r['samples']}\t{r['iou']}\t{r['cover']}\t{r['cosine']}")
                n_neg += 1
                anchors |= {a, b} & with_pos
        show = show_key(name)
        table.append(f"{title}\t{show}\t{side(show, dev)}\t{tracks}\t{len(rows)}\t{n_pos}\t{n_neg}\t{len(anchors)}\t{n_near}\t{n_held}\t{n_cut}\t{n_slip}"
                     f"\t{n_brief}\t{n_one}\t{n_alike}\t{n_none}\t{n_shape}")
    for name, lines in (("positives.tsv", pos), ("negatives.tsv", neg), ("titles.tsv", table)):
        (out / name).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    settings = {"kind": kind, "apart": apart, "held": held, "slip": slip, "shape": shape, "cut": cut, "together": together, "cover": cover, "alike": alike,
                "dev": dev,
                "titles": {"list": Path(listing).name, "sha256": file_sha256(listing), "first": first, "taken": len(wanted), "not_mined": len(missing)},
                "dropped": dropped}
    (out / "settings.json").write_text(json.dumps(settings, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    cells = [line.split("\t") for line in table[1:]]
    for which in ("train", "dev"):
        got = [c for c in cells if c[2] == which]
        total = lambda k: sum(int(c[k]) for c in got)
        print(f"{which}: {len(got)} titles of {len({c[1] for c in got})} shows ({sum(1 for c in got if int(c[7]))} with a triplet), {total(3)} tracks, "
              f"{total(4)} crops, {total(5)} positives, {total(6)} negatives, {total(7)} anchor tracks; positives taken: {total(8)} too near, "
              f"{total(9)} held, {total(10)} over a shot change, {total(11)} slipped, {total(16)} with a box of another shape; "
              f"negatives taken: {total(12)} together too briefly, "
              f"{total(13)} one head, {total(14)} alike, {total(15)} without a crop")
    print(f"{len(dropped)} titles of the list never train (--drop); {len(missing)} not mined")


class Pairs:
    """The pairs of one side, and how a triplet is drawn from them: a title (one with more anchor tracks more
    often, by the square root of their number: a crowded title doesn't lead, and one with a single anchor
    isn't shown over and over), one of its tracks that has a positive and a neighbour (every such track
    alike), a positive of it in either order, a neighbour, and any head of that neighbour."""

    def __init__(self, out, which):
        out = Path(out)
        self.kind = json.loads((out / "settings.json").read_text(encoding="utf-8"))["kind"]
        mine = {r["title"] for r in read_table(out / "titles.tsv") if r["side"] == which}
        self.positives = {}
        for r in read_table(out / "positives.tsv"):
            if r["title"] in mine:
                self.positives.setdefault((r["title"], int(r["track"])), []).append((int(r["a"]), int(r["b"])))
        self.neighbours = {}
        for r in read_table(out / "negatives.tsv"):
            if r["title"] in mine:
                for a, b in ((int(r["a"]), int(r["b"])), (int(r["b"]), int(r["a"]))):
                    if (r["title"], a) in self.positives:
                        self.neighbours.setdefault(r["title"], {}).setdefault(a, []).append(b)
        self.titles = sorted(self.neighbours)
        self.anchors = {title: sorted(self.neighbours[title]) for title in self.titles}
        self.weights = [math.sqrt(len(self.anchors[title])) for title in self.titles]
        self.heads = {}

    def load_heads(self, root):
        """Every track's crops, per title (the negative is any head of its track)."""
        for title in self.titles:
            _, track, _, _, _ = crops.read(str(Path(root) / "crops" / f"{title}.{self.kind}"))
            by_track = {}
            for i, t in enumerate(track.tolist()):
                by_track.setdefault(t, []).append(i)
            self.heads[title] = by_track

    def draw(self, rng):
        """`(title, anchor crop, positive crop, negative crop, anchor track, negative track)`."""
        title = rng.choices(self.titles, self.weights)[0]
        anchor = rng.choice(self.anchors[title])
        other = rng.choice(self.neighbours[title][anchor])
        a, p = rng.choice(self.positives[(title, anchor)])
        if rng.random() < 0.5:
            a, p = p, a
        return title, a, p, rng.choice(self.heads[title][other]), anchor, other


def pixels_of(root, kind, cache, title):
    if title not in cache:
        cache[title] = crops.read(str(Path(root) / "crops" / f"{title}.{kind}"))[3]
    return cache[title]


def look(root, out, where):
    from PIL import Image, ImageDraw, ImageFont

    where = Path(where)
    where.mkdir(parents=True, exist_ok=True)
    kind = json.loads((Path(out) / "settings.json").read_text(encoding="utf-8"))["kind"]
    titles = [r["title"] for r in read_table(Path(out) / "titles.tsv")]
    cell, heads, rows_per, font = 80, 8, 20, ImageFont.load_default(size=14)
    wide = 110 + heads * cell + 10
    index = ["n\tsheet\ttitle"]
    for s in range(0, len(titles), 2 * rows_per):
        page = Image.new("RGB", (2 * wide, rows_per * (cell + 4)), "black")
        draw = ImageDraw.Draw(page)
        for k, title in enumerate(titles[s : s + 2 * rows_per]):
            x, y = (k // rows_per) * wide, (k % rows_per) * (cell + 4)
            draw.text((x + 2, y + 2), f"#{s + k}", fill="yellow", font=font)
            draw.text((x + 2, y + 22), title[:11], fill="white", font=font)
            _, track, _, pixels, _ = crops.read(str(Path(root) / "crops" / f"{title}.{kind}"))
            first = {}
            for i, t in enumerate(track.tolist()):
                first.setdefault(t, i)
            picked = list(first.values())
            picked = [picked[int(j)] for j in np.linspace(0, len(picked) - 1, min(heads, len(picked)))] if picked else []
            for j, i in enumerate(picked):
                page.paste(Image.fromarray(np.asarray(pixels[i])).resize((cell, cell), Image.BILINEAR), (x + 110 + j * cell, y))
            index.append(f"{s + k}\t{s // (2 * rows_per):02d}\t{title}")
        page.save(where / f"look_{s // (2 * rows_per):02d}.jpg", quality=88)
    (where / "index.tsv").write_text("\n".join(index) + "\n", encoding="utf-8", newline="\n")
    print(f"{where}: {len(titles)} titles on {(len(titles) + 2 * rows_per - 1) // (2 * rows_per)} sheets")


def frames_of(media, ffmpeg, title, wanted, height):
    """Frames of `<media>/<title>.mp4` by their numbers as the pass counts them, scaled to `height`: one
    decode of the file, `{number: picture}`."""
    import subprocess
    import tempfile

    from PIL import Image

    wanted = sorted(set(wanted))
    pick = "+".join(f"eq(n\\,{n})" for n in wanted)
    with tempfile.TemporaryDirectory() as tmp:
        done = subprocess.run([ffmpeg, "-v", "error", "-i", str(Path(media) / f"{title}.mp4"), "-vf", f"select={pick},scale=-2:{height}",
                               "-fps_mode", "passthrough", "-q:v", "3", str(Path(tmp) / "%05d.jpg")], capture_output=True, text=True)
        files = sorted(Path(tmp).glob("*.jpg"))
        assert done.returncode == 0 and len(files) == len(wanted), (title, len(files), len(wanted), done.stderr[-500:])
        return {n: Image.open(f).convert("RGB") for n, f in zip(wanted, files)}


def sheets_with_frames(root, kind, items, where, key_path, media, ffmpeg):
    """The check's sheets with the frames beside the crops: per pair the two heads, then the frame of each
    with that head's box (the left head's yellow, the right one's cyan; one box a frame, whatever the pair
    is, so that the boxes do not say which kind it is)."""
    from PIL import Image, ImageDraw, ImageFont

    side_px, gap, label, per_sheet, wide = crops.CROP, 12, 34, 6, 400
    font = ImageFont.load_default(size=26)
    tables, wanted = {}, {}
    for _, title, a, b, _, _ in items:
        if title not in tables:
            tables[title] = crops.read(str(Path(root) / "crops" / f"{title}.{kind}"))
        rows = tables[title][4]
        wanted.setdefault(title, set()).update((int(rows[a]["frame"]), int(rows[b]["frame"])))
    frames = {}
    for done, (title, numbers) in enumerate(sorted(wanted.items())):
        frames[title] = frames_of(media, ffmpeg, title, numbers, side_px)
        if done % 25 == 0:
            print(f"frames of {done} of {len(wanted)} titles", flush=True)
    key = ["pair\tsheet\tkind\ttitle\tcrop_a\tcrop_b\ttrack_a\ttrack_b"]
    for s in range(0, len(items), per_sheet):
        page = Image.new("RGB", (2 * side_px + 2 * wide + 3 * gap, per_sheet * (side_px + label + gap)), "black")
        draw = ImageDraw.Draw(page)
        for k, (what, title, a, b, ta, tb) in enumerate(items[s : s + per_sheet]):
            y = k * (side_px + label + gap)
            draw.text((4, y + 2), f"pair {s + k}", fill="yellow", font=font)
            pixels, rows = tables[title][3], tables[title][4]
            page.paste(Image.fromarray(np.asarray(pixels[a])), (0, y + label))
            page.paste(Image.fromarray(np.asarray(pixels[b])), (side_px + 2, y + label))
            for j, (crop, colour) in enumerate(((a, "yellow"), (b, "cyan"))):
                frame = frames[title][int(rows[crop]["frame"])].copy()
                w, h = frame.size
                box = [float(rows[crop][c]) for c in ("x1", "y1", "x2", "y2")]
                ImageDraw.Draw(frame).rectangle([box[0] * w - 2, box[1] * h - 2, box[2] * w + 2, box[3] * h + 2], outline=colour, width=2)
                page.paste(frame.crop((0, 0, min(w, wide), h)), (2 * side_px + gap + j * (wide + gap), y + label))
            key.append(f"{s + k}\t{s // per_sheet:02d}\t{what}\t{title}\t{a}\t{b}\t{ta}\t{tb}")
        page.save(where / f"sheet_{s // per_sheet:02d}.jpg", quality=90)
    Path(key_path).write_text("\n".join(key) + "\n", encoding="utf-8", newline="\n")
    print(f"{where}: {len(items)} pairs with their frames on {(len(items) + per_sheet - 1) // per_sheet} sheets; the key is {key_path}")


def sheets(root, out, where, key_path, pairs, seed, media=None, ffmpeg=None):
    from PIL import Image, ImageDraw, ImageFont

    where = Path(where)
    where.mkdir(parents=True, exist_ok=True)
    assert Path(key_path).resolve().parent != where.resolve(), "the key belongs somewhere the readers don't look"
    source = Pairs(out, "train")
    source.load_heads(root)
    rng = random.Random(seed)
    seen, items = set(), []
    while len(items) < pairs:
        title, a, p, n, anchor, other = source.draw(rng)
        positive = len(items) % 2 == 0
        one = (title, min(a, p), max(a, p)) if positive else (title, min(a, n), max(a, n))
        if one in seen:
            continue
        seen.add(one)
        items.append(("positive", title, a, p, anchor, anchor) if positive else ("negative", title, a, n, anchor, other))
    rng.shuffle(items)
    if media:
        return sheets_with_frames(root, source.kind, items, where, key_path, media, ffmpeg)
    cache, font = {}, ImageFont.load_default(size=26)
    side_px, gap, label = crops.CROP, 12, 34
    key = ["pair\tsheet\tkind\ttitle\tcrop_a\tcrop_b\ttrack_a\ttrack_b"]
    for s in range(0, len(items), 10):
        page = Image.new("RGB", (2 * (2 * side_px + gap) + gap, 5 * (side_px + label + gap)), "black")
        draw = ImageDraw.Draw(page)
        for k, (what, title, a, b, ta, tb) in enumerate(items[s : s + 10]):
            x, y = (k % 2) * (2 * side_px + 2 * gap), (k // 2) * (side_px + label + gap)
            draw.text((x + 4, y + 2), f"pair {s + k}", fill="yellow", font=font)
            px = pixels_of(root, source.kind, cache, title)
            page.paste(Image.fromarray(np.asarray(px[a])), (x, y + label))
            page.paste(Image.fromarray(np.asarray(px[b])), (x + side_px + 2, y + label))
            key.append(f"{s + k}\t{s // 10:02d}\t{what}\t{title}\t{a}\t{b}\t{ta}\t{tb}")
        page.save(where / f"sheet_{s // 10:02d}.jpg", quality=90)
    Path(key_path).write_text("\n".join(key) + "\n", encoding="utf-8", newline="\n")
    print(f"{where}: {len(items)} pairs on {(len(items) + 9) // 10} sheets; the key is {key_path}")


def audit(key_path, answer_files):
    key = {int(r["pair"]): r["kind"] for r in read_table(key_path)}
    votes = {n: [] for n in key}
    for path in answer_files:
        for line in Path(path).read_text(encoding="utf-8").splitlines():
            part = line.replace(",", " ").split()
            if len(part) >= 2 and part[0].isdigit() and part[1] in ("s", "d", "?") and int(part[0]) in key:
                votes[int(part[0])].append(part[1])
    few = sorted(n for n in key if len(votes[n]) < min(2, len(answer_files)))
    assert not few, f"pairs with fewer answers than readers asked: {few[:10]}"
    wrong_answer = {"positive": "d", "negative": "s"}
    count = {"positive": [0, 0, 0, 0], "negative": [0, 0, 0, 0]}  # pairs, wrong, split, cannot be told
    listed = []
    for n in sorted(key):
        kind, told = key[n], [v for v in votes[n] if v != "?"]
        bad = sum(v == wrong_answer[kind] for v in told)
        count[kind][0] += 1
        if not told:
            count[kind][3] += 1
            listed.append(f"{n}\t{kind}\t{''.join(votes[n])}\tcannot be told")
        elif bad * 2 > len(told):
            count[kind][1] += 1
            listed.append(f"{n}\t{kind}\t{''.join(votes[n])}\twrong")
        elif bad * 2 == len(told):
            count[kind][2] += 1
            listed.append(f"{n}\t{kind}\t{''.join(votes[n])}\tsplit")
        elif bad:
            listed.append(f"{n}\t{kind}\t{''.join(votes[n])}\tright by most")
    total = sum(c[0] for c in count.values())
    wrong = sum(c[1] for c in count.values())
    split = sum(c[2] for c in count.values())
    for kind, c in count.items():
        print(f"{kind}: {c[0]} pairs, {c[1]} wrong, {c[2]} split, {c[3]} cannot be told")
    print(f"wrong {wrong} of {total} ({100 * wrong / total:.1f} %); with every split pair counted as wrong {100 * (wrong + split) / total:.1f} %")
    print("\n".join(listed))


def leaks(root, out, checkpoint, table, projects):
    import torch
    from PIL import Image, ImageDraw, ImageFont

    import leaks as bench
    import model

    root, out = Path(root), Path(out)
    kind = json.loads((out / "settings.json").read_text(encoding="utf-8"))["kind"]
    device = "cuda" if torch.cuda.is_available() else "cpu"
    net = model.from_checkpoint(checkpoint).to(device).eval()
    figures, vectors = [], []
    for spec in projects:
        project, files = spec.split("=", 1)
        emb, dump = files.split(",")
        tracks = crops.read_override(emb)
        labels = [line.split("\t")[4] for line in Path(dump).read_text(encoding="utf-8").splitlines() if line.strip()]
        assert len(labels) == len(tracks), (project, len(labels), len(tracks))
        gallery = [line.split("\t") for line in (bench.BENCH / f"{project}.gallery.tsv").read_text(encoding="utf-8").splitlines()[1:]]
        figure = {row[0]: row[1] for row in gallery if len(row) > 1}
        for i, label in enumerate(labels):
            label = figure.get(label, label)
            if label not in bench.NOBODY and np.linalg.norm(tracks[i]) > 0:
                figures.append(f"{project}:{label}")
                vectors.append(tracks[i] / np.linalg.norm(tracks[i]))
    bench_vectors = torch.from_numpy(np.stack(vectors)).to(device)
    found = []
    titles = [r["title"] for r in read_table(out / "titles.tsv")]
    for done, title in enumerate(titles):
        _, track, _, pixels, _ = crops.read(str(root / "crops" / f"{title}.{kind}"))
        if not len(pixels):
            continue
        parts = []
        with torch.no_grad():
            for i in range(0, len(pixels), 32):
                parts.append(torch.nn.functional.normalize(net(torch.from_numpy(crops.to_input(pixels[i : i + 32])).to(device)).float(), dim=1))
        # Head by head: a track that runs on through a cut has no one direction, its heads do.
        best = (torch.cat(parts) @ bench_vectors.t()).max(1)
        top = {}
        for i, t in enumerate(track.tolist()):
            if t not in top or float(best.values[i]) > top[t][0]:
                top[t] = (float(best.values[i]), i, figures[int(best.indices[i])])
        found += [(like, title, t, i, who) for t, (like, i, who) in top.items()]
        if done % 50 == 0:
            print(f"{done} of {len(titles)} titles", flush=True)
    found.sort(reverse=True)
    lines = ["likeness\ttitle\ttrack\tcrop\tfigure"] + [f"{l:.3f}\t{title}\t{t}\t{c}\t{who}" for l, title, t, c, who in found]
    Path(table).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    # Every title's most alike head, the most alike title first.
    per_title = {}
    for row in found:
        per_title.setdefault(row[1], row)
    ranked = list(per_title.values())
    cache, font = {}, ImageFont.load_default(size=15)
    for s in range(0, len(ranked), 60):
        page = Image.new("RGB", (10 * crops.CROP, 6 * (crops.CROP + 40)), "black")
        draw = ImageDraw.Draw(page)
        for k, (l, title, t, c, who) in enumerate(ranked[s : s + 60]):
            x, y = (k % 10) * crops.CROP, (k // 10) * (crops.CROP + 40)
            page.paste(Image.fromarray(np.asarray(pixels_of(root, kind, cache, title)[c])), (x, y + 40))
            draw.text((x + 2, y + 2), f"{l:.2f} {who.split(':', 1)[1][:22]}", fill="yellow", font=font)
            draw.text((x + 2, y + 20), f"{title} t{t}", fill="white", font=font)
        page.save(Path(table).with_suffix(f".{s // 60:02d}.jpg"), quality=90)
    bars = [0.5, 0.6, 0.7, 0.8]
    print(f"{table}: {len(found)} tracks of {len(ranked)} titles (of {len(titles)} in the tables); titles with a head at least as alike as "
          + ", ".join(f"{b}: {sum(1 for r in ranked if r[0] >= b)}" for b in bars))


def main():
    a = sys.argv[1:]
    if not a:
        sys.exit(__doc__)
    flag = lambda name, default: a[a.index(name) + 1] if name in a else default
    lone = ("--partial",)
    plain = [x for k, x in enumerate(a) if not x.startswith("--") and (k == 0 or not a[k - 1].startswith("--") or a[k - 1] in lone)]
    if plain[0] == "build" and len(plain) == 3 and "--fuse" in a and "--titles" in a:
        build(plain[1], plain[2], flag("--titles", None), int(flag("--first", 0)) or None, "--partial" in a, flag("--kind", KIND),
              float(flag("--apart", 1.0)), float(flag("--held", 8.0)), float(flag("--slip", 50.0)), float(flag("--cut", 0.1)),
              int(flag("--together", 2)), float(flag("--cover", 0.2)), float(flag("--alike", 0.5)), float(flag("--dev", 0.1)),
              flag("--drop", None), flag("--fuse", None), float(flag("--shape", 1.3)))
    elif plain[0] == "look" and len(plain) == 4:
        look(plain[1], plain[2], plain[3])
    elif plain[0] == "sheets" and len(plain) == 5:
        assert ("--media" in a) == ("--ffmpeg" in a), "--media and --ffmpeg go together"
        sheets(plain[1], plain[2], plain[3], plain[4], int(flag("--pairs", 300)), int(flag("--seed", 7)), flag("--media", None), flag("--ffmpeg", None))
    elif plain[0] == "audit" and len(plain) >= 3:
        audit(plain[1], plain[2:])
    elif plain[0] == "leaks" and len(plain) >= 6:
        leaks(plain[1], plain[2], plain[3], plain[4], plain[5:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
