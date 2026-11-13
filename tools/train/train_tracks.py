"""Recipe A, second stage: the embedder of the first stage goes on learning from tracks of real video, with a
triplet loss (Bamman et al. 2026's `--data-source tracks`; their code is MIT).

    uv run train_tracks.py --from <first stage's last.pt> --tracks <tracks dir> --pairs <dir of triplets.py build>
                           --out <run dir> [--count 100000] [--seed 42] …

A triplet is two heads of one track and a head of a track seen beside it (`triplets.py`: how they are chosen
and drawn); the anchor must be nearer to its positive than to the negative by `--margin`, in cosine distance:
`max(0, (1 - a·p) - (1 - a·n) + margin)`. The run's `--count` triplets are drawn once, from the seed, so a
run is one pass over them: AdamW at `--lr` for every part of the network, `--warmup` steps then a cosine to
zero, mixed precision, gradients clipped at 5, the crops changed as in the first stage. A batch of triplets
goes through the network `--part` triplets at a time (the same step, less memory: a triplet's loss needs only
its own three heads).

`--identity <weight>` keeps the first stage's task beside the triplets: every step also takes a batch of
iCartoonFace's pictures through the first stage's ArcFace classifier (`--zip`, `--heads`, `--drop` as the
first stage had them; its identities and classifier come from the checkpoint), weighted against the triplets.
The weight must be small: at the first stage's weights the classifier's gradient on the embedder is some 20
to 200 times the triplets' (few triplets are still wrong, every picture pulls at the classifier), and the
two are clipped together, so at 1 the step would be the classifier's alone; about 0.02 makes them alike.

What tells a run's state apart from the first stage's: the triplets of the titles held out (`dev` in
`titles.tsv`, never trained on), unchanged crops, a fixed draw: the share ranked right (the positive nearer
than the negative) and the share clear of the margin. Written before the first step and with every
checkpoint to `dev.tsv`. The last weights are the run's result (`last.pt`; `embed.py` and `export.py` read it).

Resumable (`--resume`, only with the settings it began with), saves every `--ckpt-minutes`, stops by itself
`--hours` after each start (0: never), refuses a folder that holds a run.
"""
import argparse
import hashlib
import json
import math
import os
import random
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F
from PIL import Image

import crops
import data
import model
import triplets


def arguments():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--from", dest="start", required=True, help="the first stage's checkpoint (last.pt)")
    p.add_argument("--tracks", required=True, help="the folder with crops/<title>.<kind>.crops")
    p.add_argument("--pairs", required=True, help="the folder triplets.py build wrote")
    p.add_argument("--out", required=True)
    p.add_argument("--count", type=int, default=100_000, help="triplets in the run")
    p.add_argument("--batch", type=int, default=64, help="triplets a step")
    p.add_argument("--part", type=int, default=5, help="triplets through the network at a time")
    p.add_argument("--lr", type=float, default=1e-5)
    p.add_argument("--weight-decay", type=float, default=5e-4)
    p.add_argument("--margin", type=float, default=0.3)
    p.add_argument("--warmup", type=int, default=100)
    p.add_argument("--shrink", type=float, default=0.3)
    p.add_argument("--cut", type=float, default=0.2)
    p.add_argument("--no-flip", action="store_true")
    p.add_argument("--identity", type=float, default=0.0, help="the weight of the first stage's task beside the triplets (0: triplets alone)")
    p.add_argument("--identity-batch", type=int, default=64)
    p.add_argument("--zip", default=None)
    p.add_argument("--heads", default=None)
    p.add_argument("--drop", default=None)
    p.add_argument("--dev-count", type=int, default=2000)
    p.add_argument("--seed", type=int, default=42)
    p.add_argument("--workers", type=int, default=4)
    p.add_argument("--hours", type=float, default=0.0)
    p.add_argument("--ckpt-minutes", type=float, default=20.0)
    p.add_argument("--resume", action="store_true")
    p.add_argument("--cpu", action="store_true")
    return p.parse_args()


