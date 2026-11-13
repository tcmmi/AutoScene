"""iCartoonFace's recognition training set, read straight from its archive: one folder per identity, a face box
per image. A crop is made as the app makes a head's: the box padded by a quarter on each side, clipped to the
picture, resized to 224 × 224 without keeping its shape."""
import hashlib
import io
import random
import zipfile

import numpy as np
import torch
from PIL import Image
from torchvision import transforms

import crops

ROOT = "personai_icartoonface_rectrain/"
PAD = 0.25


def index(path, heads=None, drop=None):
    """Every image of the archive: `(member, identity, box)` — the box is the set's face box, or with
    `heads` (the table of `heads.py`) the head the app's detector finds there (`with_heads`). `drop` names a
    table whose first column lists pictures to leave out (strangers in their identity; `#` lines skipped)."""
    with zipfile.ZipFile(path) as z:
        lines = z.read(ROOT + "icartoonface_rectrain_det.txt").decode("utf-8").splitlines()
    rows = []
    for line in lines:
        part = line.split()
        if len(part) == 5:
            rows.append((ROOT + "icartoonface_rectrain/" + part[0], part[0].split("/")[0], tuple(int(v) for v in part[1:])))
    if drop:
        with open(drop, encoding="utf-8") as f:
            out = {line.split()[0] for line in f if line.strip() and not line.startswith("#")}
        rows = [r for r in rows if r[0].rsplit("/", 1)[-1] not in out]
    return with_heads(rows, heads) if heads else rows


def read_exclude(path):
    """The identities that never train: the first column of a table (`#` lines and a header `id` skipped),
    as five digits or as the archive's folder names. Several tables: their paths with commas between."""
    out = set()
    for one in (path or "").split(","):
        if not one:
            continue
        with open(one, encoding="utf-8") as f:
            for line in f:
                first = line.split("\t")[0].strip()
                if first and not first.startswith("#") and first != "id":
                    out.add(f"personai_icartoonface_rectrain_{int(first):05d}" if first.isdigit() else first)
    return out


def place(name, salt):
    """A number an identity keeps whatever else is in the set: from its name alone."""
    return int.from_bytes(hashlib.sha256(f"{salt}:{name}".encode()).digest()[:8], "big")


def split(rows, exclude, limit, dev_fraction, seed, fold=None):
    """The identities that train and the ones held out for picking a checkpoint (never a bench franchise's:
    `exclude`). Which side an identity is on depends on its name and the seed alone, so a longer exclusion
    list moves no identity from one side to the other. `limit` keeps only so many identities (0: all), for a
    dry run; `fold = (k, n)` only one identity in n: the others are judged by a model that never saw them
    (`mixed.py`)."""
    names = sorted({r[1] for r in rows} - set(exclude), key=lambda n: place(n, f"order{seed}"))
    if fold:
        names = [n for n in names if place(n, f"fold{seed}") % fold[1] == fold[0]]
    if limit:
        names = names[:limit]
    dev = [n for n in names if place(n, f"dev{seed}") % 10_000 < dev_fraction * 10_000] or names[:1]
    return sorted(set(names) - set(dev)), sorted(dev)


def with_heads(rows, path):
    """The rows of `index` with the head box the app's detector found in each picture (`heads.py`) in place
    of the set's face box, which is far tighter than what the app crops: where no head was found, the face
    box widened as a head is around a face on average (measured on the pictures with one: 0.48 face heights
    above, 0.07 below, 0.4 face widths on each side)."""
    found = {}
    with open(path, encoding="utf-8") as f:
        next(f)
        for line in f:
            part = line.rstrip("\n").split("\t")
            if part[1] != "-":
                found[part[0]] = tuple(float(v) for v in part[1:5])
    out = []
    for member, name, face in rows:
        fw, fh = face[2] - face[0], face[3] - face[1]
        wide = (face[0] - 0.4 * fw, face[1] - 0.48 * fh, face[2] + 0.4 * fw, face[3] + 0.07 * fh)
        out.append((member, name, found.get(member.rsplit("/", 1)[-1], wide)))
    return out


