"""Picture embeddings of a few frames per shot, for the scenes bench (step B).

    uv run shot_embed.py <ffmpeg> <video> <project_copy_dir> <media> <dinov2.onnx> <resnet50_places365.pth.tar | -> <out_prefix>

The shots come from a COPY of a project (`project.db`), `<media>` being the
media's place in it (0 for the first); the video is decoded once, every frame from frame 0, so that frame n is the n-th picture as in the
project's frame index. Three frames per shot (at 1/6, 1/2 and 5/6 of it) go
through DINOv2 (a general picture network) and, unless `-` stands in its
place, a ResNet-50 trained on Places365 (a place network). Writes `<out_prefix>.dino_cls.emb`,
`.dino_patch.emb` and `.places.emb`: `ASEM`, the number of rows and the
vector length as u32, then per row the frame (u32) and its unit vector (f32),
little-endian. `scenes_bench score` reads them with `EMB=`.

`SIGLIP=<vision_model.onnx>` also writes `<out_prefix>.siglip.emb` (SigLIP 2
so400m at 384 px, its pooled output): a far larger network, to see what a
better encoder would be worth.

The Places365 file is a pickle from 2017: it is loaded weights-only (plain
tensors, no code).
"""
import os
import sqlite3
import struct
import subprocess
import sys
import time

import numpy as np
import onnxruntime as ort
import torch
import torchvision
from PIL import Image

W, H = 448, 252  # 32 x 18 patches of 14 px, the frame's shape
MEAN = np.array([0.485, 0.456, 0.406], np.float32).reshape(1, 3, 1, 1)
STD = np.array([0.229, 0.224, 0.225], np.float32).reshape(1, 3, 1, 1)


def shots(project, nth):
    db = sqlite3.connect(f"file:{project}/project.db?mode=ro", uri=True)
    media = db.execute("SELECT id, frames FROM media WHERE shots = 'ready' ORDER BY position, id").fetchall()
    mid, frames = media[nth]
    rows = db.execute("SELECT start, end FROM segments WHERE media_id = ? AND kind = 'shot' ORDER BY start", (mid,))
    return rows.fetchall(), frames


def wanted(rows):
    frames = set()
    for start, end in rows:
        n = end - start
        frames.update(start + (2 * k + 1) * n // 6 for k in range(3))
    return sorted(frames)


def decode(ffmpeg, video, frames, total):
    want, out = set(frames), {}
    cmd = [ffmpeg, "-v", "error", "-i", video, "-map", "0:v:0", "-vf", f"scale={W}:{H}:flags=bicubic",
           "-fps_mode", "passthrough", "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE)
    size, n = W * H * 3, 0
    while True:
        buf = p.stdout.read(size)
        if len(buf) < size:
            break
        if n in want:
            out[n] = np.frombuffer(buf, np.uint8).reshape(H, W, 3).copy()
        n += 1
    p.wait()
    assert n == total, f"decoded {n} frames, the project has {total}: not the file the project was made from"
    return out


def unit(v):
    return v / np.maximum(np.linalg.norm(v, axis=1, keepdims=True), 1e-9)


def write(path, frames, vectors):
    with open(path, "wb") as f:
        f.write(b"ASEM" + struct.pack("<II", len(frames), vectors.shape[1]))
        for frame, v in zip(frames, vectors.astype("<f4")):
            f.write(struct.pack("<I", frame) + v.tobytes())
    print(f"{path}: {len(frames)} frames x {vectors.shape[1]}")


def main():
    ffmpeg, video, project, nth, dino, places, out = sys.argv[1:8]
    rows, total = shots(project, int(nth))
    frames = wanted(rows)
    t = time.time()
    pictures = decode(ffmpeg, video, frames, total)
    print(f"{len(rows)} shots, {len(frames)} frames decoded in {time.time() - t:.0f} s")
    x = np.stack([pictures[f] for f in frames]).transpose(0, 3, 1, 2).astype(np.float32) / 255.0
    x = (x - MEAN) / STD

    t = time.time()
    net = ort.InferenceSession(dino, providers=["DmlExecutionProvider", "CPUExecutionProvider"])
    name = net.get_inputs()[0].name
    cls, patch = [], []
    for i in range(0, len(x), 16):
        tokens = net.run(None, {name: x[i:i + 16]})[0]  # [n, 1 + patches, 384]
        cls.append(tokens[:, 0])
        patch.append(tokens[:, 1:].mean(axis=1))
    print(f"DINOv2 on {net.get_providers()[0]}: {time.time() - t:.0f} s")
    write(out + ".dino_cls.emb", frames, unit(np.concatenate(cls)))
    write(out + ".dino_patch.emb", frames, unit(np.concatenate(patch)))

    siglip = os.environ.get("SIGLIP")
    if siglip:
        t = time.time()
        net = ort.InferenceSession(siglip, providers=["DmlExecutionProvider", "CPUExecutionProvider"])
        small = np.stack([np.asarray(Image.fromarray(pictures[f]).resize((384, 384), Image.BILINEAR)) for f in frames])
        y = (small.transpose(0, 3, 1, 2).astype(np.float32) / 255.0 - 0.5) / 0.5
        pooled = [net.run(["pooler_output"], {"pixel_values": y[i:i + 4]})[0] for i in range(0, len(y), 4)]
        print(f"SigLIP 2 on {net.get_providers()[0]}: {time.time() - t:.0f} s")
        write(out + ".siglip.emb", frames, unit(np.concatenate(pooled)))

    if places == "-":
        return
    t = time.time()
    saved = torch.load(places, map_location="cpu", weights_only=True)
    model = torchvision.models.resnet50(num_classes=365)
    model.load_state_dict({k.removeprefix("module."): v for k, v in saved["state_dict"].items()})
    model.fc = torch.nn.Identity()  # the 2048 numbers before the 365 place classes
    model.eval()
    with torch.inference_mode():
        feats = [model(torch.from_numpy(x[i:i + 16])).numpy() for i in range(0, len(x), 16)]
    print(f"Places365 ResNet-50 on the processor: {time.time() - t:.0f} s")
    write(out + ".places.emb", frames, unit(np.concatenate(feats)))


main()