class Triples(torch.utils.data.Dataset):
    """Triplets as `3 × 3 × 224 × 224` bytes (anchor, positive, negative), each crop changed on its own."""

    def __init__(self, root, kind, items, transform=None):
        self.root, self.kind, self.items, self.transform = str(root), kind, items, transform
        self.pixels = {}

    def __len__(self):
        return len(self.items)

    def crop(self, title, i):
        if title not in self.pixels:  # one mapping per title and worker
            path = os.path.join(self.root, "crops", f"{title}.{self.kind}.crops")
            self.pixels[title] = np.memmap(path, dtype=np.uint8, mode="r").reshape(-1, crops.CROP, crops.CROP, 3)
        image = Image.fromarray(np.asarray(self.pixels[title][i]))
        if self.transform is not None:
            image = self.transform(image)
        return torch.from_numpy(np.asarray(image).copy()).permute(2, 0, 1)

    def __getitem__(self, k):
        title, a, p, n = self.items[k]
        return torch.stack([self.crop(title, a), self.crop(title, p), self.crop(title, n)])


def draw(pairs, count, seed):
    rng = random.Random(seed)
    return [pairs.draw(rng)[:4] for _ in range(count)]


class Endless:
    """Batches of the identity pictures without end, in an order the seed fixes from any step on."""

    def __init__(self, n, batch, seed, skip):
        self.n, self.batch, self.seed, self.skip = n, batch, seed, skip

    def __iter__(self):
        per, step = self.n // self.batch, self.skip
        while True:
            epoch, at = divmod(step, per)
            order = torch.randperm(self.n, generator=torch.Generator().manual_seed(self.seed * 100_003 + epoch)).tolist()
            for k in range(at, per):
                yield order[k * self.batch : (k + 1) * self.batch]
                step += 1


@torch.no_grad()
def dev_measure(net, loader, margin, device):
    """Of the held-out triplets: the share ranked right, the share clear of the margin, and the mean likeness
    of the anchor to its positive and to its negative."""
    net.eval()
    pos, neg = [], []
    for x in loader:
        x = x.to(device, non_blocking=True)
        with torch.autocast(device.type, dtype=torch.float16, enabled=device.type == "cuda"):
            v = net(data.normalise(x.flatten(0, 1)))
        v = F.normalize(v.float(), dim=1).view(len(x), 3, -1)
        pos.append((v[:, 0] * v[:, 1]).sum(1))
        neg.append((v[:, 0] * v[:, 2]).sum(1))
    net.train()
    pos, neg = torch.cat(pos), torch.cat(neg)
    return ((pos > neg).float().mean().item(), (pos - neg >= margin).float().mean().item(), pos.mean().item(), neg.mean().item())


# What a run can't change half way.
FIXED = ("start", "pairs", "count", "batch", "lr", "weight_decay", "margin", "warmup", "shrink", "cut", "no_flip", "identity",
         "identity_batch", "zip", "heads", "drop", "dev_count", "seed")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 22), b""):
            h.update(block)
    return h.hexdigest()


def save(path, state):
    tmp = Path(str(path) + ".part")
    torch.save(state, tmp)
    os.replace(tmp, path)


