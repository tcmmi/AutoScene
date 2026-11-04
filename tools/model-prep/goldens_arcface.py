"""Bake the ArcFace parity goldens a GPU session of the app proves itself
against (`crates/as-vision/fixtures/arcface`): the faces of the Tears of
Steel stills the app ships (`app/public/footage/live-*.webp`; CC BY 3.0,
Blender Foundation), aligned to ArcFace's 112x112 template, as raw RGB bytes,
and the vectors ONNX Runtime's CPU gives them.

    uv run goldens_arcface.py <ffmpeg dir> <stills dir> <scrfd_10g.onnx> <arcface_glintr100.onnx> <out dir>

- ffmpeg decodes the stills, so no imaging library is needed.
- SCRFD (InsightFace det_10g, CPU) finds the faces and their five points at
  the still's own size, padded to a multiple of 32: RGB, (v - 127.5) / 128,
  strides 8/16/32 with two anchors each, score >= 0.6, NMS at IoU 0.4.
- Each face is aligned by the least-squares similarity (Umeyama, no
  reflection) that maps its points onto the template, sampled bilinearly
  with black outside the frame, as `as_vision::arcface::align` does.
- ArcFace reads RGB, (v - 127.5) / 127.5, NCHW; its raw output is stored.

Outputs, per face i (strongest first): tos_<i>.rgb (112*112*3 bytes),
tos_<i>.vec (512 little-endian f32) and golden.json (still, box, score,
points, model hashes).
"""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

import numpy as np
import onnxruntime as ort

