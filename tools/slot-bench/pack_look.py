"""A look by eye at what a filter on "Whole scenes" would give (`bench/models/DECISIONS.md`, row 35).

    uv run pack_look.py drops <ffmpeg> <video> <project_copy_dir> <media> <out_dir>
    uv run pack_look.py pack <ffmpeg> <video> <project_copy_dir> <media> <detail.tsv> <out_prefix>
    uv run pack_look.py tally <project_copy_dir> <media> <reader_a.tsv> <reader_b.tsv> <detail.tsv> [...]

`drops` draws every shot in which the cast shows nobody, for readers who sort
them: a row per shot with three pictures of it (framed) between one picture
of the shot before and one of the shot after. Four rows per sheet
(`sheet_000.jpg`, …) and `rows.tsv` (`sheet shot start end`, frames).

`pack` takes what `DETAIL=<character> conversation_bench …` prints for one
character (every shot with the sets that hold it) and makes, for the owner to
look at: sheets of the character's whole scenes in order, one picture per
shot, framed green for an own shot, yellow for a shot the filter adds, and
dimmed with a red bar for a shot the filter leaves out
(`<out_prefix>.sheet_000.jpg`, …); and two videos cut from `<video>`,
`<out_prefix>.filter.mp4` and `<out_prefix>.scenes.mp4`, with a bar at the top
of the picture: green over an own shot, yellow over an added one, red over
one the filter would leave out (the last only in the second video). The
videos are for looking, not for measuring: a cut may sit a frame off.

`tally` counts, per character: what the filter leaves out of whole scenes,
the holes that makes (a left-out stretch between two kept shots of one found
scene), and how much of what is left out both readers call `talk`.

The video is decoded from frame 0; the run stops when the number of frames
is not the project's.
"""
import os
import sqlite3
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont


def project(folder, nth):
    """The media's shots `[(start, end)]`, whether the cast shows someone in each, frames and rate."""
    db = sqlite3.connect(f"file:{folder}/project.db?mode=ro", uri=True)
    mid, frames, fps = db.execute(
        "SELECT id, frames, CAST(fps_num AS REAL) / fps_den FROM media WHERE shots = 'ready' ORDER BY position, id"
    ).fetchall()[nth]
    shots = db.execute(
        "SELECT start, end FROM segments WHERE media_id = ? AND kind = 'shot' ORDER BY start", (mid,)).fetchall()
    seen = {row[0] for row in db.execute(
        "SELECT DISTINCT shot FROM tracks WHERE media_id = ? AND not_face = 0 AND assign IN ('auto', 'pinned')"
        " AND character_id IS NOT NULL", (mid,))}
    return shots, [start in seen for start, _ in shots], frames, fps


