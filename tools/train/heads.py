"""The heads the app's own detector finds in iCartoonFace's pictures: a training crop is then the crop the app
would make of that picture (its head box padded by a quarter), not one around the set's tighter face box.

    uv run heads.py <rectrain.zip> <anime head detector.onnx> <out.tsv> [--every <n>] [--cpu]

Per picture the head the face box belongs to: the detected box (score at least 0.25) that holds the face
box's middle and overlaps it most. Written as `picture x1 y1 x2 y2 score` in the picture's pixels (`-` where
no head was found: the face box stands in, widened as heads are on average). `--every <n>` takes every n-th
picture only (a sample). The detector runs letterboxed at 640 × 640 on DirectML, as the anime pass runs it on
a frame. Rows are written as they come, and a start goes on after the rows that are there.
"""
import io
import sys
import zipfile
from pathlib import Path

import numpy as np
from PIL import Image

import data

SIDE = 640
LEAST = 0.25
BATCH = 16


def letterbox(image):
    w, h = image.size
    s = min(SIDE / w, SIDE / h)
    nw, nh = max(1, round(w * s)), max(1, round(h * s))
    canvas = Image.new("RGB", (SIDE, SIDE), (114, 114, 114))
    ox, oy = (SIDE - nw) // 2, (SIDE - nh) // 2
    canvas.paste(image.resize((nw, nh), Image.BILINEAR), (ox, oy))
    return np.asarray(canvas, dtype=np.float32).transpose(2, 0, 1) / 255.0, (s, ox, oy)


def head_of(out, placed, size, face):
    """The detected head a face box belongs to, in the picture's pixels, or `None`."""
    s, ox, oy = placed
    if out.shape[0] < out.shape[1]:
        out = out.T
    out = out[out[:, 4] >= LEAST]
    best, most = None, 0.0
    cx, cy = (face[0] + face[2]) / 2, (face[1] + face[3]) / 2
    for bx, by, bw, bh, score in out:
        x1, y1 = max(0.0, (bx - bw / 2 - ox) / s), max(0.0, (by - bh / 2 - oy) / s)
        x2, y2 = min(float(size[0]), (bx + bw / 2 - ox) / s), min(float(size[1]), (by + bh / 2 - oy) / s)
        if not (x1 <= cx <= x2 and y1 <= cy <= y2):
            continue
        iw, ih = min(x2, face[2]) - max(x1, face[0]), min(y2, face[3]) - max(y1, face[1])
        both = max(0.0, iw) * max(0.0, ih)
        iou = both / ((x2 - x1) * (y2 - y1) + (face[2] - face[0]) * (face[3] - face[1]) - both)
        if iou > most:
            best, most = (x1, y1, x2, y2, float(score)), iou
    return best


def main():
    a = sys.argv[1:]
    plain = [x for k, x in enumerate(a) if not x.startswith("--") and (k == 0 or a[k - 1] != "--every")]
    if len(plain) != 3:
        sys.exit(__doc__)
    path, onnx_path, out = plain
    every = int(a[a.index("--every") + 1]) if "--every" in a else 1
    from concurrent.futures import ThreadPoolExecutor

    import onnxruntime as ort

    # Every free size fixed: DirectML is slow on a graph whose shapes are left open.
    options = ort.SessionOptions()
    for free, size in (("batch", BATCH), ("height", SIDE), ("width", SIDE)):
        options.add_free_dimension_override_by_name(free, size)
    providers = ["CPUExecutionProvider"] if "--cpu" in a else ["DmlExecutionProvider", "CPUExecutionProvider"]
    session = ort.InferenceSession(onnx_path, options, providers=providers)
    name = session.get_inputs()[0].name
    rows = data.index(path)[::every]
    # What an earlier start wrote stays: the run goes on after it.
    out = Path(out)
    done = len(out.read_text(encoding="utf-8").splitlines()) - 1 if out.exists() else 0
    if done <= 0:
        out.write_text("picture\tx1\ty1\tx2\ty2\tscore\n", encoding="utf-8", newline="\n")
        done = 0
    found = 0

    def ready(raw):
        image = Image.open(io.BytesIO(raw)).convert("RGB")
        return image.size, letterbox(image)

    with zipfile.ZipFile(path) as z, ThreadPoolExecutor(8) as pool, open(out, "a", encoding="utf-8", newline="\n") as f:
        for start in range(done, len(rows), BATCH):
            part = rows[start : start + BATCH]
            boxed = list(pool.map(ready, [z.read(member) for member, _, _ in part]))
            x = np.stack([b[1][0] for b in boxed])
            if len(part) < BATCH:
                x = np.concatenate([x, np.zeros((BATCH - len(part), *x.shape[1:]), dtype=np.float32)])
            outs = session.run(None, {name: x})[0]
            for (member, _, face), (size, (_, placed)), o in zip(part, boxed, outs):
                head = head_of(o, placed, size, face)
                found += head is not None
                short = member.rsplit("/", 1)[-1]
                f.write(f"{short}\t" + ("-\t-\t-\t-\t-" if head is None else "%.1f\t%.1f\t%.1f\t%.1f\t%.3f" % head) + "\n")
            if (start - done) % (BATCH * 250) == 0:
                f.flush()
                print(f"{start} of {len(rows)} pictures, a head in {found} since this start", flush=True)
    print(f"{out}: {len(rows)} pictures done, a head in {found} of the {len(rows) - done} of this start, on {session.get_providers()[0]}")


if __name__ == "__main__":
    main()