TEMPLATE = np.array(
    [[38.2946, 51.6963], [73.5318, 51.5014], [56.0252, 71.7366], [41.5493, 92.3655], [70.7299, 92.2041]],
    np.float64,
)
CROP = 112
STRIDES = (8, 16, 32)
ANCHORS = 2
SCORE = 0.6
NMS_IOU = 0.4
# Faces whose eyes are closer than this (pixels) say too little to be a golden.
MIN_EYES = 20.0


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def decode(ffmpeg: Path, still: Path) -> np.ndarray:
    """The still as an H x W x 3 uint8 RGB array."""
    probe = subprocess.run(
        [str(ffmpeg / "ffprobe"), "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height",
         "-of", "csv=p=0", str(still)],
        check=True, capture_output=True, text=True,
    )
    w, h = (int(v) for v in probe.stdout.strip().split(","))
    raw = subprocess.run(
        [str(ffmpeg / "ffmpeg"), "-v", "error", "-i", str(still), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        check=True, capture_output=True,
    ).stdout
    return np.frombuffer(raw, np.uint8).reshape(h, w, 3)


def iou(a: np.ndarray, b: np.ndarray) -> float:
    x0, y0 = max(a[0], b[0]), max(a[1], b[1])
    x1, y1 = min(a[2], b[2]), min(a[3], b[3])
    inter = max(0.0, x1 - x0) * max(0.0, y1 - y0)
    union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter
    return inter / union if union > 0 else 0.0


def detect(scrfd: ort.InferenceSession, rgb: np.ndarray) -> list[tuple[float, np.ndarray, np.ndarray]]:
    """(score, box x0 y0 x1 y1, points 5x2) of each face, strongest first."""
    h, w, _ = rgb.shape
    pw, ph = -(-w // 32) * 32, -(-h // 32) * 32
    x = np.zeros((1, 3, ph, pw), np.float32)
    x[0, :, :h, :w] = ((rgb.astype(np.float32) - 127.5) / 128.0).transpose(2, 0, 1)
    outs = scrfd.run(None, {scrfd.get_inputs()[0].name: x})
    faces = []
    for level, s in enumerate(STRIDES):
        gw, gh = pw // s, ph // s
        scores = outs[level].reshape(-1)
        boxes = outs[level + 3].reshape(-1, 4)
        kps = outs[level + 6].reshape(-1, 10)
        for i in np.nonzero(scores >= SCORE)[0]:
            loc = i // ANCHORS
            cx, cy = (loc % gw) * s, (loc // gw) * s
            b = boxes[i] * s
            box = np.array([cx - b[0], cy - b[1], cx + b[2], cy + b[3]])
            pts = np.array([[cx + kps[i][2 * j] * s, cy + kps[i][2 * j + 1] * s] for j in range(5)])
            faces.append((float(scores[i]), box, pts))
        assert gw * gh * ANCHORS == scores.size, f"stride {s}: {scores.size} anchors"
    faces.sort(key=lambda f: -f[0])
    kept: list[tuple[float, np.ndarray, np.ndarray]] = []
    for f in faces:
        if all(iou(f[1], k[1]) <= NMS_IOU for k in kept):
            kept.append(f)
    return kept


def similarity(pts: np.ndarray) -> tuple[float, float, float, float]:
    """(a, b, tx, ty) of x' = a x - b y + tx, y' = b x + a y + ty, mapping pts onto the template."""
    sm, dm = pts.mean(0), TEMPLATE.mean(0)
    s, d = pts - sm, TEMPLATE - dm
    den = (s ** 2).sum()
    a = (s[:, 0] * d[:, 0] + s[:, 1] * d[:, 1]).sum() / den
    b = (s[:, 0] * d[:, 1] - s[:, 1] * d[:, 0]).sum() / den
    tx = dm[0] - (a * sm[0] - b * sm[1])
    ty = dm[1] - (b * sm[0] + a * sm[1])
    return a, b, tx, ty


def align(rgb: np.ndarray, pts: np.ndarray) -> np.ndarray:
    """The 112x112 RGB crop: each template pixel sampled bilinearly at its source position."""
    h, w, _ = rgb.shape
    a, b, tx, ty = similarity(pts)
    det = max(a * a + b * b, 1e-12)
    v, u = np.mgrid[0:CROP, 0:CROP].astype(np.float64)
    px, py = u - tx, v - ty
    sx, sy = (a * px + b * py) / det, (-b * px + a * py) / det
    x0, y0 = np.floor(sx), np.floor(sy)
    fx, fy = (sx - x0)[..., None], (sy - y0)[..., None]
    src = rgb.astype(np.float64)

    def at(xx: np.ndarray, yy: np.ndarray) -> np.ndarray:
        inside = (xx >= 0) & (yy >= 0) & (xx < w) & (yy < h)
        out = np.zeros((CROP, CROP, 3))
        out[inside] = src[yy[inside].astype(int), xx[inside].astype(int)]
        return out

    mixed = (at(x0, y0) * (1 - fx) * (1 - fy) + at(x0 + 1, y0) * fx * (1 - fy)
             + at(x0, y0 + 1) * (1 - fx) * fy + at(x0 + 1, y0 + 1) * fx * fy)
    return np.clip(np.round(mixed), 0, 255).astype(np.uint8)


def main() -> None:
    ffmpeg, stills_dir, scrfd_path, arcface_path, out = map(Path, sys.argv[1:6])
    out.mkdir(parents=True, exist_ok=True)
    stills = sorted(stills_dir.glob("live-*.webp"))
    assert stills, f"no live-*.webp in {stills_dir}"
    cpu = ["CPUExecutionProvider"]
    quiet = ort.SessionOptions()
    quiet.log_severity_level = 3  # SCRFD's declared output shapes are for 640x640
    scrfd = ort.InferenceSession(str(scrfd_path), sess_options=quiet, providers=cpu)
    arcface = ort.InferenceSession(str(arcface_path), sess_options=quiet, providers=cpu)
    found = []
    for still in stills:
        rgb = decode(ffmpeg, still)
        for score, box, pts in detect(scrfd, rgb):
            if np.linalg.norm(pts[1] - pts[0]) >= MIN_EYES:
                found.append((score, still.name, box, pts, rgb))
    found.sort(key=lambda f: -f[0])
    meta = {
        "source": "Tears of Steel stills (app/public/footage), CC BY 3.0, Blender Foundation",
        "scrfd": {"file": scrfd_path.name, "sha256": sha256(scrfd_path)},
        "arcface": {"file": arcface_path.name, "sha256": sha256(arcface_path)},
        "onnxruntime": ort.__version__,
        "faces": [],
    }
    for i, (score, name, box, pts, rgb) in enumerate(found):
        crop = align(rgb, pts)
        x = ((crop.astype(np.float32) - 127.5) / 127.5).transpose(2, 0, 1)[None]
        vec = arcface.run(None, {arcface.get_inputs()[0].name: x})[0][0]
        (out / f"tos_{i}.rgb").write_bytes(crop.tobytes())
        (out / f"tos_{i}.vec").write_bytes(vec.astype("<f4").tobytes())
        meta["faces"].append({
            "still": name, "score": round(score, 5), "box": [round(float(v), 2) for v in box],
            "points": [[round(float(v), 2) for v in p] for p in pts], "norm": round(float(np.linalg.norm(vec)), 4),
            "rgb": f"tos_{i}.rgb", "vec": f"tos_{i}.vec",
        })
        print(f"tos_{i}: {name} score {score:.3f} eyes {np.linalg.norm(pts[1] - pts[0]):.0f} px")
    (out / "golden.json").write_text(json.dumps(meta, indent=1) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
