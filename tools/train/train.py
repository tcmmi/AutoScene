"""Recipe A, first stage: fine-tune DINOv2 on iCartoonFace's identities with an ArcFace classifier.

    uv run train.py --zip <personai_icartoonface_rectrain.zip> --out <run dir> [--arch vits14] …

After Bamman et al. 2026 (their code is MIT): AdamW, the backbone at 1e-5 and the new parts at 1e-3, 500
warm-up steps then a cosine to zero, mixed precision, gradients clipped at 5; crops grow and shrink a little,
mirror, change colour, and (ours) are shrunk and compressed as a small head in a video is. A tenth of the
identities is held out; the checkpoint that finds a held-out face's own identity most often (Rank@1 among all
held-out faces) is `best.pt`.

A run is resumable (`--resume`: it goes on from `last.pt`, same order of pictures, and only with the settings
it was started with), saves every `--ckpt-minutes`, and stops by itself `--hours` after each start (0: never),
so a rented machine never runs on unwatched.
`--limit-identities` keeps a share of the data for a dry run; `--exclude` names a file of identities that
never train (the bench franchises, `leaks.py`); `--fold k/n` trains on every n-th identity only, so that the
others can be judged by a model that never saw them (`mixed.py`). What ran is written to `manifest.json`, the
identities it trained on to `identities.txt`.
"""
import argparse
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

import data
import model


def arguments():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--zip", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--arch", default="vits14", choices=list(model.ARCH))
    p.add_argument("--dim", type=int, default=0, help="the embedding's size (0: the backbone's own)")
    p.add_argument("--epochs", type=int, default=30)
    p.add_argument("--batch", type=int, default=64)
    p.add_argument("--lr-backbone", type=float, default=1e-5)
    p.add_argument("--lr-head", type=float, default=1e-3)
    p.add_argument("--weight-decay", type=float, default=5e-4)
    p.add_argument("--warmup", type=int, default=500)
    p.add_argument("--shrink", type=float, default=0.3, help="the share of crops shrunk and compressed")
    p.add_argument("--cut", type=float, default=0.2, help="the share of crops of which only a band is kept")
    p.add_argument("--heads", default=None, help="the table of heads.py: crops around the app's head boxes")
    p.add_argument("--no-flip", action="store_true")
    p.add_argument("--dev-fraction", type=float, default=0.1)
    p.add_argument("--dev-probes", type=int, default=2)
    p.add_argument("--limit-identities", type=int, default=0)
    p.add_argument("--exclude", default=None)
    p.add_argument("--drop", default=None, help="a table of single pictures that never train")
    p.add_argument("--fold", default=None, help="k/n: train on every n-th identity only, from the k-th on")
    p.add_argument("--seed", type=int, default=42)
    p.add_argument("--workers", type=int, default=4)
    p.add_argument("--hours", type=float, default=0.0)
    p.add_argument("--ckpt-minutes", type=float, default=30.0)
    p.add_argument("--resume", action="store_true")
    p.add_argument("--cpu", action="store_true")
    return p.parse_args()


class Order(torch.utils.data.Sampler):
    """The pictures of one epoch, in an order its number and the seed fix, from batch `skip` on."""

    def __init__(self, n, batch, seed):
        self.n, self.batch, self.seed, self.items = n, batch, seed, []

    def set(self, epoch, skip):
        g = torch.Generator().manual_seed(self.seed * 100_003 + epoch)
        self.items = torch.randperm(self.n, generator=g).tolist()[skip * self.batch :]

    def __iter__(self):
        return iter(self.items)

    def __len__(self):
        return len(self.items)


@torch.no_grad()
def rank1(net, loader, probes, device):
    """How often the nearest other held-out face of a probe is of its own identity."""
    net.eval()
    vectors, labels = [], []
    for x, y in loader:
        with torch.autocast(device.type, dtype=torch.float16, enabled=device.type == "cuda"):
            v = net(data.normalise(x.to(device, non_blocking=True)))
        vectors.append(F.normalize(v.float(), dim=1))
        labels.append(y)
    vectors, labels = torch.cat(vectors), torch.cat(labels).to(device)
    seen, probe = {}, []
    for i, y in enumerate(labels.tolist()):
        seen[y] = seen.get(y, 0) + 1
        if seen[y] <= probes:
            probe.append(i)
    probe = torch.tensor(probe, device=device)
    like = vectors[probe] @ vectors.t()
    like[torch.arange(len(probe), device=device), probe] = -2.0
    net.train()
    return (labels[like.argmax(1)] == labels[probe]).float().mean().item()


