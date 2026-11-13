"""The embedder being trained: a DINOv2 vision transformer whose class token is a head's identity, and the
ArcFace classifier it is trained with (Bamman et al. 2026's recipe; DINOv2's weights are Apache-2.0)."""
import math

import timm
import torch
import torch.nn.functional as F
from torch import nn

ARCH = {
    "vits14": ("vit_small_patch14_dinov2.lvd142m", 384),
    "vitb14": ("vit_base_patch14_dinov2.lvd142m", 768),
}
SIDE = 224


class Embedder(nn.Module):
    """A batch of crops (`n × 3 × 224 × 224`, ImageNet-normalised) to their identities: the class token after
    the last norm, through a linear projection when `dim` isn't the backbone's own width (0: none). The output
    isn't unit length: the app makes it so. Its size must be one no embedder of the app's tracks has (128
    SFace, 512 ArcFace, 768 CCIP, 1024 Berkeley): the small backbone's own 384 is free there, the base one's
    768 is not (`--dim 640`)."""

    def __init__(self, arch, dim=0, pretrained=False):
        super().__init__()
        name, width = ARCH[arch]
        self.arch, self.dim = arch, dim or width
        self.backbone = timm.create_model(name, pretrained=pretrained, num_classes=0, img_size=SIDE)
        self.proj = nn.Linear(width, dim) if dim and dim != width else None

    def forward(self, x):
        x = self.backbone(x)
        return x if self.proj is None else self.proj(x)


class ArcFace(nn.Module):
    """Additive angular margin over the training identities (Deng et al. 2019): scale 64, margin 0.5."""

    def __init__(self, dim, classes, scale=64.0, margin=0.5):
        super().__init__()
        self.weight = nn.Parameter(torch.empty(classes, dim))
        nn.init.xavier_uniform_(self.weight)
        self.scale, self.margin = scale, margin

    def forward(self, features, labels):
        cosine = F.normalize(features.float(), dim=1) @ F.normalize(self.weight, dim=1).t()
        sine = torch.sqrt((1.0 - cosine.pow(2)).clamp(1e-9, 1.0))
        phi = cosine * math.cos(self.margin) - sine * math.sin(self.margin)
        # Past pi - margin the margin would lower the angle's cosine again: a straight penalty there.
        phi = torch.where(cosine > math.cos(math.pi - self.margin), phi, cosine - math.sin(math.pi - self.margin) * self.margin)
        one_hot = F.one_hot(labels, cosine.shape[1]).to(cosine.dtype)
        return self.scale * (one_hot * phi + (1.0 - one_hot) * cosine)


def from_checkpoint(path):
    """The embedder of a checkpoint `train.py` wrote (weights only: nothing pickled runs)."""
    state = torch.load(path, map_location="cpu", weights_only=True)
    net = Embedder(state["arch"], state["dim"])
    net.load_state_dict(state["model"])
    return net
