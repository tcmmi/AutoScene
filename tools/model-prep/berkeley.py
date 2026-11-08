"""Berkeley's animated-character embedder → ONNX, for AutoScene's anime pass.

Bamman et al. ("Measuring Gender Representation in Animated Films", 2026;
github.com/bamman-group/animated-gender-representation) fine-tuned DINOv2
ViT-L/14 to tell drawn and rendered characters apart (trained on
iCartoonFace and WIDER Face). The checkpoint is `dino_vitl14_crop25.FINAL.pth`
from their pipeline (`models/`); the network is DINOv2's own definition
(a checkout of github.com/facebookresearch/dinov2, Apache-2.0), loaded with
the checkpoint's backbone tensors (`weights_only`: nothing pickled runs).

Input: the head box padded by a quarter of its size on each side, resized
to 224×224, RGB, NCHW, ImageNet mean/std. Output: the 1024-d CLS feature
(its length says how clear the head is).

    python berkeley.py <dino_vitl14_crop25.FINAL.pth> <dinov2 checkout> <out.onnx>
"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
import torch

PTH_SHA256 = None  # printed on first use; pin it in models/manifest.toml's source note


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def load(pth: Path, dinov2: Path) -> torch.nn.Module:
    st = torch.load(pth, map_location="cpu", weights_only=True)
    assert st["proj_dim"] == 0, "a projection head isn't part of this export"
    net = torch.hub.load(str(dinov2), f"dinov2_{st['arch']}", source="local", pretrained=False)
    backbone = {k.removeprefix("backbone."): v for k, v in st["model"].items() if k.startswith("backbone.")}
    missing, unexpected = net.load_state_dict(backbone, strict=False)
    # DINOv2's mask token is only used in training.
    assert set(missing) <= {"mask_token"} and not unexpected, (missing, unexpected)
    return Features(net).eval()


class Features(torch.nn.Module):
    """The CLS feature of a batch of crops (DINOv2's forward also takes
    training masks, which the export must not see)."""

    def __init__(self, net: torch.nn.Module) -> None:
        super().__init__()
        self.net = net

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        return self.net.forward_features(x)["x_norm_clstoken"]


def for_directml(model: onnx.ModelProto) -> None:
    """DirectML rejects `Reshape(allowzero=1)`; with no 0 in a constant
    target shape it means the same as the default, so it goes."""
    consts = {i.name: onnx.numpy_helper.to_array(i) for i in model.graph.initializer}
    for n in model.graph.node:
        if n.op_type == "Reshape" and any(a.name == "allowzero" and a.i == 1 for a in n.attribute):
            if n.input[1] in consts and 0 in consts[n.input[1]].tolist():
                sys.exit(f"{n.name}: Reshape to a shape that may hold 0; allowzero matters")
            keep = [a for a in n.attribute if a.name != "allowzero"]
            del n.attribute[:]
            n.attribute.extend(keep)


def main() -> None:
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    pth, dinov2, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    print(f"{pth}: sha256 {sha256(pth)}")
    net = load(pth, dinov2)
    example = torch.randn(2, 3, 224, 224)
    with torch.no_grad():
        torch.onnx.export(
            net,
            (example,),
            str(out),
            input_names=["input"],
            output_names=["embedding"],
            dynamic_axes={"input": {0: "batch"}, "embedding": {0: "batch"}},
            opset_version=17,
            dynamo=False,
        )
    model = onnx.load(str(out))
    for_directml(model)
    onnx.checker.check_model(model)
    onnx.save(model, str(out))

    # The graph must give what the network gives, crops of any look.
    torch.manual_seed(0)
    x = torch.randn(6, 3, 224, 224)
    x[3:] *= 0.1  # dull, low-contrast heads too
    with torch.no_grad():
        want = net(x).numpy()
    got = ort.InferenceSession(str(out), providers=["CPUExecutionProvider"]).run(None, {"input": x.numpy()})[0]
    cos = (want * got).sum(1) / (np.linalg.norm(want, axis=1) * np.linalg.norm(got, axis=1))
    rel = np.abs(np.linalg.norm(got, axis=1) / np.linalg.norm(want, axis=1) - 1)
    print(f"parity: min cosine {cos.min():.7f}, max norm error {rel.max():.2e}")
    if cos.min() < 0.99999 or rel.max() > 1e-4:
        sys.exit("ONNX graph doesn't match the network")
    print(f"{out}: {out.stat().st_size} bytes, sha256 {sha256(out)}")


if __name__ == "__main__":
    main()
