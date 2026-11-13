"""The link bar swept for an embedder: every embedder has its own scale of likeness, so one bar fits only one.

    uv run sweep.py <out.tsv> <name>[=<dir with <project>.emb>] … [--links 0,0.05,…] [--adapt 1,0]

Per embedder (without a directory: the Standard pass's own vectors), link bar and fitted space on or off, the
tuning projects are grouped and scored as `score.py` does. Written per run: `embedder link adapt` and each
project's and the mean's precision, recall and F (main cast per track). The other bars stay the shipped ones.
"""
import sys
from pathlib import Path

import score


def main():
    a = sys.argv[1:]
    flag = lambda name, default: a[a.index(name) + 1] if name in a else default
    plain = [x for k, x in enumerate(a) if not x.startswith("--") and (k == 0 or not a[k - 1].startswith("--"))]
    if len(plain) < 2:
        sys.exit(__doc__)
    out, embedders = Path(plain[0]), plain[1:]
    links = [float(v) for v in flag("--links", "0,0.05,0.1,0.15,0.2,0.25,0.3,0.35,0.4,0.45,0.5,0.55,0.6").split(",")]
    adapts = flag("--adapt", "1,0").split(",")
    names = list(score.PROJECTS)
    lines = ["embedder\tlink\tadapt\t" + "\t".join(f"{p}_{m}" for p in names + ["mean"] for m in "PRF")]
    for spec in embedders:
        name, _, folder = spec.partition("=")
        for adapt in adapts:
            for link in links:
                settings = {"LINK": str(link), "ADAPT": adapt}
                rows = [score.score(f"sweep.{name}", p, Path(folder) / f"{p}.emb" if folder else None, settings) for p in names]
                mean = [sum(r[k] for r in rows) / len(rows) for k in range(3)]
                lines.append(f"{name}\t{link}\t{adapt}\t" + "\t".join("%.3f" % v for r in rows + [mean] for v in r))
                print(f"{name} link {link} adapt {adapt}: P %.3f R %.3f F %.3f" % tuple(mean), flush=True)
        out.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
