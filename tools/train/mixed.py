"""Identities of iCartoonFace that hold more than one character, and pictures filed under the wrong one —
looked for with embedders that never trained on the identities they judge (`train.py --fold k/n`).

    uv run mixed.py embed <rectrain.zip> <run dir> <out.npz> [--exclude <file>] [--heads <heads.tsv>]
        The vectors of every picture of the identities the run did not train on: all but those of its
        `identities.txt` and of the exclusion list. The checkpoint is the run's `best.pt`.

    uv run mixed.py find <out.tsv> <vectors.npz> [<vectors.npz> …]
        Per identity (the first file that holds it counts), least coherent first:
        `identity pictures likeness smaller apart` — the mean likeness of its pictures to the rest of the
        identity (the cosine to the middle of the others), and what its two halves are like when it is cut
        in two around its two least alike pictures: the smaller half's share and the cosine between the
        halves' middles. Beside it `<out>.pictures.tsv`: `identity picture likeness half` for every picture.

    uv run mixed.py sheets <rectrain.zip> <out.tsv of find> <out dir> [--share 0.2] [--heads <heads.tsv>]
        Contact sheets of the least coherent `--share` of the identities, to be looked at: twelve identities
        a sheet, a row each with five pictures of either half (spread over it, the first half left).
        `<out dir>/rows.tsv` says which identity is where.

The vectors alone do not settle it: characters who look alike stay alike to a model trained for three epochs.
They say where to look and which single pictures are strangers.
"""
import sys
from pathlib import Path

import numpy as np

import data

CELL = 128


def embed(path, run, out, exclude, heads):
    import torch
    import torch.nn.functional as F

    import model

    run = Path(run)
    seen = set((run / "identities.txt").read_text().split()) | data.read_exclude(exclude)
    rows = data.index(path, heads)
    names = sorted({r[1] for r in rows} - seen)
    faces = data.Faces(path, rows, names)
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    net = model.from_checkpoint(run / "best.pt").to(device).eval()
    loader = torch.utils.data.DataLoader(faces, batch_size=256, shuffle=False, num_workers=4)
    vectors = []
    with torch.no_grad():
        for k, (x, _) in enumerate(loader):
            with torch.autocast(device.type, dtype=torch.float16, enabled=device.type == "cuda"):
                v = net(data.normalise(x.to(device)))
            vectors.append(F.normalize(v.float(), dim=1).cpu().numpy().astype(np.float16))
            if k % 100 == 0:
                print(f"{k * 256} of {len(faces)} pictures", flush=True)
    np.savez(
        out,
        identity=np.array([names[label] for _, label, _ in faces.items]),
        picture=np.array([member.rsplit("/", 1)[-1] for member, _, _ in faces.items]),
        vectors=np.concatenate(vectors),
    )
    print(f"{out}: {len(faces)} pictures of {len(names)} identities the run never trained on")


def unit(v):
    return v / np.maximum(np.linalg.norm(v, axis=-1, keepdims=True), 1e-12)


def halves(v, rounds=12):
    """An identity's pictures cut in two around its two least alike ones: which picture is in the first
    half, and the cosine between the halves' middles."""
    like = v @ v.T
    a, b = np.unravel_index(like.argmin(), like.shape)
    first, second = v[a], v[b]
    side = v @ first >= v @ second
    for _ in range(rounds):
        if side.all() or not side.any():
            break
        first, second = unit(v[side].mean(0)), unit(v[~side].mean(0))
        again = v @ first >= v @ second
        if (again == side).all():
            break
        side = again
    if side.mean() < 0.5:  # the bigger half first
        side, first, second = ~side, second, first
    return side, float(first @ second)


