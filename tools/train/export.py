"""A checkpoint of `train.py` as the ONNX graph the app would run: opset 17, any batch, 32-bit floats.

    uv run export.py <best.pt> <out.onnx> [<crops prefix>]

The graph must give what the network gives (the export gate of `bench/models/README.md`): cosine at least
0.99999 on the processor and 0.999 on DirectML, on noise and, with a prefix of `anime_crops`, on real heads.
"""
import hashlib
import sys
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
import torch

import crops
import model

BATCH = 8


def for_directml(graph):
    """DirectML rejects `Reshape(allowzero=1)`; with no 0 in a constant target shape it means the same as the
    default, so it goes (as `tools/model-prep/berkeley.py` does)."""
    consts = {i.name: onnx.numpy_helper.to_array(i) for i in graph.graph.initializer}
    for n in graph.graph.node:
        if n.op_type == "Reshape" and any(a.name == "allowzero" and a.i == 1 for a in n.attribute):
            if n.input[1] in consts and 0 in consts[n.input[1]].tolist():
                sys.exit(f"{n.name}: Reshape to a shape that may hold 0; allowzero matters")
            keep = [a for a in n.attribute if a.name != "allowzero"]
            del n.attribute[:]
            n.attribute.extend(keep)


def cosine(a, b):
    return (a * b).sum(1) / (np.linalg.norm(a, axis=1) * np.linalg.norm(b, axis=1))


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    pt, out = sys.argv[1], Path(sys.argv[2])
    net = model.from_checkpoint(pt).eval()
    with torch.no_grad():
        torch.onnx.export(
            net, (torch.randn(2, 3, model.SIDE, model.SIDE),), str(out),
            input_names=["input"], output_names=["embedding"],
            dynamic_axes={"input": {0: "batch"}, "embedding": {0: "batch"}}, opset_version=17, dynamo=False,
        )
    graph = onnx.load(str(out))
    for_directml(graph)
    onnx.checker.check_model(graph)
    onnx.save(graph, str(out))

    torch.manual_seed(0)
    x = torch.randn(BATCH, 3, model.SIDE, model.SIDE)
    x[BATCH // 2 :] *= 0.1  # dull, low-contrast heads too
    tests = [("noise", x.numpy())]
    if len(sys.argv) > 3:
        pixels = crops.read(sys.argv[3])[3]
        pick = np.linspace(0, len(pixels) - 1, 4 * BATCH).astype(int)
        tests.append(("heads", crops.to_input(pixels[pick])))
    fixed = ort.SessionOptions()
    fixed.add_free_dimension_override_by_name("batch", BATCH)
    sessions = [
        ("processor", ort.InferenceSession(str(out), providers=["CPUExecutionProvider"]), 0.99999),
        ("DirectML", ort.InferenceSession(str(out), fixed, providers=["DmlExecutionProvider", "CPUExecutionProvider"]), 0.999),
    ]
    ok = True
    for what, x in tests:
        with torch.no_grad():
            want = net(torch.from_numpy(x)).numpy()
        for where, session, least in sessions:
            got = np.concatenate([session.run(None, {"input": x[i : i + BATCH]})[0] for i in range(0, len(x), BATCH)])
            cos = cosine(want, got)
            length = np.abs(np.linalg.norm(got, axis=1) / np.linalg.norm(want, axis=1) - 1)
            good = cos.min() >= least and length.max() <= (1e-4 if where == "processor" else 1e-2)
            ok &= bool(good)
            print(f"{what} on the {where} ({session.get_providers()[0]}): min cosine {cos.min():.7f}, max length error {length.max():.1e}{'' if good else '  FAILS'}")
    if not ok:
        sys.exit("the graph doesn't match the network")
    digest = hashlib.sha256(out.read_bytes()).hexdigest()
    print(f"{out}: {net.dim}-d, {out.stat().st_size} bytes, sha256 {digest}")


if __name__ == "__main__":
    main()