def decode(ffmpeg, video, want, total, size):
    w, h = size
    cmd = [ffmpeg, "-v", "error", "-i", video, "-map", "0:v:0", "-vf", f"scale={w}:{h}:flags=bicubic",
           "-fps_mode", "passthrough", "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE)
    out, n = {}, 0
    while True:
        buf = p.stdout.read(w * h * 3)
        if len(buf) < w * h * 3:
            break
        if n in want:
            out[n] = Image.fromarray(np.frombuffer(buf, np.uint8).reshape(h, w, 3).copy())
        n += 1
    p.wait()
    assert n == total, f"decoded {n} frames, the project has {total}: not the file the project was made from"
    return out


def at(shot, share):
    return shot[0] + int((shot[1] - shot[0]) * share)


def drops(ffmpeg, video, folder, nth, out):
    shots, seen, total, fps = project(folder, int(nth))
    w, h, strip, per = 384, 216, 24, 4
    rows = [i for i in range(len(shots)) if not seen[i]]
    want = set()
    for i in rows:
        want |= {at(shots[i], s) for s in (1 / 6, 1 / 2, 5 / 6)}
        want |= {at(shots[j], 1 / 2) for j in (i - 1, i + 1) if 0 <= j < len(shots)}
    pictures = decode(ffmpeg, video, want, total, (w, h))
    os.makedirs(out, exist_ok=True)
    font = ImageFont.load_default(size=17)
    with open(os.path.join(out, "rows.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("sheet\tshot\tstart\tend\n")
        for s in range(0, len(rows), per):
            page = Image.new("RGB", (5 * w, per * (h + strip)), (16, 16, 16))
            draw = ImageDraw.Draw(page)
            for r, i in enumerate(rows[s:s + per]):
                y = r * (h + strip)
                if i > 0:
                    page.paste(pictures[at(shots[i - 1], 1 / 2)], (0, y))
                for k, share in enumerate((1 / 6, 1 / 2, 5 / 6)):
                    page.paste(pictures[at(shots[i], share)], ((1 + k) * w, y))
                if i + 1 < len(shots):
                    page.paste(pictures[at(shots[i + 1], 1 / 2)], (4 * w, y))
                draw.rectangle((w, y, 4 * w - 1, y + h - 1), outline=(255, 220, 90), width=3)
                when = int(shots[i][0] / fps)
                draw.text((4, y + h + 2), "the shot before", fill=(150, 150, 150), font=font)
                draw.text((w + 4, y + h + 2),
                          f"shot {i}    {when // 60}:{when % 60:02d}    {(shots[i][1] - shots[i][0]) / fps:.1f} s long",
                          fill=(255, 220, 90), font=font)
                draw.text((4 * w + 4, y + h + 2), "the shot after", fill=(150, 150, 150), font=font)
                f.write(f"{s // per}\t{i}\t{shots[i][0]}\t{shots[i][1]}\n")
            page.save(os.path.join(out, f"sheet_{s // per:03d}.jpg"), quality=88)
    print(f"{len(rows)} shots without anyone the cast shows, on {(len(rows) + per - 1) // per} sheets in {out}")


def detail(path):
    """`[(shot, found scene, own, kept by the filter)]` for the shots of the character's whole scenes."""
    out = []
    for line in list(open(path, encoding="utf-8"))[2:]:
        f = line.rstrip("\n").split("\t")
        if len(f) >= 6 and f[5][7] == "s":
            out.append((int(f[0]), int(f[4]), f[5][0] == "o", f[5][5] == "a"))
    return out


def runs(shots, picked):
    """The frame ranges of the picked shots, neighbours joined."""
    out = []
    for i in sorted(picked):
        if out and out[-1][1] == shots[i][0]:
            out[-1][1] = shots[i][1]
        else:
            out.append(list(shots[i]))
    return out


def video_of(ffmpeg, video, shots, fps, groups, out):
    """`groups`: `[(colour, shots)]`; every shot of any group is in the video, with its group's bar.
    Cut in pieces of thirty shots each and joined: one filter for a long pack is more than FFmpeg
    takes."""
    wanted = sorted({i for _, picked in groups for i in picked})
    parts = []
    for c in range(0, len(wanted), 30):
        here = set(wanted[c:c + 30])
        piece = runs(shots, here)
        first, last = piece[0][0], piece[-1][1]
        t0 = first / fps
        span = lambda rs: "+".join(
            f"between(t,{(a - 0.5) / fps - t0:.4f},{(b - 0.5) / fps - t0:.4f})" for a, b in rs) or "0"
        bars = "".join(
            f"drawbox=x=0:y=0:w=iw:h=10:color={colour}:t=fill:enable='{span(inside)}',"
            for colour, picked in groups
            if (inside := runs(shots, [i for i in picked if i in here])))
        graph = (f"[0:v:0]{bars}select='{span(piece)}',setpts=N/FRAME_RATE/TB,scale=640:360[v];"
                 f"[0:a:0]aselect='{span(piece)}',asetpts=N/SR/TB[a]")
        part = f"{out}.part{c // 30:03d}.mp4"
        subprocess.run([ffmpeg, "-v", "error", "-y", "-ss", f"{t0:.4f}", "-to", f"{last / fps:.4f}", "-i", video,
                        "-filter_complex", graph, "-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-preset",
                        "veryfast", "-crf", "26", "-c:a", "aac", "-b:a", "96k", part], check=True)
        parts.append(part)
    listing = f"{out}.parts.txt"
    with open(listing, "w", encoding="utf-8", newline=chr(10)) as f:
        for part in parts:
            f.write("file '" + os.path.abspath(part).replace(os.sep, "/") + "'" + chr(10))
    subprocess.run([ffmpeg, "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", listing, "-c", "copy", out],
                   check=True)
    for path in parts + [listing]:
        os.remove(path)


def pack(ffmpeg, video, folder, nth, path, prefix):
    shots, _, total, fps = project(folder, int(nth))
    rows = detail(path)
    w, h, strip, cols, lines = 240, 135, 22, 8, 6
    pictures = decode(ffmpeg, video, {at(shots[i], 1 / 2) for i, *_ in rows}, total, (w, h))
    font = ImageFont.load_default(size=15)
    per = cols * lines
    for s in range(0, len(rows), per):
        page = Image.new("RGB", (cols * w, lines * (h + strip)), (16, 16, 16))
        draw = ImageDraw.Draw(page)
        for k, (i, found, own, kept) in enumerate(rows[s:s + per]):
            x, y = (k % cols) * w, (k // cols) * (h + strip)
            picture = pictures[at(shots[i], 1 / 2)]
            if not (own or kept):
                picture = Image.eval(picture, lambda v: v // 3)
            page.paste(picture, (x, y))
            colour = (70, 200, 90) if own else (255, 220, 90) if kept else (220, 60, 60)
            if own or kept:
                draw.rectangle((x, y, x + w - 1, y + h - 1), outline=colour, width=3)
            else:
                draw.rectangle((x, y, x + w - 1, y + 7), fill=colour)
            first = k + s == 0 or rows[k + s - 1][1] != found
            when = int(shots[i][0] / fps)
            draw.text((x + 4, y + h + 2), f"{'| scene ' + str(found) + '  ' if first else ''}{i}  {when // 60}:{when % 60:02d}"
                      f"  {(shots[i][1] - shots[i][0]) / fps:.0f}s", fill=colour if first else (200, 200, 200), font=font)
        page.save(f"{prefix}.sheet_{s // per:03d}.jpg", quality=88)
    own = [i for i, _, o, _ in rows if o]
    added = [i for i, _, o, k in rows if k and not o]
    left = [i for i, _, o, k in rows if not (o or k)]
    video_of(ffmpeg, video, shots, fps, [("green", own), ("yellow", added)], f"{prefix}.filter.mp4")
    video_of(ffmpeg, video, shots, fps, [("green", own), ("yellow", added), ("red", left)], f"{prefix}.scenes.mp4")
    secs = lambda picked: sum(shots[i][1] - shots[i][0] for i in picked) / fps
    print(f"{prefix}: {len(own)} own shots ({secs(own):.0f} s), the filter adds {len(added)} ({secs(added):.0f} s) "
          f"and leaves out {len(left)} ({secs(left):.0f} s); {(len(rows) + per - 1) // per} sheets, two videos")


def kinds(path):
    out = {}
    for line in list(open(path, encoding="utf-8"))[1:]:
        if line.strip():
            shot, kind = line.rstrip("\n").split("\t")[:2]
            assert kind in ("talk", "place", "other", "nothing"), f"{path}: {line!r}"
            assert int(shot) not in out, f"{path}: shot {shot} twice"
            out[int(shot)] = kind
    return out


def tally(folder, nth, a_path, b_path, paths):
    shots, seen, _, fps = project(folder, int(nth))
    a, b = kinds(a_path), kinds(b_path)
    unseen = {i for i in range(len(shots)) if not seen[i]}
    assert set(a) == unseen and set(b) == unseen, "each reader sorts every shot in which the cast shows nobody"
    secs = lambda picked: sum(shots[i][1] - shots[i][0] for i in picked) / fps
    same = [i for i in unseen if a[i] == b[i]]
    print(f"{len(unseen)} shots without anyone the cast shows ({secs(unseen):.0f} s); the readers give the same kind in "
          f"{len(same)} ({len(same) / len(unseen):.3f})")
    for kind in ("talk", "place", "other", "nothing"):
        both = [i for i in unseen if a[i] == kind and b[i] == kind]
        print(f"  {kind}: reader a {sum(v == kind for v in a.values())}, reader b {sum(v == kind for v in b.values())}, "
              f"both {len(both)} ({secs(both):.0f} s)")
    out_all, talk_all, left_once, talk_once = 0.0, 0.0, set(), set()
    for path in paths:
        rows = detail(path)
        left = [i for i, _, own, kept in rows if not (own or kept)]
        assert set(left) <= unseen, f"{path}: a left-out shot in which the cast shows someone"
        talk = [i for i in left if a[i] == "talk" and b[i] == "talk"]
        either = [i for i in left if a[i] == "talk" or b[i] == "talk"]
        holes, run, last_found, kept_before = [], [], None, False
        for i, found, own, kept in rows:
            if found != last_found:
                run, kept_before, last_found = [], False, found
            if own or kept:
                if run and kept_before:
                    holes.append(run)
                run, kept_before = [], True
            else:
                run = run + [i]
        lengths = sorted(secs(hole) for hole in holes)
        middle = (lengths[(len(lengths) - 1) // 2] + lengths[len(lengths) // 2]) / 2 if lengths else 0.0
        in_holes = [i for hole in holes for i in hole]
        print(f"{os.path.basename(path)}: whole scenes {len(rows)} shots ({secs([r[0] for r in rows]):.0f} s); left out "
              f"{len(left)} ({secs(left):.0f} s); of them talk by both readers {len(talk)} ({secs(talk):.0f} s, "
              f"{secs(talk) / max(secs(left), 1e-9):.3f} of the seconds), by either {len(either)} ({secs(either):.0f} s); "
              f"holes {len(holes)} holding {len(in_holes)} of the left-out shots ({secs(in_holes):.0f} s), "
              f"median {middle:.1f} s, longest {lengths[-1] if lengths else 0:.1f} s, "
              f"{sum(v <= 6 for v in lengths)} of them up to 6 s; talk in holes "
              f"{len([i for i in talk if i in in_holes])} of {len(talk)}")
        # What a filter that kept the holes of up to 6 s would take back, by what the readers call it.
        short = [i for hole in holes if secs(hole) <= 6 for i in hole]
        both = lambda kind: [i for i in short if a[i] == kind and b[i] == kind]
        print(f"  in the {sum(secs(hole) <= 6 for hole in holes)} holes of up to 6 s: {len(short)} shots ({secs(short):.0f} s): "
              + ", ".join(f"{kind} {len(both(kind))} ({secs(both(kind)):.0f} s)" for kind in ("talk", "place", "other", "nothing"))
              + f", the readers apart {len([i for i in short if a[i] != b[i]])}; talk left outside them "
              f"{len([i for i in talk if i not in short])} ({secs([i for i in talk if i not in short]):.0f} s)")
        out_all += secs(left)
        talk_all += secs(talk)
        left_once |= set(left)
        talk_once |= set(talk)
    print(f"all characters given: {talk_all:.0f} s of talk in {out_all:.0f} s left out: {talk_all / max(out_all, 1e-9):.3f}; "
          f"each shot counted once: {secs(talk_once):.1f} s in {secs(left_once):.1f} s: "
          f"{secs(talk_once) / max(secs(left_once), 1e-9):.3f}")


def main():
    mode = sys.argv[1]
    if mode == "drops":
        drops(*sys.argv[2:7])
    elif mode == "pack":
        pack(*sys.argv[2:8])
    else:
        assert mode == "tally", "drops, pack or tally"
        tally(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5], sys.argv[6:])


main()
