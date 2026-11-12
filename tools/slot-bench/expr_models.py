"""Ready-made facial-expression models over face crops: one table of scores per model.

    uv run expr_models.py [--top <dir>] [--emb <dir>] <models_root> <crops_dir> <out_dir> [model ...]

Step 1 of the expressions slot (`bench/models/DECISIONS.md`, row 5): what the
ready-made models say about our own face crops, before anything is trained.
`<models_root>` holds the model folders of `bench/models/data.tsv`
(`hsemotion/`, `fmae/`, ...); `<crops_dir>` is what face_crops.py wrote. Each
model writes `<out_dir>/<model>.tsv`: a comment line saying what the numbers
are and how the crop was fed, then `file` and one column per output, raw as
the model gives them. With `--top`, every expression output also gets a
contact sheet of the 30 crops it ranks highest (`<model>__<output>.jpg`, the
score under each: probabilities for the classifiers, the positive prompt's
cosine minus the neutral one's for SigLIP). With `--emb`, the models that have
a picture embedding also write it, one row per crop (`<model>.npy`: FMAE's
pooled features, SigLIP's picture embedding, and `dinov2_small`, which has no
expression output at all and runs only when named: its class token and the
mean of its patch tokens), for expr_probe.py. Without model names, all others
run; a model that cannot be loaded stops the run. `DEVICE=cpu` keeps the ONNX
models off the graphics card.

ONNX models run on the graphics card (DirectML; Kagami's graph is changed in
memory, see `without_allowzero`); FMAE runs in PyTorch on the processor.
The FMAE pickles are loaded weights-only, with `argparse.Namespace`
(their `args` entry) allowed by name. py-feat's face_multitask_v2 is not
here: its AU graph layer and its mesh-to-emotion path cannot be rebuilt from
the tensor names and shapes in the safetensors file.
"""
import argparse
import csv
import os
import time

import numpy as np
import onnx
import onnxruntime as ort
import torch
from PIL import Image, ImageDraw, ImageFont
from tokenizers import Tokenizer

IMAGENET = np.array([0.485, 0.456, 0.406], np.float32), np.array([0.229, 0.224, 0.225], np.float32)
# HSEmotion's class order (its ImageFolder names, sorted).
HSEMOTION = ["Anger", "Contempt", "Disgust", "Fear", "Happiness", "Neutral", "Sadness", "Surprise"]
# FMAE's AffectNet-8 head: AffectNet's own class numbers 0 to 7 (on our crops each column
# follows HSEmotion's class of the same name best). BP4D's 12 AUs in their usual order.
AFFECTNET = ["Neutral", "Happy", "Sad", "Surprise", "Fear", "Disgust", "Anger", "Contempt"]
BP4D = ["AU01", "AU02", "AU04", "AU06", "AU07", "AU10", "AU12", "AU14", "AU15", "AU17", "AU23", "AU24"]
TAGS = [
    "smile", "light_smile", "grin", ":d", ";d", "^_^", "happy", "laughing", "happy_tears",
    "crying", "crying_with_eyes_open", "tears", "streaming_tears", "tearing_up", "sobbing",
    "angry", "annoyed", "anger_vein", "glaring", "frown", "light_frown", "furrowed_brow", "v-shaped_eyebrows",
    "clenched_teeth", "sad", "depressed", "worried", "surprised", "wide-eyed", ":o",
    "scared", "panicking", "turn_pale", "shaded_face", "trembling", "nervous", "shouting", "screaming",
    "blush", "light_blush", "nose_blush", "full-face_blush", "embarrassed",
    "open_mouth", "closed_mouth", "closed_eyes", "half-closed_eyes", ">_<", "wavy_mouth", "teeth",
]
# label: (positive prompt, neutral prompt); SigLIP 2 was trained on lower-case text.
PROMPTS = {
    "smiling": ("a photo of a person smiling", "a photo of a person with a straight, relaxed mouth"),
    "laughing": ("a photo of a person laughing", "a photo of a person with a calm, quiet face"),
    "crying": ("a photo of a person crying", "a photo of a person with a calm face and dry eyes"),
    "angry_look": ("a photo of a person with an angry look", "a photo of a person with a calm, friendly look"),
    "shouting": ("a photo of a person shouting", "a photo of a person keeping quiet"),
    "surprised": ("a photo of a person looking surprised", "a photo of a person with a calm, unsurprised look"),
    "downcast": ("a photo of a person looking downcast", "a photo of a person with a neutral, attentive look"),
    "blushing": ("a photo of a person blushing", "a photo of a person with an even skin tone on the cheeks"),
    "eyes_closed": ("a photo of a person with closed eyes", "a photo of a person with open eyes"),
    "mouth_open": ("a photo of a person with an open mouth", "a photo of a person with a closed mouth"),
}
COLS, ROWS, CELL, STRIP = 6, 5, 160, 20
FILE_SAFE = str.maketrans({":": "colon_", ";": "semicolon_", "^": "caret", ">": "gt", "<": "lt"})


