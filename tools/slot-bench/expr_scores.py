"""What expr_models.py wrote, as one number per crop and output: read by expr_select.py and expr_check.py.

A classifier's logits become probabilities (softmax over its 8 emotions,
sigmoid for the action units), the taggers' probabilities stay, valence and
arousal stay, and SigLIP's two cosines per label become their difference
(the label's prompt minus its neutral one). An output's name is
`<model>:<column>`.
"""
import csv
import os

import numpy as np

MODELS = ["enet_b2_8", "enet_b0_8_va_mtl", "fmae_affectnet8", "fmae_iat_bp4d", "kagami_24k", "wd_eva02_large_v3",
          "siglip2_so400m"]
# The outputs that could stand for a label, by their names: the check set is drawn from their
# highest tracks, and expr_check.py scores every output against every label anyway.
CANDIDATES = {
    "smile": ["enet_b2_8:Happiness", "enet_b0_8_va_mtl:Happiness", "fmae_affectnet8:Happy", "fmae_iat_bp4d:AU12",
              "kagami_24k:smile", "wd_eva02_large_v3:smile", "siglip2_so400m:smiling"],
    "laugh": ["siglip2_so400m:laughing", "kagami_24k:laughing", "wd_eva02_large_v3:laughing", "kagami_24k::d",
              "wd_eva02_large_v3::d", "fmae_iat_bp4d:AU06"],
    "cry": ["siglip2_so400m:crying", "kagami_24k:crying", "wd_eva02_large_v3:crying", "kagami_24k:tears",
            "wd_eva02_large_v3:tears"],
    "angry": ["enet_b2_8:Anger", "enet_b0_8_va_mtl:Anger", "fmae_affectnet8:Anger", "fmae_iat_bp4d:AU04",
              "kagami_24k:angry", "wd_eva02_large_v3:angry", "siglip2_so400m:angry_look"],
    "shout": ["siglip2_so400m:shouting", "kagami_24k:shouting", "wd_eva02_large_v3:shouting", "kagami_24k:screaming",
              "wd_eva02_large_v3:screaming", "enet_b0_8_va_mtl:arousal"],
    "surprised": ["enet_b2_8:Surprise", "enet_b0_8_va_mtl:Surprise", "fmae_affectnet8:Surprise", "fmae_affectnet8:Fear",
                  "kagami_24k:surprised", "wd_eva02_large_v3:surprised", "kagami_24k:wide-eyed",
                  "siglip2_so400m:surprised"],
    "downcast": ["enet_b2_8:Sadness", "enet_b0_8_va_mtl:Sadness", "fmae_affectnet8:Sad", "fmae_iat_bp4d:AU15",
                 "kagami_24k:sad", "wd_eva02_large_v3:sad", "siglip2_so400m:downcast"],
    "blush": ["kagami_24k:blush", "wd_eva02_large_v3:blush", "siglip2_so400m:blushing"],
    "eyes_closed": ["kagami_24k:closed_eyes", "wd_eva02_large_v3:closed_eyes", "siglip2_so400m:eyes_closed"],
    "mouth_open": ["kagami_24k:open_mouth", "wd_eva02_large_v3:open_mouth", "siglip2_so400m:mouth_open"],
}
LABELS = list(CANDIDATES)


def crops(crops_dir):
    """The rows of face_crops.py's `crops.tsv`."""
    return list(csv.DictReader(open(os.path.join(crops_dir, "crops.tsv"), encoding="utf-8"), delimiter="\t",
                               quoting=csv.QUOTE_NONE))


def table(path):
    lines = [line.rstrip("\n").split("\t") for line in open(path, encoding="utf-8") if not line.startswith("#")]
    return lines[0][1:], [r[0] for r in lines[1:]], np.array([r[1:] for r in lines[1:]], np.float64)


def softmax(z):
    e = np.exp(z - z.max(1, keepdims=True))
    return e / e.sum(1, keepdims=True)


def outputs(scores_dir, files):
    """{`model:column`: one value per crop of `files`}, for every model with a table in `scores_dir`."""
    out = {}
    for model in MODELS:
        path = os.path.join(scores_dir, f"{model}.tsv")
        if not os.path.exists(path):
            continue
        columns, rows, s = table(path)
        assert rows == files, f"{path}: not the crops of crops.tsv, or not in their order"
        if model.startswith("enet") or model == "fmae_affectnet8":
            s = np.concatenate([softmax(s[:, :8]), s[:, 8:]], 1)
        elif model == "fmae_iat_bp4d":
            s = 1 / (1 + np.exp(-s))
        elif model == "siglip2_so400m":
            columns, s = columns[::2], s[:, ::2] - s[:, 1::2]
        for i, c in enumerate(columns):
            out[f"{model}:{c}"] = s[:, i]
    return out
