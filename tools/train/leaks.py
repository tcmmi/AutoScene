"""Bench characters inside iCartoonFace, by how they look: CCIP's nearest neighbours of the bench's labelled
tracks among the training identities (`bench/models/README.md`: every bench franchise leaves the training
data, by name and by nearest neighbour).

    uv run leaks.py embed <rectrain.zip> <ccip.onnx> <identities.npz> [--per 6]
        CCIP's vectors of `--per` pictures of every identity (the square of 2.4 face boxes around the face:
        about what the anime pass shows CCIP of a head).

    uv run leaks.py near <identities.npz> <out.tsv> <project>=<tracks.emb>,<names.tsv> …
        Per identity, the bench figure most like it. `<tracks.emb>` holds a Standard pass's own vectors
        (`identity_bench embout <project>.std3.tracks`), `<names.tsv>` its tracks' labels (the fifth column
        of `identity_bench chars`' `DUMP`), one row per track; the labels' codes become figures by
        `bench/anime/<project>.gallery.tsv` (a figure's looks are one figure). An identity's likeness to a
        figure is the median, over its pictures, of each picture's best cosine to the figure's tracks.
        Written most alike first: `identity project figure likeness second` (`second`: the next figure's).
"""
import io
import sys
import zipfile
from pathlib import Path

import numpy as np
from PIL import Image

import crops
import data

SIDE = 384
SCALE = 2.4
MEAN = np.array([0.48145466, 0.4578275, 0.40821072], dtype=np.float32)
STD = np.array([0.26862954, 0.2613026, 0.2757771], dtype=np.float32)
# Labels that name nobody: extras, non-faces, unreadable or left-out tracks.
NOBODY = {"n", "?e", "?u", "x", "?", "-", ""}
BENCH = Path(__file__).resolve().parents[2] / "bench" / "anime"
BATCH = 8


def look(image, box):
    """The square around a face box that shows the head with its hair, as CCIP's input."""
    side = max(box[2] - box[0], box[3] - box[1]) * SCALE
    cx, cy = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
    area = tuple(round(v) for v in (cx - side / 2, cy - side / 2, cx + side / 2, cy + side / 2))
    return np.asarray(image.crop(area).resize((SIDE, SIDE), Image.BILINEAR), dtype=np.float32)


def embed(path, onnx_path, out, per):
    import onnxruntime as ort

    options = ort.SessionOptions()
    options.add_free_dimension_override_by_name("batch", BATCH)  # DirectML wants its batch fixed
    session = ort.InferenceSession(onnx_path, options, providers=["DmlExecutionProvider", "CPUExecutionProvider"])
    name = session.get_inputs()[0].name
    groups = {}
    for member, identity, box in data.index(path):
        groups.setdefault(identity, []).append((member, box))
    names = sorted(groups)
    owners, pictures = [], []
    for i, identity in enumerate(names):
        got = groups[identity]
        for k in np.linspace(0, len(got) - 1, min(per, len(got))).astype(int):
            owners.append(i)
            pictures.append(got[k])
    vectors = np.zeros((len(pictures), 768), dtype=np.float32)
    with zipfile.ZipFile(path) as z:
        for start in range(0, len(pictures), BATCH):
            part = pictures[start : start + BATCH]
            x = np.stack([look(Image.open(io.BytesIO(z.read(m))).convert("RGB"), b) for m, b in part])
            x = np.ascontiguousarray(((x / 255.0 - MEAN) / STD).transpose(0, 3, 1, 2))
            if len(part) < BATCH:
                x = np.concatenate([x, np.zeros((BATCH - len(part), *x.shape[1:]), dtype=np.float32)])
            vectors[start : start + len(part)] = session.run(None, {name: x})[0][: len(part)]
            if start % (BATCH * 250) == 0:
                print(f"{start} of {len(pictures)} pictures", flush=True)
    np.savez(out, names=np.array(names), owner=np.array(owners), vectors=vectors)
    print(f"{out}: {len(pictures)} pictures of {len(names)} identities on {session.get_providers()[0]}")


def near(identities, out, projects):
    got = np.load(identities)
    names, owner = got["names"], got["owner"]
    vectors = got["vectors"] / np.maximum(np.linalg.norm(got["vectors"], axis=1, keepdims=True), 1e-12)
    best = np.full((len(names), 2), -1.0, dtype=np.float32)
    who = [("", "")] * len(names)
    for spec in projects:
        project, files = spec.split("=", 1)
        emb, dump = files.split(",")
        tracks = crops.read_override(emb)
        labels = [line.split("\t")[4] for line in Path(dump).read_text(encoding="utf-8").splitlines() if line.strip()]
        assert len(labels) == len(tracks), (project, len(labels), len(tracks))
        gallery = [line.split("\t") for line in (BENCH / f"{project}.gallery.tsv").read_text(encoding="utf-8").splitlines()[1:]]
        figure = {row[0]: row[1] for row in gallery if len(row) > 1}
        labels = [figure.get(l, l) for l in labels]
        for label in sorted(set(labels) - NOBODY):
            rows = tracks[[i for i, l in enumerate(labels) if l == label]]
            rows = rows / np.maximum(np.linalg.norm(rows, axis=1, keepdims=True), 1e-12)
            top = (vectors @ rows.T).max(1)
            for i in range(len(names)):
                like = float(np.median(top[owner == i]))
                if like > best[i, 0]:
                    best[i] = (like, best[i, 0])
                    who[i] = (project, label)
                elif like > best[i, 1]:
                    best[i, 1] = like
    order = np.argsort(-best[:, 0])
    lines = ["identity\tproject\tfigure\tlikeness\tsecond"]
    for i in order:
        lines.append(f"{str(names[i]).rsplit('_', 1)[-1]}\t{who[i][0]}\t{who[i][1]}\t{best[i, 0]:.3f}\t{best[i, 1]:.3f}")
    Path(out).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{out}: {len(names)} identities; the ten most alike:")
    print("\n".join(lines[1:11]))


def main():
    a = sys.argv[1:]
    if len(a) >= 4 and a[0] == "embed":
        embed(a[1], a[2], a[3], int(a[a.index("--per") + 1]) if "--per" in a else 6)
    elif len(a) >= 4 and a[0] == "near":
        near(a[1], a[2], a[3:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