def softmax(z):
    e = np.exp(z - z.max(1, keepdims=True))
    return e / e.sum(1, keepdims=True)


def sigmoid(z):
    return 1 / (1 + np.exp(-z))


def session(graph, dims, disabled=(), cpu=False):
    """An ONNX Runtime session (a file or its bytes) on the graphics card or the processor, free dimensions fixed."""
    so = ort.SessionOptions()
    so.log_severity_level = 3
    so.enable_mem_pattern = False
    so.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    for name, size in dims.items():
        so.add_free_dimension_override_by_name(name, size)
    cpu = cpu or os.environ.get("DEVICE") == "cpu"
    providers = ["CPUExecutionProvider"] if cpu else ["DmlExecutionProvider", "CPUExecutionProvider"]
    s = ort.InferenceSession(graph, so, providers=providers, disabled_optimizers=list(disabled))
    assert cpu or s.get_providers()[0] == "DmlExecutionProvider", "DirectML did not load"
    return s


def without_allowzero(path):
    """The graph with Reshape's `allowzero` off, which DirectML rejects; Kagami's shapes hold no 0, so nothing changes."""
    m = onnx.load(path)
    for n in m.graph.node:
        for a in n.attribute:
            if n.op_type == "Reshape" and a.name == "allowzero":
                a.i = 0
    return m.SerializeToString()


def square(pic, box, grow):
    """A square `grow` times the face box's longer side around its centre (black outside the crop)."""
    x1, y1, x2, y2 = (v * pic.width for v in box)
    half, cx, cy = grow * max(x2 - x1, y2 - y1) / 2, (x1 + x2) / 2, (y1 + y2) / 2
    return pic.crop((round(cx - half), round(cy - half), round(cx + half), round(cy + half)))


class Model:
    batch, device = 16, "graphics card (DirectML)"
    embedding = None  # the last batch's picture embeddings, for the models that have one

    def sheets(self, s):
        return {c: s[:, i] for i, c in enumerate(self.columns)}


class Hsemotion(Model):
    def __init__(self, path, size, va):
        self.session, self.size = session(path, {"batch_size": self.batch}), size
        self.columns = HSEMOTION + (["valence", "arousal"] if va else [])
        self.note = (f"{os.path.basename(path)}: logits of 8 emotions (softmax gives probabilities)"
                     f"{', then valence and arousal as given' if va else ''}; input: the face box as found,"
                     f" stretched to {size}x{size}, RGB, ImageNet mean/std")

    def prepare(self, pic, box):
        x1, y1, x2, y2 = (v * pic.width for v in box)
        face = pic.crop((round(x1), round(y1), round(x2), round(y2))).resize((self.size, self.size), Image.BILINEAR)
        return ((np.asarray(face, np.float32) / 255 - IMAGENET[0]) / IMAGENET[1]).transpose(2, 0, 1)

    def run(self, x):
        return self.session.run(None, {"input": x})[0]

    def sheets(self, s):
        p = softmax(s[:, :8])
        return {c: p[:, i] for i, c in enumerate(HSEMOTION)} | {c: s[:, 8 + i] for i, c in enumerate(self.columns[8:])}