# What a run can't change half way: its checkpoints only fit the same settings.
FIXED = ("arch", "dim", "epochs", "batch", "lr_backbone", "lr_head", "weight_decay", "warmup", "shrink", "cut", "heads", "drop", "no_flip",
         "dev_fraction", "limit_identities", "exclude", "fold", "seed")


def save(path, state):
    tmp = Path(str(path) + ".part")
    torch.save(state, tmp)
    os.replace(tmp, path)


def main():
    a = arguments()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    random.seed(a.seed)
    np.random.seed(a.seed)
    torch.manual_seed(a.seed)
    device = torch.device("cpu" if a.cpu or not torch.cuda.is_available() else "cuda")

    rows = data.index(a.zip, a.heads, a.drop)
    exclude = data.read_exclude(a.exclude)
    fold = tuple(int(v) for v in a.fold.split("/")) if a.fold else None
    train_names, dev_names = data.split(rows, exclude, a.limit_identities, a.dev_fraction, a.seed, fold)
    train = data.Faces(a.zip, rows, train_names, data.train_transform(a.shrink, not a.no_flip, a.cut))
    dev = data.Faces(a.zip, rows, dev_names)
    steps_per_epoch = len(train) // a.batch
    total = steps_per_epoch * a.epochs
    # The workers stay for the whole run: starting them takes longer than a small epoch.
    workers = dict(num_workers=a.workers, persistent_workers=a.workers > 0, pin_memory=device.type == "cuda")
    dev_loader = torch.utils.data.DataLoader(dev, batch_size=128, shuffle=False, **workers)
    order = Order(len(train), a.batch, a.seed)
    loader = torch.utils.data.DataLoader(train, batch_size=a.batch, sampler=order, drop_last=True, **workers)

    net = model.Embedder(a.arch, a.dim, pretrained=not a.resume).to(device)
    head = model.ArcFace(net.dim, len(train_names)).to(device)
    fresh = [p for n, p in net.named_parameters() if not n.startswith("backbone.")] + list(head.parameters())
    optimizer = torch.optim.AdamW(
        [{"params": net.backbone.parameters(), "lr": a.lr_backbone}, {"params": fresh, "lr": a.lr_head}],
        weight_decay=a.weight_decay,
    )

    def factor(step):
        if step < a.warmup:
            return (step + 1) / a.warmup
        return 0.5 * (1.0 + math.cos(math.pi * (step - a.warmup) / max(1, total - a.warmup)))

    scaler = torch.amp.GradScaler(device.type, enabled=device.type == "cuda")
    state = {"step": 0, "epoch": 0, "in_epoch": 0, "best": -1.0, "minutes": 0.0}
    last = out / "last.pt"
    fixed = {k: vars(a)[k] for k in FIXED}
    if not a.resume and (last.exists() or (out / "best.pt").exists()):
        sys.exit(f"{out} holds a run already: --resume goes on with it, another --out starts a new one")
    if a.resume:
        saved = torch.load(last, map_location="cpu", weights_only=True)
        other = {k: (saved["args"][k], v) for k, v in fixed.items() if saved["args"][k] != v}
        if other or saved["names"] != train_names:
            sys.exit(f"the run was started with other settings or identities: {other or 'another set of identities'}")
        net.load_state_dict(saved["model"])
        head.load_state_dict(saved["head"])
        optimizer.load_state_dict(saved["optimizer"])
        scaler.load_state_dict(saved["scaler"])
        state = saved["state"]
        print(f"resumed at step {state['step']} (epoch {state['epoch']}, batch {state['in_epoch']})", flush=True)
    else:
        commit = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, cwd=Path(__file__).parent).stdout.strip()
        manifest = {
            "args": vars(a), "commit": commit, "archive": [Path(a.zip).name, os.path.getsize(a.zip)],
            "identities": {"train": len(train_names), "dev": len(dev_names), "excluded": len(exclude)},
            "pictures": {"train": len(train), "dev": len(dev)}, "steps": total,
            "torch": torch.__version__, "device": torch.cuda.get_device_name(0) if device.type == "cuda" else "cpu",
        }
        (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
        (out / "identities.txt").write_text("\n".join(train_names) + "\n", newline="\n")
        (out / "train.tsv").write_text("step\tepoch\tloss\tlr\tpictures_per_s\tminutes\n", newline="\n")
        (out / "dev.tsv").write_text("step\tepoch\trank1\n", newline="\n")
    print(f"{len(train)} pictures of {len(train_names)} identities, {len(dev)} of {len(dev_names)} held out; {total} steps on {device}", flush=True)

    def checkpoint():
        save(last, {
            "arch": net.arch, "dim": net.dim if net.proj is not None else 0, "model": net.state_dict(),
            "head": head.state_dict(), "optimizer": optimizer.state_dict(), "scaler": scaler.state_dict(),
            "state": state, "names": train_names, "args": fixed,
        })

    def weights(extra):
        return {"arch": net.arch, "dim": net.dim if net.proj is not None else 0, "model": net.state_dict(), **extra}

    began = saved_at = time.time()
    before = state["minutes"]
    net.train()
    while state["epoch"] < a.epochs:
        order.set(state["epoch"], state["in_epoch"])
        seen, t0, losses = 0, time.time(), []
        for x, y in loader:
            for group, base in zip(optimizer.param_groups, (a.lr_backbone, a.lr_head)):
                group["lr"] = base * factor(state["step"])
            x, y = x.to(device, non_blocking=True), y.to(device, non_blocking=True)
            with torch.autocast(device.type, dtype=torch.float16, enabled=device.type == "cuda"):
                features = net(data.normalise(x))
            loss = F.cross_entropy(head(features, y), y)
            optimizer.zero_grad(set_to_none=True)
            scaler.scale(loss).backward()
            scaler.unscale_(optimizer)
            torch.nn.utils.clip_grad_norm_([p for g in optimizer.param_groups for p in g["params"]], 5.0)
            scaler.step(optimizer)
            scaler.update()
            state["step"] += 1
            state["in_epoch"] += 1
            state["minutes"] = before + (time.time() - began) / 60
            losses.append(loss.item())
            seen += len(x)
            if not math.isfinite(losses[-1]):
                sys.exit(f"the loss is {losses[-1]} at step {state['step']}: stopped; a checkpoint, if there is one, is from before")
            if state["step"] % 50 == 0:
                line = (state["step"], state["epoch"], sum(losses) / len(losses), optimizer.param_groups[0]["lr"], seen / (time.time() - t0), state["minutes"])
                with open(out / "train.tsv", "a", newline="\n") as f:
                    f.write("%d\t%d\t%.4f\t%.3g\t%.0f\t%.1f\n" % line)
                print("step %d epoch %d loss %.3f lr %.2g %.0f pictures/s %.1f min" % line, flush=True)
                seen, t0, losses = 0, time.time(), []
            if time.time() - saved_at > a.ckpt_minutes * 60:
                checkpoint()
                saved_at = time.time()
            if a.hours and time.time() - began > a.hours * 3600:
                checkpoint()
                print(f"stopped after {(time.time() - began) / 60:.0f} minutes of this start ({state['minutes']:.0f} in all), as asked; --resume goes on", flush=True)
                return
        state["epoch"] += 1
        state["in_epoch"] = 0
        found = rank1(net, dev_loader, a.dev_probes, device)
        with open(out / "dev.tsv", "a", newline="\n") as f:
            f.write("%d\t%d\t%.4f\n" % (state["step"], state["epoch"], found))
        print(f"epoch {state['epoch']}: Rank@1 on the held-out identities {found:.4f}", flush=True)
        if found >= state["best"]:  # as good later counts: it trained longer
            state["best"] = found
            save(out / "best.pt", weights({"step": state["step"], "epoch": state["epoch"], "rank1": found}))
        checkpoint()
        saved_at = time.time()
    print(f"done: {state['step']} steps in {state['minutes']:.0f} minutes, best Rank@1 {state['best']:.4f}", flush=True)


if __name__ == "__main__":
    main()
