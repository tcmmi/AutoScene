"""Head crops of an anime pass's tracks (the `anime_crops` example of `as-analysis`) and the per-track vectors
the identity bench reads in place of a pass's own (`EMB_OVERRIDE`)."""
import struct

import numpy as np

CROP = 224
CROP_BYTES = CROP * CROP * 3
# ImageNet's, as `as_vision::berkeley` normalises a crop: (v / 255 - mean) / std.
MEAN = np.array([0.485, 0.456, 0.406], dtype=np.float32)
STD = np.array([0.229, 0.224, 0.225], dtype=np.float32)
# The least a head's quality weighs in its track's vector (`anime::identify`).
LEAST = 1e-3


def read(prefix):
    """`<prefix>.crops` and `.crops.tsv`: the number of tracks, each crop's track and quality, and the crops
    (`n × 224 × 224 × 3` bytes, mapped, not loaded)."""
    with open(prefix + ".crops.tsv", encoding="utf-8") as f:
        head = f.readline().split()
        assert head[:2] == ["#", "tracks"], head
        tracks = int(head[2])
        names = f.readline().rstrip("\n").split("\t")
        rows = [dict(zip(names, line.rstrip("\n").split("\t"))) for line in f if line.strip()]
    assert [int(r["crop"]) for r in rows] == list(range(len(rows)))
    track = np.array([int(r["track"]) for r in rows], dtype=np.int64)
    quality = np.array([float(r["quality"]) for r in rows], dtype=np.float32)
    pixels = np.memmap(prefix + ".crops", dtype=np.uint8, mode="r", shape=(len(rows), CROP, CROP, 3))
    return tracks, track, quality, pixels, rows


def to_input(pixels):
    """Crops as bytes (`n × 224 × 224 × 3`) to the network's input (`n × 3 × 224 × 224`, normalised)."""
    x = (pixels.astype(np.float32) / 255.0 - MEAN) / STD
    return np.ascontiguousarray(x.transpose(0, 3, 1, 2))


def track_vectors(tracks, track, quality, vectors):
    """A track's identity as the Best scan makes it: its heads' unit vectors, each weighted by its quality,
    summed and made unit length. A track without a crop has no direction (zeros)."""
    unit = vectors / np.maximum(np.linalg.norm(vectors, axis=1, keepdims=True), 1e-12)
    out = np.zeros((tracks, vectors.shape[1]), dtype=np.float32)
    np.add.at(out, track, unit * np.maximum(quality, LEAST)[:, None])
    length = np.linalg.norm(out, axis=1, keepdims=True)
    return np.where(length > 0, out / np.maximum(length, 1e-12), out).astype(np.float32)


def write_override(path, vectors):
    """`EMB_OVERRIDE`: `u32 n, u32 dim`, then `n × dim` f32, little-endian, in the tracks' order."""
    vectors = np.ascontiguousarray(vectors, dtype="<f4")
    with open(path, "wb") as f:
        f.write(struct.pack("<II", *vectors.shape))
        f.write(vectors.tobytes())


def read_override(path):
    with open(path, "rb") as f:
        n, dim = struct.unpack("<II", f.read(8))
        return np.frombuffer(f.read(), dtype="<f4").reshape(n, dim)
