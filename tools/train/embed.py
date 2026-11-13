"""Embed the head crops of the tuning projects' tracks with another embedder and write the tracks' vectors for
the identity bench (`EMB_OVERRIDE`): the embedder is scored on the pass's own tracks, without a pass of its own.

    uv run embed.py <model.onnx | checkpoint.pt> <out dir> [--crops <dir>] [--kind h4] [--cpu] [--batch <n>]

For every tuning project of `score.py` its episodes' `anime_crops` outputs (`<crops dir>/<episode>.<kind>`,
default `$TRAIN_WORK/crops`) are embedded in the project's own order of episodes — the order their tracks have
in the project's tracks file — and written as `<out dir>/<project>.emb`. An ONNX graph runs on DirectML, a
checkpoint of `train.py` on CUDA when there is one (`--cpu`: either on the processor). A track's vector is
made as the Best scan makes it (`crops.track_vectors`).
"""
import argparse
import time
from pathlib import Path

import numpy as np

import crops
import score


def onnx_embedder(path, cpu, batch):
    import onnxruntime as ort

    options = ort.SessionOptions()
    # A fixed batch: DirectML is slow or wrong on graphs whose batch is left open.
    options.add_free_dimension_override_by_name("batch", batch)
    providers = ["CPUExecutionProvider"] if cpu else ["DmlExecutionProvider", "CPUExecutionProvider"]
    session = ort.InferenceSession(path, options, providers=providers)
    name = session.get_inputs()[0].name
    print(f"{path}: on {session.get_providers()[0]}", flush=True)

    def run(x):
        n = len(x)
        if n < batch:
            x = np.concatenate([x, np.zeros((batch - n, *x.shape[1:]), dtype=x.dtype)])
        return session.run(None, {name: x})[0][:n]

    return run


def torch_embedder(path, cpu):
    import torch

    import model

    device = "cuda" if torch.cuda.is_available() and not cpu else "cpu"
    net = model.from_checkpoint(path).to(device).eval()
    print(f"{path}: on {device}", flush=True)

    def run(x):
        with torch.no_grad():
            return net(torch.from_numpy(x).to(device)).float().cpu().numpy()

    return run


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("model")
    p.add_argument("out")
    p.add_argument("--crops", default=str(score.WORK / "crops"))
    p.add_argument("--kind", default="h4")
    p.add_argument("--cpu", action="store_true")
    p.add_argument("--batch", type=int, default=8)
    a = p.parse_args()
    run = onnx_embedder(a.model, a.cpu, a.batch) if a.model.endswith(".onnx") else torch_embedder(a.model, a.cpu)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    t0, done = time.time(), 0
    for project, (episodes, _) in score.PROJECTS.items():
        parts = []
        for episode in episodes:
            tracks, track, quality, pixels, _ = crops.read(str(Path(a.crops) / f"{episode}.{a.kind}"))
            vectors = np.concatenate([run(crops.to_input(pixels[i : i + a.batch])) for i in range(0, len(pixels), a.batch)])
            parts.append(crops.track_vectors(tracks, track, quality, vectors))
            done += len(pixels)
        crops.write_override(out / f"{project}.emb", np.concatenate(parts))
        print(f"{project}: {sum(len(x) for x in parts)} tracks of {len(episodes)} episodes, {parts[0].shape[1]} values each", flush=True)
    print(f"{done / (time.time() - t0):.0f} crops a second")


if __name__ == "__main__":
    main()
