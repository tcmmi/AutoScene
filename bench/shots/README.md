# Shots bench

Where AutoScene's shot detection (`as_analysis::fuse`) puts a boundary,
against boundaries a person checked by eye in six episodes: live action, 3D
animation and anime. It fails when a change merges two labelled shots,
splits a shot inside a dialogue scene, or does worse on gradual transitions
than the fusion it replaces: more continuous shots split, or more real
transitions lost. Made for issue #59 (2026-11-03).

The media are not in the repository; the files hold frame numbers and short
notes only. All frames are on the decoded frame grid: every frame decoded
from frame 0 (`-fps_mode passthrough`), frame n being the n-th picture. That
grid is the one the app's frame index and stored signals use.

## The episodes

| Episode | What | Frames | fps | Fusion |
|---|---|---|---|---|
| `bb_s01e01` | Breaking Bad S01E01 (BluRay remux, 58 min), as in `bench/identity` | 83,593 | 23.976 | live action |
| `bl_s01e01` | The Blacklist S01E01 (45 min) | 65,030 | 23.976 | live action |
| `ml_awakening` | Miraculous: Awakening (film, 3D/CGI, 102 min) | 147,331 | 24 | live action |
| `ds_mugen_train_e01` | Demon Slayer, Mugen Train arc, episode 1 (26 min) | 37,796 | 23.976 | `--anime` |
| `rz_31` | Re:Zero 31, a tuning episode of `bench/anime` | 37,641 | 23.977 (mean) | `--anime` |
| `rz_33` | Re:Zero 33, a tuning episode of `bench/anime` | 34,766 | 23.977 (mean) | `--anime` |

The fusion is the one the app picks for the project's footage type: anime
gets `FusionParams::anime()`, everything else `live()`, 3D animation
included. The Re:Zero files are the ones `bench/anime/media.tsv` names by
their SHA-256 (`f918dff3…` and `246e00b3…`); they belong to that bench's
tuning set, so its test and reserve episodes stay unseen.

## The files

Tab-separated, with a header row, sorted by episode (in the order above) and
frame.

- **`cuts.tsv`**, `episode frame verdict picked note`: 1,159 hard cuts, each
  checked on the frames on both sides of it, and more where in doubt.
  `frame` is the first frame of the new shot. `verdict` is `cut`, or
  `no_cut` where the detector cuts inside one shot (one row: Miraculous
  113029, lightning relights a face). The Demon Slayer row 37277 is where a
  whip pan really cuts; the detector puts that cut two motion-blurred frames
  late, at 37279.
  `picked` says why the cut was checked, one letter or several:
  - **A**: the shots on both sides share one of the episode's four main
    characters. Only such a cut could ever join two cards of a character.
  - **B**: a face box carries on across the cut (IoU ≥ 0.3 between the last
    detection before it and the first after).
  - **C**: the cut lies in a dialogue window. A window's cut that was
    picked as A or B before the windows were complete may carry only that
    letter: `windows.tsv` says which cuts lie in a window.
  - **R**: a Re:Zero cut in a dialogue window (Re:Zero has no face data).
- **`windows.tsv`**, `episode from to cuts note`: 77 dialogue windows, frames
  `[from, to)` from the start of a run's first shot to the end of its last.
  A run is 5 or more hard cuts in a row between shots under 4 s, with faces
  in the shots on both sides of each cut and 2 or more characters in the
  run; for Re:Zero, the shot lengths alone. **Every boundary inside a window
  is a `cut` row, and there is no other boundary there:** these are complete
  labels, not samples. `cuts` counts them (628 in all: BB 118 in 16 windows,
  BL 232 in 27, ML 75 in 11, DS 6 in 1, RZ 31 73 in 11, RZ 33 124 in 11).
