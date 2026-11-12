"""Would a trained head do better than the ready-made outputs? A first, free look.

    uv run expr_probe.py <name>=<crops_dir>,<scores_dir>,<check.tsv>[,<emb_dir>] [<name>=… …]

Per label a logistic regression over every output of every model (about
150 numbers per crop, the models frozen), on the labelled crops of the
check sets; or, with `EMB=<model>[,<model>]`, over those models' picture
embeddings (`<emb_dir>/<model>.npy`, written by expr_models.py with `--emb
<emb_dir> --only <check.tsv>`). Three ways, each scored per crop like
expr_check.py (same weights, same interval, same gate):

- `own`: trained and scored inside one episode, in five turns: the tracks
  are split into five parts and each part is scored by a head that never
  saw its tracks.
- `others`: trained on the other episodes, scored on this one.
- `all`: the five turns again, with the other episodes' crops added to the
  training side, each episode counting the same there.

`best` is the highest AP a single ready-made output (or the mean of a
label's candidates) reaches on the same crops. This says whether training is
worth a try, nothing more: the check sets are not there to train what
ships, one episode per kind of footage is little, and which features and
which weight decay are reported was decided after seeing the numbers.

`REG=<strength>` sets the weight decay (0.02, on the mean loss);
`FEATURES=<prefix>[,…]` keeps only the outputs whose names begin so;
`FOLD=character` keeps all tracks of one character of the project in one
part, so that `own` is scored on characters the head never saw (the
stricter reading; a track without a character stands alone, and the app
may have split one person into two characters).
"""
import os
import sys

import numpy as np
import torch

import expr_check as ec
import expr_scores as es

FOLDS = 5


def features(values):
    """One row per crop: probabilities as log-odds, the rest as they are."""
    keep = os.environ.get("FEATURES")
    names = [n for n in sorted(values) if not keep or n.startswith(tuple(keep.split(",")))]
    cols = []
    for n in names:
        v = values[n]
        if not (n.startswith("siglip") or n.endswith((":valence", ":arousal"))):
            p = np.clip(v, 1e-4, 1 - 1e-4)
            v = np.log(p / (1 - p))
        cols.append(v)
    return names, np.stack(cols, 1)


def load(spec):
    name, rest = spec.split("=", 1)
    crops_dir, scores_dir, check_path, *emb_dir = rest.split(",")
    files = [r["file"] for r in es.crops(crops_dir)]
    values = es.outputs(scores_dir, files)
    names, x = features(values)
    place = {f: i for i, f in enumerate(files)}
    check = ec.table(check_path)
    tracks = sorted({c[1] for c in check})
    group, who = {c[1]: c[2] for c in check}, {c[1]: c[5] for c in check}
    judged = [c for c in check if c[4] != "unclear"]
    at = {t: i for i, t in enumerate(tracks)}
    rows = np.array([place[c[0]] for c in judged])
    x, single = x[rows], {n: v[rows] for n, v in ec.add_means(values).items()}
    if os.environ.get("EMB"):
        listed = {c[0] for c in check}  # the rows of `--only <check.tsv>`: the check crops, in crops.tsv's order
        where = {f: i for i, f in enumerate(f for f in files if f in listed)}
        x = np.concatenate([np.load(os.path.join(emb_dir[0], f"{m}.npy")) for m in os.environ["EMB"].split(",")], 1)
        assert len(x) == len(where), "the embeddings are not of the check file's crops"
        x = x[np.array([where[c[0]] for c in judged])].astype(np.float64)
        names = [f"{os.environ['EMB']}:{i}" for i in range(x.shape[1])]
    fold_of = {}
    if os.environ.get("FOLD") == "character":
        # A character's tracks stay in one part: the largest character first, each into the smallest part.
        size, filled, part = {}, [0] * FOLDS, {}
        for c in judged:
            size[c[5]] = size.get(c[5], 0) + 1
        for person in sorted(size, key=lambda n: (-size[n], n)):
            part[person] = int(np.argmin(filled))
            filled[part[person]] += size[person]
        fold_of = {t: part.get(who[t], 0) for t in tracks}
    else:
        rng = np.random.default_rng(4)
        for g in sorted(set(group.values())):  # each group spread evenly over the parts
            members = [t for t in tracks if group[t] == g]
            for k, i in enumerate(rng.permutation(len(members))):
                fold_of[members[i]] = k % FOLDS
    return {
        "name": name, "names": names, "x": x, "single": single,
        "y": {label: np.array([ec.shows(c[4], label) for c in judged], float) for label in es.LABELS},
        "w": np.array([c[3] for c in judged]), "unit": np.array([at[c[1]] for c in judged]),
        "rest": np.array([c[2] == "rest" for c in judged]),
        "fold": np.array([fold_of[c[1]] for c in judged]), "mult": ec.track_draws(tracks, group, who),
    }