class Block(torch.nn.Module):
    def __init__(self, dim, heads=16):
        super().__init__()
        self.heads = heads
        self.norm1, self.norm2 = torch.nn.LayerNorm(dim, eps=1e-6), torch.nn.LayerNorm(dim, eps=1e-6)
        self.attn, self.mlp = torch.nn.Module(), torch.nn.Module()
        self.attn.qkv, self.attn.proj = torch.nn.Linear(dim, 3 * dim), torch.nn.Linear(dim, dim)
        self.mlp.fc1, self.mlp.fc2 = torch.nn.Linear(dim, 4 * dim), torch.nn.Linear(4 * dim, dim)

    def forward(self, x):
        b, n, d = x.shape
        q, k, v = self.attn.qkv(self.norm1(x)).reshape(b, n, 3, self.heads, d // self.heads).permute(2, 0, 3, 1, 4)
        x = x + self.attn.proj(torch.nn.functional.scaled_dot_product_attention(q, k, v).transpose(1, 2).reshape(b, n, d))
        return x + self.mlp.fc2(torch.nn.functional.gelu(self.mlp.fc1(self.norm2(x))))


class Vit(torch.nn.Module):
    """MAE's fine-tuning ViT-L/16 at 224 px: 24 blocks, the mean of the patch tokens, fc_norm, head."""

    def __init__(self, classes, dim=1024):
        super().__init__()
        self.patch_embed = torch.nn.Module()
        self.patch_embed.proj = torch.nn.Conv2d(3, dim, 16, 16)
        self.cls_token = torch.nn.Parameter(torch.zeros(1, 1, dim))
        self.pos_embed = torch.nn.Parameter(torch.zeros(1, 197, dim))
        self.blocks = torch.nn.ModuleList(Block(dim) for _ in range(24))
        self.fc_norm = torch.nn.LayerNorm(dim, eps=1e-6)
        self.head = torch.nn.Linear(dim, classes)

    def forward(self, x):
        x = self.patch_embed.proj(x).flatten(2).transpose(1, 2)
        x = torch.cat([self.cls_token.expand(len(x), -1, -1), x], 1) + self.pos_embed
        for block in self.blocks:
            x = block(x)
        pooled = self.fc_norm(x[:, 1:].mean(1))
        return self.head(pooled), pooled


class Fmae(Model):
    batch, device = 8, "processor (PyTorch)"

    def __init__(self, path, columns, squash, what):
        with torch.serialization.safe_globals([argparse.Namespace]):
            state = torch.load(path, map_location="cpu", weights_only=True)["model"]
        self.net = Vit(len(columns)).eval()
        self.net.load_state_dict({k: v for k, v in state.items() if not k.startswith("ID_head.")})  # IAT's identity head
        self.columns, self.squash = columns, squash
        self.note = (f"{os.path.basename(path)}: {what}; input: a square 1.2 times the face box, resized to 256"
                     f" (bicubic), the middle 224, RGB, ImageNet mean/std")

    def prepare(self, pic, box):
        face = square(pic, box, 1.2).resize((256, 256), Image.BICUBIC).crop((16, 16, 240, 240))
        return ((np.asarray(face, np.float32) / 255 - IMAGENET[0]) / IMAGENET[1]).transpose(2, 0, 1)

    def run(self, x):
        with torch.inference_mode():
            scores, pooled = self.net(torch.from_numpy(x))
        self.embedding = pooled.numpy()
        return scores.numpy()

    def sheets(self, s):
        p = self.squash(s)
        return {c: p[:, i] for i, c in enumerate(self.columns)}


class Tagger(Model):
    batch = 8

    def __init__(self, graph, tags, dims, feed):
        self.session = session(graph, dims)
        names = [r["name"] for r in csv.DictReader(open(tags, encoding="utf-8"))]
        self.columns = [t for t in TAGS if t in names]
        self.keep = [names.index(t) for t in self.columns]
        missing = [t for t in TAGS if t not in names]
        self.note = (f"{os.path.basename(os.path.dirname(tags))}: probabilities (sigmoid inside the graph) of the"
                     f" tags below; input: the whole crop resized to 448 (bicubic), {feed}"
                     f"{'; not in this vocabulary: ' + ' '.join(missing) if missing else ''}")

    def run(self, x):
        return self.session.run(None, {"input": x})[0][:, self.keep]


class Kagami(Tagger):
    def prepare(self, pic, box):
        x = np.asarray(pic.resize((448, 448), Image.BICUBIC), np.float32) / 255 * 2 - 1
        return x[:, :, ::-1].transpose(2, 0, 1)


class Wd(Tagger):
    def prepare(self, pic, box):
        return np.asarray(pic.resize((448, 448), Image.BICUBIC), np.float32)[:, :, ::-1]


class Siglip(Model):
    def __init__(self, folder):
        tokens = Tokenizer.from_file(os.path.join(folder, "tokenizer.json"))  # pads to 64 with <pad>, after <eos>
        ids = np.array([e.ids for e in tokens.encode_batch([p.lower() for pair in PROMPTS.values() for p in pair])], np.int64)
        assert ids.shape[1] == 64, ids.shape
        text = session(os.path.join(folder, "onnx", "text_model_fp16.onnx"), {}, ["SimplifiedLayerNormFusion"], cpu=True)
        t = text.run(["pooler_output"], {"input_ids": ids})[0].astype(np.float32)
        self.text = t / np.linalg.norm(t, axis=1, keepdims=True)
        self.session = session(os.path.join(folder, "onnx", "vision_model.onnx"),
                               {"batch_size": self.batch, "num_channels": 3, "height": 384, "width": 384})
        self.columns = [f"{label}{end}" for label in PROMPTS for end in ("", "_neutral")]
        self.note = ("siglip2-so400m: cosine between the picture's and each prompt's embedding (pooler outputs,"
                     " L2-normalised), one column per prompt (see PROMPTS in expr_models.py); input: the whole"
                     " crop resized to 384 (bilinear), RGB, mean/std 0.5")

    def prepare(self, pic, box):
        return (np.asarray(pic.resize((384, 384), Image.BILINEAR), np.float32) / 255 * 2 - 1).transpose(2, 0, 1)

    def run(self, x):
        v = self.session.run(["pooler_output"], {"pixel_values": x})[0]
        self.embedding = v / np.linalg.norm(v, axis=1, keepdims=True)
        return self.embedding @ self.text.T

    def sheets(self, s):
        return {label: s[:, 2 * i] - s[:, 2 * i + 1] for i, label in enumerate(PROMPTS)}


class Dino(Model):
    """DINOv2-small: no expression output, only its embedding (class token, then the mean of the patch tokens)."""
    columns = []

    def __init__(self, path):
        self.session = session(path, {})

    def prepare(self, pic, box):
        x = np.asarray(pic.resize((224, 224), Image.BICUBIC), np.float32) / 255
        return ((x - IMAGENET[0]) / IMAGENET[1]).transpose(2, 0, 1)

    def run(self, x):
        tokens = self.session.run(["last_hidden_state"], {"pixel_values": x})[0]
        self.embedding = np.concatenate([tokens[:, 0], tokens[:, 1:].mean(1)], 1)
        return np.zeros((len(x), 0), np.float32)


MODELS = {
    "enet_b2_8": lambda r: Hsemotion(os.path.join(r, "hsemotion", "enet_b2_8.onnx"), 260, False),
    "enet_b0_8_va_mtl": lambda r: Hsemotion(os.path.join(r, "hsemotion", "enet_b0_8_va_mtl.onnx"), 224, True),
    "fmae_affectnet8": lambda r: Fmae(os.path.join(r, "fmae", "FMAE_AffectNet8.pth"), AFFECTNET, softmax,
                                      "logits of 8 emotions (softmax gives probabilities)"),
    "fmae_iat_bp4d": lambda r: Fmae(os.path.join(r, "fmae", "FMAE_IAT_BP4D_fold1.pth"), BP4D, sigmoid,
                                    "logits of 12 BP4D action units (sigmoid gives probabilities)"),
    "kagami_24k": lambda r: Kagami(without_allowzero(os.path.join(r, "kagami-24k", "onnx", "model_prob.onnx")),
                                   os.path.join(r, "kagami-24k", "selected_tags.csv"), {"batch": Tagger.batch},
                                   "NCHW, BGR, (x/255 - 0.5)/0.5"),
    "wd_eva02_large_v3": lambda r: Wd(os.path.join(r, "wd-eva02-large-v3", "model.onnx"),
                                      os.path.join(r, "wd-eva02-large-v3", "selected_tags.csv"),
                                      {"batch_size": Tagger.batch}, "NHWC, BGR, 0-255"),
    "siglip2_so400m": lambda r: Siglip(os.path.join(r, "siglip2-so400m")),
    "dinov2_small": lambda r: Dino(os.path.join(r, "..", "scenes", "dinov2-small", "onnx", "model.onnx")),
}


def score(model, crops_dir, rows):
    out, emb = [], []
    for i in range(0, len(rows), model.batch):
        chunk = rows[i:i + model.batch]
        x = np.stack([model.prepare(Image.open(os.path.join(crops_dir, r["file"])).convert("RGB"),
                                    [float(r[k]) for k in ("x1", "y1", "x2", "y2")]) for r in chunk]).astype(np.float32)
        if len(x) < model.batch:  # the shapes stay fixed on the graphics card
            x = np.concatenate([x, np.repeat(x[-1:], model.batch - len(x), 0)])
        out.append(model.run(x)[:len(chunk)])
        if model.embedding is not None:
            emb.append(model.embedding[:len(chunk)])
    return np.concatenate(out), (np.concatenate(emb) if emb else None)


def contact_sheets(model, name, scores, rows, crops_dir, top):
    os.makedirs(top, exist_ok=True)
    font = ImageFont.load_default(size=14)
    for output, values in model.sheets(scores).items():
        sheet = Image.new("RGB", (COLS * CELL, ROWS * (CELL + STRIP)), (16, 16, 16))
        draw = ImageDraw.Draw(sheet)
        for k, i in enumerate(np.argsort(-values, kind="stable")[:COLS * ROWS]):
            x, y = (k % COLS) * CELL, (k // COLS) * (CELL + STRIP)
            sheet.paste(Image.open(os.path.join(crops_dir, rows[i]["file"])).resize((CELL, CELL), Image.BILINEAR), (x, y))
            draw.text((x + 4, y + CELL + 2), f"{values[i]:.3f}  t{rows[i]['track']}", fill=(235, 235, 235), font=font)
        sheet.save(os.path.join(top, f"{name}__{output.translate(FILE_SAFE)}.jpg"), quality=85)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--top", help="folder for the contact sheets")
    ap.add_argument("--emb", help="folder for the picture embeddings")
    ap.add_argument("--only", help="a table whose `file` column names the crops to run (a check file); the others are left out")
    ap.add_argument("models_root")
    ap.add_argument("crops_dir")
    ap.add_argument("out_dir")
    ap.add_argument("models", nargs="*", choices=list(MODELS), default=[m for m in MODELS if m != "dinov2_small"])
    a = ap.parse_args()
    rows = list(csv.DictReader(open(os.path.join(a.crops_dir, "crops.tsv"), encoding="utf-8"), delimiter="\t"))
    if a.only:
        wanted = {r["file"] for r in csv.DictReader(open(a.only, encoding="utf-8"), delimiter="\t")}
        rows = [r for r in rows if r["file"] in wanted]
    os.makedirs(a.out_dir, exist_ok=True)
    for name in a.models:
        model = MODELS[name](a.models_root)
        start = time.perf_counter()
        scores, emb = score(model, a.crops_dir, rows)
        secs = time.perf_counter() - start
        if a.emb and emb is not None:
            os.makedirs(a.emb, exist_ok=True)
            np.save(os.path.join(a.emb, f"{name}.npy"), emb.astype(np.float32))
        if model.columns:
            with open(os.path.join(a.out_dir, f"{name}.tsv"), "w", encoding="utf-8", newline="\n") as f:
                f.write(f"# {model.note}\n" + "\t".join(["file"] + model.columns) + "\n")
                for r, s in zip(rows, scores):
                    f.write(r["file"] + "\t" + "\t".join(f"{v:.6g}" for v in s) + "\n")
        device = "processor" if os.environ.get("DEVICE") == "cpu" and "DirectML" in model.device else model.device
        print(f"{name}: {len(rows)} crops in {secs:.0f} s, {100 * secs / len(rows):.1f} s per 100 crops on the {device}")
        if a.top and model.columns:
            contact_sheets(model, name, scores, rows, a.crops_dir, a.top)
        del model


if __name__ == "__main__":
    main()