def main():
    a = arguments()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    last = out / "last.pt"
    if not a.resume and last.exists():
        sys.exit(f"{out} holds a run already: --resume goes on with it, another --out starts a new one")
    random.seed(a.seed)
    np.random.seed(a.seed)
    torch.manual_seed(a.seed)
    device = torch.device("cpu" if a.cpu or not torch.cuda.is_available() else "cuda")

    settings = json.loads((Path(a.pairs) / "settings.json").read_text(encoding="utf-8"))
    train_pairs, dev_pairs = triplets.Pairs(a.pairs, "train"), triplets.Pairs(a.pairs, "dev")
    train_pairs.load_heads(a.tracks)
    dev_pairs.load_heads(a.tracks)
    items = draw(train_pairs, a.count, a.seed)
    dev_items = draw(dev_pairs, a.dev_count, 20261113)  # the same held-out triplets for every run
    total = a.count // a.batch
    workers = dict(num_workers=a.workers, persistent_workers=a.workers > 0, pin_memory=device.type == "cuda")
    dev_loader = torch.utils.data.DataLoader(Triples(a.tracks, settings["kind"], dev_items), batch_size=16, shuffle=False, **workers)

    first = torch.load(a.start, map_location="cpu", weights_only=True)
    net = model.Embedder(first["arch"], first["dim"]).to(device)
    net.load_state_dict(first["model"])
    params = list(net.parameters())
    head = faces = None
    if a.identity > 0:
        assert a.zip and "head" in first and "names" in first, "--identity needs --zip and a first-stage last.pt (with its classifier)"
        # The pictures as the first stage saw them: the same head boxes and the same pictures left out.
        for which in ("heads", "drop"):
            was, now = first["args"].get(which), vars(a)[which]
            assert (Path(was).name if was else None) == (Path(now).name if now else None), f"--{which}: the first stage had {was}"
        names = first["names"]
        head = model.ArcFace(net.dim, len(names), margin=first["args"]["margin"]).to(device)
        head.load_state_dict(first["head"])
        params += list(head.parameters())
        faces = data.Faces(a.zip, data.index(a.zip, a.heads, a.drop), names, data.train_transform(a.shrink, not a.no_flip, a.cut))
    optimizer = torch.optim.AdamW(params, lr=a.lr, weight_decay=a.weight_decay)
    scaler = torch.amp.GradScaler(device.type, enabled=device.type == "cuda")
    state = {"step": 0, "minutes": 0.0}
    # The tables the triplets are drawn from belong to what a run can't change: a rebuild would draw others.
    fixed = {**{k: vars(a)[k] for k in FIXED}, "tables": triplets.tables_of(a.pairs)}

    def factor(step):
        if step < a.warmup:
            return (step + 1) / a.warmup
        return 0.5 * (1.0 + math.cos(math.pi * (step - a.warmup) / max(1, total - a.warmup)))

    def measure():
        found = dev_measure(net, dev_loader, a.margin, device)
        with open(out / "dev.tsv", "a", newline="\n") as f:
            f.write("%d\t%.4f\t%.4f\t%.4f\t%.4f\n" % (state["step"], *found))
        print("step %d: held-out triplets ranked right %.4f, clear of the margin %.4f (likeness to the positive %.3f, to the negative %.3f)" % (state["step"], *found), flush=True)

    if a.resume:
        saved = torch.load(last, map_location="cpu", weights_only=True)
        other = {k: (saved["args"].get(k), v) for k, v in fixed.items() if saved["args"].get(k) != v}
        if other:
            sys.exit(f"the run was started with other settings: {other}")
        net.load_state_dict(saved["model"])
        if head is not None:
            head.load_state_dict(saved["head"])
        optimizer.load_state_dict(saved["optimizer"])
        scaler.load_state_dict(saved["scaler"])
        state = saved["state"]
        print(f"resumed at step {state['step']}", flush=True)
    else:
        commit = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, cwd=Path(__file__).parent).stdout.strip()
        manifest = {
            "args": vars(a), "commit": commit, "from": {"file": a.start, "sha256": sha256(a.start), "arch": first["arch"], "dim": first["dim"]},
            "pairs": settings, "tables": fixed["tables"], "titles": {"train": len(train_pairs.titles), "dev": len(dev_pairs.titles)},
            "anchor_tracks": {"train": sum(len(v) for v in train_pairs.anchors.values()), "dev": sum(len(v) for v in dev_pairs.anchors.values())},
            "steps": total, "torch": torch.__version__, "device": torch.cuda.get_device_name(0) if device.type == "cuda" else "cpu",
        }
        (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
        (out / "train.tsv").write_text("step\tloss\tactive\tidentity_loss\tlr\ttriplets_per_s\tminutes\n", newline="\n")
        (out / "dev.tsv").write_text("step\tright\tclear\tto_positive\tto_negative\n", newline="\n")
    print(f"{a.count} triplets of {len(train_pairs.titles)} titles ({len(dev_pairs.titles)} held out), {total} steps on {device}"
          + (f", with the identity task at {a.identity}" if head is not None else ""), flush=True)
    if not a.resume:
        measure()

    def checkpoint():
        save(last, {
            "arch": net.arch, "dim": net.dim if net.proj is not None else 0, "model": net.state_dict(),
            **({"head": head.state_dict(), "names": first["names"]} if head is not None else {}),
            "optimizer": optimizer.state_dict(), "scaler": scaler.state_dict(), "state": state, "args": fixed,
        })

    rest = torch.utils.data.Subset(Triples(a.tracks, settings["kind"], items, data.train_transform(a.shrink, not a.no_flip, a.cut)),
                                   range(state["step"] * a.batch, total * a.batch))
    loader = torch.utils.data.DataLoader(rest, batch_size=a.batch, shuffle=False, drop_last=True, **workers)
    faces_iter = None
    if faces is not None:
        order = Endless(len(faces), a.identity_batch, a.seed, state["step"])
        faces_iter = iter(torch.utils.data.DataLoader(faces, batch_sampler=order, **workers))

    began = saved_at = time.time()
    before = state["minutes"]
    seen, t0, losses, actives, sides = 0, time.time(), [], [], []
    net.train()
    for x in loader:
        for group in optimizer.param_groups:
            group["lr"] = a.lr * factor(state["step"])
        x = x.to(device, non_blocking=True)
        optimizer.zero_grad(set_to_none=True)
        loss, active = 0.0, 0.0
        for xs in x.split(a.part):
            with torch.autocast(device.type, dtype=torch.float16, enabled=device.type == "cuda"):
                v = net(data.normalise(xs.flatten(0, 1)))
            v = F.normalize(v.float(), dim=1).view(len(xs), 3, -1)
            each = ((v[:, 0] * v[:, 2]).sum(1) - (v[:, 0] * v[:, 1]).sum(1) + a.margin).clamp(min=0)
            part = each.sum() / len(x)
            scaler.scale(part).backward()
            loss, active = loss + part.detach(), active + (each > 0).sum().item()
        side = 0.0
        if faces_iter is not None:
            fx, fy = next(faces_iter)
            fx, fy = fx.to(device, non_blocking=True), fy.to(device, non_blocking=True)
            for xs, ys in zip(fx.split(3 * a.part), fy.split(3 * a.part)):
                with torch.autocast(device.type, dtype=torch.float16, enabled=device.type == "cuda"):
                    features = net(data.normalise(xs))
                part = F.cross_entropy(head(features, ys), ys) * (len(xs) / len(fx))
                scaler.scale(part * a.identity).backward()
                side = side + part.detach()
        scaler.unscale_(optimizer)
        torch.nn.utils.clip_grad_norm_(params, 5.0)
        scaler.step(optimizer)
        scaler.update()
        state["step"] += 1
        state["minutes"] = before + (time.time() - began) / 60
        losses.append(loss.item())
        actives.append(active / len(x))
        sides.append(float(side))
        seen += len(x)
        if not math.isfinite(losses[-1]) or not math.isfinite(sides[-1]):
            sys.exit(f"the loss is {losses[-1]} / {sides[-1]} at step {state['step']}: stopped; a checkpoint, if there is one, is from before")
        if state["step"] % 25 == 0:
            line = (state["step"], sum(losses) / len(losses), sum(actives) / len(actives), sum(sides) / len(sides), optimizer.param_groups[0]["lr"],
                    seen / (time.time() - t0), state["minutes"])
            with open(out / "train.tsv", "a", newline="\n") as f:
                f.write("%d\t%.4f\t%.3f\t%.4f\t%.3g\t%.1f\t%.1f\n" % line)
            print("step %d loss %.4f active %.3f identity %.3f lr %.2g %.1f triplets/s %.1f min" % line, flush=True)
            seen, t0, losses, actives, sides = 0, time.time(), [], [], []
        if time.time() - saved_at > a.ckpt_minutes * 60 and state["step"] < total:
            checkpoint()
            measure()
            saved_at = time.time()
        if a.hours and time.time() - began > a.hours * 3600 and state["step"] < total:
            checkpoint()
            print(f"stopped after {(time.time() - began) / 60:.0f} minutes of this start ({state['minutes']:.0f} in all), as asked; --resume goes on", flush=True)
            return
    checkpoint()
    measure()
    print(f"done: {state['step']} steps in {state['minutes']:.0f} minutes", flush=True)


if __name__ == "__main__":
    main()