def crop(image, box):
    """The head crop of a picture, as `as_vision::berkeley::crop` makes it (with PIL's sampling, which sits
    up to a source pixel beside the app's where the box starts between two pixels)."""
    w, h = image.size
    bw, bh = box[2] - box[0], box[3] - box[1]
    area = (max(0.0, box[0] - PAD * bw), max(0.0, box[1] - PAD * bh), min(float(w), box[2] + PAD * bw), min(float(h), box[3] + PAD * bh))
    return image.resize((crops.CROP, crops.CROP), Image.BILINEAR, box=area)


class Shrink:
    """What a small or soft head in a video looks like: a share of the crops is made as small as `least` to
    `most` pixels and back, half of those through a JPEG of low quality on the way."""

    def __init__(self, share, least=20, most=112):
        self.share, self.least, self.most = share, least, most

    def __call__(self, image):
        if random.random() >= self.share:
            return image
        side = random.randint(self.least, self.most)
        small = image.resize((side, side), Image.BILINEAR)
        if random.random() < 0.5:
            data = io.BytesIO()
            small.save(data, "JPEG", quality=random.randint(25, 75))
            small = Image.open(io.BytesIO(data.getvalue())).convert("RGB")
        return small.resize(image.size, Image.BILINEAR)


class Cut:
    """A head the frame cuts off, as in a close-up: of a share of the crops only a band is kept (mostly a
    lying one, down to 0.4 of the height; at times an upright one, down to 0.65 of the width) and squeezed
    to the full crop, as the app squeezes a head box of any shape."""

    def __init__(self, share):
        self.share = share

    def __call__(self, image):
        if random.random() >= self.share:
            return image
        w, h = image.size
        if random.random() < 0.75:
            keep = random.uniform(0.4, 1.0) * h
            top = random.uniform(0.0, h - keep)
            area = (0.0, top, float(w), top + keep)
        else:
            keep = random.uniform(0.65, 1.0) * w
            left = random.uniform(0.0, w - keep)
            area = (left, 0.0, left + keep, float(h))
        return image.resize((w, h), Image.BILINEAR, box=area)


def train_transform(shrink, flip, cut=0.0):
    steps = [Cut(cut), transforms.RandomResizedCrop(crops.CROP, scale=(0.6, 1.0))]
    if flip:
        steps.append(transforms.RandomHorizontalFlip())
    steps += [transforms.ColorJitter(brightness=0.3, contrast=0.3, saturation=0.2), Shrink(shrink)]
    return transforms.Compose(steps)


class Faces(torch.utils.data.Dataset):
    """The images of some identities: `(crop as bytes, 3 × 224 × 224; label)`. Normalising happens on the
    graphics card (`normalise`)."""

    def __init__(self, path, rows, names, transform=None):
        label = {name: i for i, name in enumerate(names)}
        self.path, self.transform = path, transform
        self.items = [(member, label[name], box) for member, name, box in rows if name in label]
        self.zip = None

    def __len__(self):
        return len(self.items)

    def __getitem__(self, i):
        if self.zip is None:  # one handle per worker
            self.zip = zipfile.ZipFile(self.path)
        member, label, box = self.items[i]
        image = crop(Image.open(io.BytesIO(self.zip.read(member))).convert("RGB"), box)
        if self.transform is not None:
            image = self.transform(image)
        return torch.from_numpy(np.asarray(image).copy()).permute(2, 0, 1), label


def normalise(batch):
    """Crops as bytes on the device to the network's input."""
    mean = torch.tensor(crops.MEAN, device=batch.device).view(1, 3, 1, 1)
    std = torch.tensor(crops.STD, device=batch.device).view(1, 3, 1, 1)
    return (batch.float() / 255.0 - mean) / std