def find(out, files):
    identity, picture, vectors, held = [], [], [], set()
    for f in files:
        got = np.load(f)
        keep = np.array([i not in held for i in got["identity"]])
        identity.append(got["identity"][keep])
        picture.append(got["picture"][keep])
        vectors.append(got["vectors"][keep].astype(np.float32))
        held |= set(got["identity"].tolist())
    identity, picture, vectors = np.concatenate(identity), np.concatenate(picture), np.concatenate(vectors)
    names, owner = np.unique(identity, return_inverse=True)
    sums = np.zeros((len(names), vectors.shape[1]), dtype=np.float32)
    np.add.at(sums, owner, vectors)
    like = (vectors * unit(sums[owner] - vectors)).sum(1)
    rows, lines = [], ["identity\tpicture\tlikeness\thalf"]
    for i, name in enumerate(names):
        at = np.flatnonzero(owner == i)
        side, apart = halves(vectors[at])
        short = name.rsplit("_", 1)[-1]
        rows.append((short, len(at), float(like[at].mean()), float(1 - side.mean()), apart))
        for k, j in enumerate(at):
            lines.append(f"{short}\t{picture[j]}\t{like[j]:.3f}\t{1 if side[k] else 2}")
    rows.sort(key=lambda r: r[2])
    text = ["identity\tpictures\tlikeness\tsmaller\tapart"] + ["%s\t%d\t%.3f\t%.3f\t%.3f" % r for r in rows]
    Path(out).write_text("\n".join(text) + "\n", encoding="utf-8", newline="\n")
    Path(out + ".pictures.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{out}: {len(vectors)} pictures of {len(names)} identities; a picture's likeness to the rest of its identity: 1st percentile {np.quantile(like, 0.01):.2f}, 5th {np.quantile(like, 0.05):.2f}, median {np.median(like):.2f}")


def sheets(path, table, out, share, heads):
    import io
    import zipfile

    from PIL import Image, ImageDraw

    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    order = [line.split("\t")[0] for line in Path(table).read_text(encoding="utf-8").splitlines()[1:]]
    order = order[: round(len(order) * share)]
    half = {}
    for line in Path(table + ".pictures.tsv").read_text(encoding="utf-8").splitlines()[1:]:
        name, picture, _, side = line.split("\t")
        half.setdefault(name, ([], []))[int(side) - 1].append(picture)
    where = {member.rsplit("/", 1)[-1]: (member, box) for member, _, box in data.index(path, heads)}
    log = ["row\tsheet\tline\tidentity"]
    with zipfile.ZipFile(path) as z:
        for s in range(0, len(order), 12):
            page = Image.new("RGB", (60 + 10 * CELL + 12, 12 * (CELL + 6)), "black")
            draw = ImageDraw.Draw(page)
            for r, name in enumerate(order[s : s + 12]):
                y = r * (CELL + 6)
                draw.text((4, y + 4), str(s + r), fill="yellow")
                first, second = half[name]
                # Five of either half, spread over it; a half too small gives its places to the other.
                take = lambda pics, n: [pics[k] for k in np.linspace(0, len(pics) - 1, min(n, len(pics))).astype(int)]
                right = take(second, 5)
                left = take(first, 10 - len(right))
                for k, picture in enumerate(left + right):
                    member, box = where[picture]
                    im = data.crop(Image.open(io.BytesIO(z.read(member))).convert("RGB"), box).resize((CELL, CELL), Image.BILINEAR)
                    page.paste(im, (60 + k * CELL + (12 if k >= len(left) else 0), y))
                log.append(f"{s + r}\t{s // 12:02d}\t{r}\t{name}")
            page.save(out / f"sheet_{s // 12:02d}.jpg", quality=88)
    (out / "rows.tsv").write_text("\n".join(log) + "\n", encoding="utf-8", newline="\n")
    print(f"{out}: {len(order)} identities on {(len(order) + 11) // 12} sheets")


def main():
    a = sys.argv[1:]
    flag = lambda name, default: a[a.index(name) + 1] if name in a else default
    plain = [x for k, x in enumerate(a) if not x.startswith("--") and (k == 0 or not a[k - 1].startswith("--"))]
    if len(plain) >= 4 and plain[0] == "embed":
        embed(plain[1], plain[2], plain[3], flag("--exclude", None), flag("--heads", None))
    elif len(plain) >= 3 and plain[0] == "find":
        find(plain[1], plain[2:])
    elif len(plain) >= 4 and plain[0] == "sheets":
        sheets(plain[1], plain[2], plain[3], float(flag("--share", 0.2)), flag("--heads", None))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