- **`transitions.tsv`**, `episode start end verdict note`: all 216 gradual
  transitions (dissolves, wipes, fades) and flashes the detector put between
  two shots, as its segment `[start, end)`. Each was judged on the frames
  around it and six frames across it. `verdict` is `transition` (a real
  dissolve, wipe or blend, or a cut inside the span), `one_shot` (the
  pictures before and after belong to one continuous shot: a camera move,
  smoke, a change of light, a rack focus, something passing the lens), or
  `unsure`.
- **`limits.tsv`**, `episode still_split lost note`: per episode, the most
  `one_shot` spans the fusion may leave split and the most `transition`
  spans it may lose. They are the numbers of the fusion as it is (`note`
  names it), so a change that does worse on either fails until this file
  says it may: the trade a change makes stands in its diff.

## Running it

```sh
# 1. Signals from the episode file (the format the app stores)
cargo run --release -p as-analysis --example shot_signals -- <ort_dir> <transnet.onnx> <video> <episode>.sig
# 2. Fuse and score (add --anime for the anime episodes)
cargo run --release -p as-analysis --example fuse_signals -- <episode>.sig --bench bench/shots <episode>
```

A boundary the detector finds within 3 frames of a labelled one is that
boundary. The run reports:

- **merged**: `cut` rows with no hard cut within 3 frames: two shots became
  one, or the cut became a blend. Must be 0.
- **split**: a hard cut inside a dialogue window that is no labelled cut, or
  a transition or flash there: one shot became two. Must be 0. A boundary
  within 3 frames of a window's edge only moves that edge.
- **known false cuts**: `no_cut` rows where a hard cut remains.
- **misplaced**: labelled cuts found 2 or 3 frames away.
- **one-shot spans still split**: `one_shot` spans with a boundary of any
  kind within 3 frames of them. **Real transitions lost**: `transition`
  spans with none. Each must stay within its limit in `limits.tsv`.
  `unsure` spans aren't scored.

It exits with 1 when anything is merged or split, or a limit is exceeded;
the last line says which. A run that passes names both numbers with their
limits, and asks for a limit to come down when it is under it, so that the
gain stays. The limits hold for the signal files the results below come
from: signals measured again, on other hardware for instance, may move a
span or two, and then both fusions are run on them before a limit moves.

Its unit tests run with `cargo test` (`cargo test -p as-analysis --example
fuse_signals`); one of them checks these files: rows sorted and unique,
known verdicts, windows apart, each window's `cuts` equal to its `cut`
rows, and one row of limits per episode.

## Results

The fusion of 0cc0975 (2026-11-03). The signals of the four library
episodes fuse to exactly the segments the app stored; Re:Zero's were
measured with `shot_signals`.

| Episode | Merged | Split | Known false cuts | Misplaced | One-shot spans still split | Real transitions lost | Unsure |
|---|---|---|---|---|---|---|---|
| `bb_s01e01` | 0 | 0 | 0 | 0 | 23 / 23 | 0 / 3 | 1 |
| `bl_s01e01` | 0 | 0 | 0 | 0 | 32 / 32 | 0 / 0 | 0 |
| `ml_awakening` | 0 | 0 | 1 (113029) | 0 | 80 / 80 | 0 / 20 | 9 |
| `ds_mugen_train_e01` | 0 | 0 | 0 | 1 (37277 → 37279) | 13 / 13 | 0 / 12 | 0 |
| `rz_31` | 0 | 0 | 0 | 0 | 5 / 5 | 0 / 1 | 2 |
| `rz_33` | 0 | 0 | 0 | 0 | 9 / 9 | 0 / 3 | 3 |
| **All** | **0** | **0** | **1** | **1** | **162 / 162** | **0 / 39** | **15** |

Hard cuts are right: 1,158 of the 1,159 checked are real, all 628 in
dialogue windows among them. The shots a dialogue scene seems to be split
into are shot / reverse-shot editing. Gradual transitions are where shots
really are split: 162 of the 216 lie inside one continuous shot, which they
cut in two, dropping its middle frames. Most of them come from the
palette-walk wipe finder (116 of its 144) and from short, weak TransNet
runs (45 of 71).