def fit(x, y, w, reg):
    """Weighted logistic regression with weight decay, on standardised features; returns a scorer."""
    mean, std = x.mean(0), x.std(0) + 1e-6
    xs = torch.tensor((x - mean) / std, dtype=torch.float64)
    yt, wt = torch.tensor(y, dtype=torch.float64), torch.tensor(w / w.sum(), dtype=torch.float64)
    weight = torch.zeros(xs.shape[1], dtype=torch.float64, requires_grad=True)
    bias = torch.zeros(1, dtype=torch.float64, requires_grad=True)
    opt = torch.optim.LBFGS([weight, bias], max_iter=200, line_search_fn="strong_wolfe")

    def closure():
        opt.zero_grad()
        loss = (wt * torch.nn.functional.binary_cross_entropy_with_logits(xs @ weight + bias, yt, reduction="none")).sum()
        loss = loss + reg * (weight ** 2).sum()  # the same pull whatever the number of crops
        loss.backward()
        return loss

    opt.step(closure)
    wn, b = weight.detach().numpy(), bias.item()
    return lambda z: ((z - mean) / std) @ wn + b


def main():
    sets = [load(spec) for spec in sys.argv[1:]]
    assert all(s["names"] == sets[0]["names"] for s in sets), "the episodes do not have the same outputs"
    reg = float(os.environ.get("REG", "0.02"))
    print(f"{len(sets[0]['names'])} numbers per crop ({os.environ.get('EMB') or 'the outputs'}), weight decay {reg},"
          f" parts by {os.environ.get('FOLD') or 'track'}")
    print("episode\tlabel\tshown\ttracks\trest\tbest\tbest AP\town AP\town P@R50\t95 %\tgate\tothers AP\tall AP\tall P@R50\t95 %\tgate")
    for s in sets:
        rest = [o for o in sets if o is not s]
        for label in es.LABELS:
            y, w = s["y"][label], s["w"]
            n = int(y.sum())
            shown = np.unique(s["unit"][y > 0])
            n_tracks, n_rest = len(shown), len(np.unique(s["unit"][(y > 0) & s["rest"]]))
            if n < ec.FEW:
                print(f"{s['name']}\t{label}\t{n}\t{n_tracks}\t{n_rest}\tfew")
                continue
            best = max(s["single"], key=lambda name: ec.measures(s["single"][name], y, w)[0][0])
            # each episode counts the same on the training side, whatever its number of crops
            ox = np.concatenate([o["x"] for o in rest]) if rest else None
            oy = np.concatenate([o["y"][label] for o in rest]) if rest else None
            ow = np.concatenate([o["w"] / o["w"].sum() for o in rest]) if rest else None
            own, both = np.zeros(len(y)), np.zeros(len(y))
            for k in range(FOLDS):
                train, test = s["fold"] != k, s["fold"] == k
                own[test] = fit(s["x"][train], y[train], w[train], reg)(s["x"][test])
                if rest:
                    both[test] = fit(np.concatenate([s["x"][train], ox]), np.concatenate([y[train], oy]),
                                     np.concatenate([w[train] / w[train].sum(), ow]), reg)(s["x"][test])
            cells = []
            for scores in (own, both):
                ap, half, _, _ = (m[0] for m in ec.measures(scores, y, w))
                low, high = np.nanpercentile(ec.measures(scores, y, w * s["mult"][:, s["unit"]])[1], [2.5, 97.5])
                cells.append(f"{ap:.3f}\t{half:.2f}\t{low:.2f} to {high:.2f}\t{ec.verdict(n_tracks, n_rest, half, low)}")
            others = f"{ec.measures(fit(ox, oy, ow, reg)(s['x']), y, w)[0][0]:.3f}" if rest and oy.sum() >= ec.FEW else "-"
            print(f"{s['name']}\t{label}\t{n}\t{n_tracks}\t{n_rest}\t{best}\t{ec.measures(s['single'][best], y, w)[0][0]:.3f}"
                  f"\t{cells[0]}\t{others}\t{cells[1] if rest else '-'}")


main()
