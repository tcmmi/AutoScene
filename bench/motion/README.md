# Motion bench: repeated frames from the stored signals

Can the per-frame differences the app already stores say, per shot, how many
frames are repeats of the frame before (held frames, animation "on twos" or
"on threes")? Bench only: nothing in the app changes, no new analysis pass.

Why: readiness for slow motion is rank 4 of what editors ask of a pack
(`research_notes/Scenepack goals/proposal.md`), and editors complain about
dead or repeated frames in anime. The shots pass stores, for every frame,
the mean absolute RGB difference to the previous frames on a 48×27 picture
(`FrameSignals.diff`, `crates/as-analysis/src/signals.rs`, table
`shot_signals`). A flag made from those numbers would cost no decode and
work on projects analysed long ago.

The media are not in the repository. Decisions and their outcomes are rows
of `DECISIONS.md`.

## Plan (fixed before any run, row 1)

Written on 2026-11-15 before any signal file or video of this bench was
read. A change after that is a new row of `DECISIONS.md` with its reason.

### Episodes

Keys as in `bench/anime/media.tsv`, `bench/3d/media.tsv` and `bench/shots`.
"Fit" episodes choose the candidate's setting; "check" episodes are scored
with that setting fixed.

| Key | Footage | Role | Stored signals | Pictures |
|---|---|---|---|---|
| `jjk_s2e03` | anime | fit | bench input `bb.sig` | the file of `bench/anime/media.tsv` |
| `op_6` | anime | fit | bench input | same |
| `rz_31` | anime | fit | bench input | same |
| `jjk_s2e04` | anime | check | bench input | same |
| `op_a` | anime | check | bench input | same |
| `rz_33` | anime | check | bench input | same |
| `op_7` | anime | check, also shown apart (the file is 25 frames a second) | bench input | same |
| `mir_s1e11` | 3D animation | fit | bench input | the file of `bench/3d/media.tsv` |
| `mir_s5e21` | 3D animation | check | bench input | same |
| `bb_s01e01` | live action | check | a project copy (`shot_signals` as the app stored them, measured on the remux) | the 960×540 playback proxy, the only picture left; same frame grid |

Shots are the segments of kind `shot` that the fusion made from those
signals (`bb.segments.tsv` of each bench input; for Breaking Bad the
`segments` table of the project copy). Shots of at least 12 frames are
scored; shorter ones are counted only. Transitions, black and flashes are
not clips and are left out.

Live action has no fit episode: no repeated frames are expected there, and
the settings chosen for anime and for 3D are both applied to it.

### Reference

FFmpeg's `mpdecimate` (the bundled build, `app/src-tauri/ffmpeg`), on every
decoded frame in order, at the file's own size and pixel format. A frame is
a **repeat** when `mpdecimate` drops it: against the last frame it kept, no
8×8 block differs by more than `hi` and at most `frac` of the blocks differ
by more than `lo` (sums of absolute differences over the block's 64
samples). `max` and `keep` stay 0, so nothing limits a run of drops.

| Setting | `hi` | `lo` | `frac` | Why |
|---|---|---|---|---|
| default (primary) | 768 | 320 | 0.33 | what plain `mpdecimate` does, the filter an editor runs to drop dead frames. 12 levels a sample over one block is more than a second encode of the same drawing changes and less than an outline that moved |
| strict | 256 | 128 | 0.10 | 4 levels a sample: only near-exact copies. Expected to miss repeats that the encoder made noisy |
| loose | 1536 | 640 | 0.33 | 24 levels a sample: forgives heavy compression. Expected to swallow small real motion |

Every comparison is made against all three, so it shows whether a result
hangs on the reference's bar.

**Alignment**, checked before any comparison: the frames decoded equal the
frames in the signal file; and with the reference shifted by −2 to +2
frames, the mean stored difference of the frames it drops is lowest at no
shift. An episode that fails is left out and named.

**The reference checked by eye.** Frame pairs (the frame before, the frame,
and their difference made eight times stronger) are drawn at random
(seed 20261115) from four strata per footage type: dropped by strict;
dropped by default but kept by strict; dropped by loose but kept by default;
kept by all three. 12 pairs a stratum for anime, 6 for 3D, 6 for live action
(fewer where a stratum is smaller). A reader who knows neither the stratum
nor the stored signals says of each: repeat (only noise differs), changed
(something moved or changed), or cannot tell. The default is trusted for a
footage type when at least 90 % of the pairs dropped by strict read as
repeats and at least 90 % of the pairs kept by all three read as changed.
The two strata in between show which way the default errs; their counts are
reported. Where the default is not trusted, the comparison is shown for
information and no gate is read.

### Candidate

From the stored signals alone: `diff[0]` of each frame, the mean absolute
RGB difference to the frame before on the 48×27 picture, 0 to 255. The
shot's first frame is left out (its difference is to another shot).

- **C1, one bar**: a frame is held when `diff[0]` is under the bar. Bars:
  0.02, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5, 0.75, 1.0, 1.5.
- **C2, a bar and the neighbours**: a frame is held when `diff[0]` is under
  a floor, or under the bar and under `rho` times the largest `diff[0]` of
  the up to four frames around it in the shot (two before, two after).
  Floor 0.02 or 0.05; bar 0.3, 0.5 or 1.0; `rho` 0.25 or 0.5.

Per shot: the **held share** (held frames over the shot's frames less one)
and the **cadence**: the shot is cut into steps, a new frame and the held
frames that follow it; the cadence is the step length (1, 2, 3, or 4 and
more, which is a still or a hold) that holds the most frames.

The setting is chosen per footage type on the fit episodes: the one with the
most shots within 0.10 of the default reference's held share; on a tie C1
before C2, then the lower bar. It is then fixed.

What a 48×27 picture cannot see is expected to show as errors, and a sample
of them is looked at: a mouth moving on a held drawing, a slow pan over a
flat background (held to the candidate, new frames to the reference), film
grain and compression noise (new to the candidate, repeats to the
reference).

### Measures

Per footage type, on the check episodes (the fit episodes are shown beside
them, marked), against each reference:

1. The share of shots whose held share is within 0.10 of the reference's;
   the same weighted by frames; Pearson and Spearman correlation of the two
   shares over shots.
2. "This shot has repeated frames" (held share of at least 0.10; also shown
   at 0.50): precision and recall of the candidate's flag against the
   reference's, at every bar of C1 and at the chosen setting; the share of
   shots the reference flags; and how many of the shots it does not flag the
   candidate leaves unflagged.
3. Cadence: how often the candidate's cadence is the reference's, as a
   table of counts.
4. Per frame, as a diagnostic: precision and recall of "held".
5. As a by-product: the spread of a plain motion score per shot (the mean
   `diff[0]` of its frames) per footage type. No claim is made from it.

### Draft gate (for the owner to change)

For a footage type the stored signals are enough for a per-shot flag when,
on its check episodes and with the setting fixed on its fit episodes:

- at least 90 % of the shots have a held share within 0.10 of the default
  reference's, and
- "has repeated frames" (share of at least 0.10) reaches a precision and a
  recall of at least 0.90.

For live action: the candidate flags at most 2 % of the shots that the
reference does not flag.

A context-free reviewer (given the files, not the conclusions) checks the
method and the numbers before the outcome is written into row 1.

## Results (2026-11-15)

Every table is in `tables.md`, written by the tool; the ones that carry the
answer are copied here. Tables added after the first numbers were seen
(row 2) and after the review (row 3) are marked. What the review changed
is listed at the end.

### In short

- **The draft gate is not met for anime or 3D.** Anime: 72 % of the check
  shots are within 0.10 of the default reference's held share (the gate
  asks for 90 %), and the flag reaches a precision of 0.941 and a recall
  of 0.8998, one shot short of 0.90. 3D: 83 %, 0.71 and 0.58. Live action
  passes its gate: 1 of 710 shots without repeats is flagged.
- **The stored number tells the clear cases apart, not the frames in
  between.** A frame the strict reference drops has a lower stored
  difference than a frame all three references keep in 98 to 99.9 % of
  the pairs. But a fifth of the anime frames lie between those two bands,
  and over every frame the same measure against the default is 0.95
  (anime), 0.95 (3D) and 0.97 (live action); per frame the chosen bar
  reaches a precision of 0.84 and a recall of 0.87 for anime, 0.52 and
  0.54 for 3D.
- **No bar does better, against any of the three references.** On the
  anime check shots the best share within 0.10 is 0.724 against the
  default, 0.714 against strict and 0.684 against loose. The miss is not
  to one side (mean +0.004 against the default; 15 % of the shots are over
  by more than 0.10, 12 % under); nine shots in ten are within 0.27.
- **The reference has a soft edge of its own.** The three settings differ
  much (anime repeats: 0.12 to 0.50 of the frames by strict, 0.22 to 0.59
  by the default), and the readers found the truth between strict and
  default: the default also drops frames in which something moved slowly.
- **What the 48×27 picture misses**: a mouth, a blink, sparks, a small
  figure far away (it calls the frame held); a second encode's block noise
  over the whole picture (it calls a repeat new).

### The reference

**What the plan got wrong about `mpdecimate`** (row 3). It does not keep a
frame when more than `frac` of the blocks are over `lo`. It sums 8×8
blocks every 4 samples and keeps the frame when more than `frac` ×
(width / 16) × (height / 16) of them are over `lo`: about 2 % of the
blocks it looks at for 0.33, not a third. The reviewer rebuilt the rule in
numpy and got the tool's masks to the frame only this way. The settings
and the data are as planned; the default is stricter about widespread
small change than the plan's words say.

All ten episodes are aligned: the frames decoded equal the frames in the
signals, and the frames the default drops have their lowest stored
difference at no shift (0.05 to 0.27, against 0.57 to 7.0 one or two frames
off; `tables.md`, "Alignment"). The reviewer checked it from the pictures:
48×27 differences made from the video equal the stored ones on `op_7` and
`mir_s1e11`, and for Breaking Bad the proxy's differences follow the
remux's signals at no shift (correlation 0.9999, 0.38 one frame off).

Repeats in scored shots, as a share of their frames:

| Key | Footage | Role | Shots scored | Frames | Default | Strict | Loose | Shots with repeats (default, share ≥ 0.10) | Cadence by the default: 1 / 2 / 3 / 4+ |
|---|---|---|---|---|---|---|---|---|---|
| `jjk_s2e03` | anime | fit | 441 | 33,085 | 0.346 | 0.191 | 0.441 | 0.508 | 256 / 48 / 77 / 60 |
| `op_6` | anime | fit | 479 | 32,921 | 0.251 | 0.151 | 0.299 | 0.547 | 306 / 68 / 69 / 36 |
| `rz_31` | anime | fit | 377 | 36,654 | 0.587 | 0.497 | 0.654 | 0.817 | 113 / 52 / 112 / 100 |
| `jjk_s2e04` | anime | check | 449 | 32,892 | 0.314 | 0.155 | 0.389 | 0.494 | 267 / 55 / 83 / 44 |
| `op_a` | anime | check | 391 | 33,035 | 0.440 | 0.326 | 0.496 | 0.701 | 176 / 56 / 97 / 62 |
| `rz_33` | anime | check | 383 | 33,395 | 0.489 | 0.396 | 0.579 | 0.653 | 171 / 35 / 87 / 90 |
| `op_7` | anime | check | 422 | 34,304 | 0.220 | 0.117 | 0.282 | 0.502 | 295 / 82 / 28 / 17 |
| `mir_s1e11` | 3D | fit | 399 | 31,731 | 0.046 | 0.023 | 0.081 | 0.140 | 389 / 0 / 1 / 9 |
| `mir_s5e21` | 3D | check | 399 | 32,928 | 0.065 | 0.021 | 0.124 | 0.261 | 379 / 7 / 2 / 11 |
| `bb_s01e01` | live action | check | 770 | 81,274 | 0.057 | 0.014 | 0.123 | 0.078 | 750 / 4 / 0 / 16 |

**By eye.** 96 frame pairs, two readers where the plan asked for one
(language-model agents, Claude Opus, each with the pages and the rules
only; neither saw the stratum, the episode or the other's answers).
Repeat / changed / cannot tell:

| Footage | Stratum | Pairs | Reader 1 | Reader 2 |
|---|---|---|---|---|
| anime | dropped by strict | 12 | 12 / 0 / 0 | 12 / 0 / 0 |
| anime | dropped by default, kept by strict | 12 | 3 / 8 / 1 | 2 / 6 / 4 |
| anime | dropped by loose, kept by default | 12 | 0 / 11 / 1 | 0 / 12 / 0 |
| anime | kept by all three | 12 | 0 / 12 / 0 | 0 / 12 / 0 |
| 3D | dropped by strict | 6 | 6 / 0 / 0 | 6 / 0 / 0 |
| 3D | dropped by default, kept by strict | 6 | 1 / 3 / 2 | 1 / 5 / 0 |
| 3D | dropped by loose, kept by default | 6 | 0 / 5 / 1 | 1 / 5 / 0 |
| 3D | kept by all three | 6 | 0 / 6 / 0 | 0 / 6 / 0 |
| live action | dropped by strict | 6 | 6 / 0 / 0 | 6 / 0 / 0 |
| live action | dropped by default, kept by strict | 6 | 2 / 4 / 0 | 2 / 4 / 0 |
| live action | dropped by loose, kept by default | 6 | 0 / 5 / 1 | 0 / 4 / 2 |
| live action | kept by all three | 6 | 0 / 6 / 0 | 0 / 6 / 0 |

By the plan's rule the default is trusted for all three kinds of footage:
every pair strict drops reads as a repeat, every pair all three keep reads
as changed. The two readers gave the same answer on 88 of the 96 pairs; the
other eight are one "cannot tell" against a verdict.

The strata in between say which way the default errs. Of the 24 pairs the
default drops and strict keeps, the readers call 6 and 5 repeats and 15
each changed: a camera that drifts, clouds, smoke, hair. Of the 24 that
loose drops and the default keeps, 21 each are changed (mouths, outlines).
So **the default counts slow motion as repeats, and the truth lies between
strict and default, nearer to strict**. That band is not small: 27,774 of
the 236,286 anime frames in scored shots (12 %), 2,190 of 64,659 for 3D,
3,518 of 81,274 for live action. (The settings are not nested: each
compares a frame with the last one it kept itself, so a few frames are
dropped by strict and kept by the default.)

In live action the default's repeats are mostly not copies: the proxy's
encoder smoothed the grain of still shots, and the default then drops
frames of a slowly moving camera. Strict finds what was expected: the end
credits and title cards (6 shots, 1.4 % of the frames; 1,043 of its 1,164
drops lie in those six shots, and all six pairs of its stratum are
end-credit cards, so the eye check says nothing about strict elsewhere in
live action).

The pairs were drawn from frames of scored shots only (shots of 12 frames
or more, their first frame left out); the plan did not say so.

**The references against each other** (row 2; all episodes):

| Footage | Pair | Shots | Held share within 0.10 | Pearson | Flag ≥ 0.10: precision / recall of the first against the second |
|---|---|---|---|---|---|
| anime | strict against default | 2,942 | 0.635 | 0.890 | 1.000 / 0.741 |
| anime | loose against default | 2,942 | 0.772 | 0.937 | 0.854 / 1.000 |
| 3D | strict against default | 798 | 0.870 | 0.822 | 1.000 / 0.388 |
| 3D | loose against default | 798 | 0.833 | 0.921 | 0.563 / 1.000 |
| live action | strict against default | 770 | 0.927 | 0.608 | 1.000 / 0.100 |
| live action | loose against default | 770 | 0.819 | 0.891 | 0.392 / 1.000 |

### The candidate

**The setting.** On the fit episodes the most shots are within 0.10 of the
default with C1 at 0.2 for anime (0.722) and C1 at 0.05 for 3D (0.932). No
setting of C2 is better there (`tables.md`). Had the strict
reference chosen, it would have been C1 at 0.05 for anime and 0.02 for 3D
(row 2).

**Check episodes, the chosen setting** (`tables.md` has the fit episodes
and the flag at 0.50):

| Shots | Setting | Reference | n | Held share within 0.10 | ... by frames | Pearson | Spearman | Flag ≥ 0.10: precision / recall | Reference flags | Unflagged left unflagged |
|---|---|---|---|---|---|---|---|---|---|---|
| anime, check | C1 0.2 | default | 1,645 | 0.724 | 0.692 | 0.898 | 0.897 | 0.941 / 0.900 | 0.582 | 0.921 (633 of 687) |
| anime, check | C1 0.2 | strict | 1,645 | 0.622 | 0.587 | 0.867 | 0.882 | 0.758 / 0.987 | 0.427 | 0.764 (720 of 942) |
| anime, check | C1 0.2 | loose | 1,645 | 0.664 | 0.653 | 0.851 | 0.840 | 0.975 / 0.797 | 0.681 | 0.956 (502 of 525) |
| `jjk_s2e04` | C1 0.2 | default | 449 | 0.737 | 0.722 | 0.892 | 0.881 | 0.918 / 0.905 | 0.494 | 0.921 |
| `op_a` | C1 0.2 | default | 391 | 0.734 | 0.680 | 0.923 | 0.929 | 0.981 / 0.927 | 0.701 | 0.957 |
| `rz_33` | C1 0.2 | default | 383 | 0.640 | 0.575 | 0.888 | 0.888 | 0.957 / 0.892 | 0.653 | 0.925 |
| `op_7` (25 frames a second) | C1 0.2 | default | 422 | 0.777 | 0.789 | 0.844 | 0.851 | 0.898 / 0.868 | 0.502 | 0.900 |
| 3D, check (`mir_s5e21`) | C1 0.05 | default | 399 | 0.832 | 0.884 | 0.600 | 0.621 | 0.706 / 0.577 | 0.261 | 0.915 (270 of 295) |
| 3D, check | C1 0.05 | strict | 399 | 0.850 | 0.876 | 0.527 | 0.662 | 0.318 / 0.818 | 0.083 | 0.842 (308 of 366) |
| 3D, check | C1 0.05 | loose | 399 | 0.672 | 0.743 | 0.565 | 0.543 | 0.847 / 0.396 | 0.456 | 0.940 (204 of 217) |
| live action (`bb_s01e01`) | C1 0.2 | default | 770 | 0.934 | 0.901 | 0.631 | 0.375 | 0.900 / 0.150 | 0.078 | 0.999 (709 of 710) |
| live action | C1 0.2 | strict | 770 | 0.992 | 0.993 | 0.921 | 0.471 | 0.600 / 1.000 | 0.008 | 0.995 (760 of 764) |
| live action | C1 0.05 | default | 770 | 0.929 | 0.891 | 0.615 | 0.223 | 1.000 / 0.100 | 0.078 | 1.000 (710 of 710) |
| live action | C1 0.05 | strict | 770 | 0.999 | 1.000 | 0.963 | 0.524 | 1.000 / 1.000 | 0.008 | 1.000 (764 of 764) |

**The draft gate:**

| Footage | Setting | Shots within 0.10 (≥ 0.90) | Flag precision (≥ 0.90) | Flag recall (≥ 0.90) | Unflagged shots flagged (live action: ≤ 0.02) | Gate |
|---|---|---|---|---|---|---|
| anime | C1 0.2 | 0.724 | 0.9410 (862 of 916) | 0.8998 (862 of 958) | 0.079 (54 of 687) | fail |
| 3D | C1 0.05 | 0.832 | 0.7059 (60 of 85) | 0.5769 (60 of 104) | 0.085 (25 of 295) | fail |
| live action | C1 0.2 | 0.934 | 0.9000 (9 of 10) | 0.1500 (9 of 60) | 0.001 (1 of 710) | pass |
| live action | C1 0.05 | 0.929 | 1.0000 (6 of 6) | 0.1000 (6 of 60) | 0.000 (0 of 710) | pass |

- **Anime fails on both parts.** The share is far from 90 %, and no bar
  brings it near: the best on the check episodes is the chosen one. The
  flag's precision passes; its recall is 862 of 958, one shot short of
  0.90 (`rz_33` and `op_7` are under 0.90 on recall alone, `op_7` on
  precision too). Against strict the same flag reads 0.758 / 0.987, against
  loose 0.975 / 0.797: whether it flags too much or too little depends on
  where the reference's bar lies. The share fails against all three.
- **`op_7` is a quarter of the anime check shots and is not like the
  others** (row 3). The file is at 25 frames a second and repeats a frame
  about once a second: of the gaps between one strict drop and the next,
  447 are exactly 24 or 25 frames, against 3 to 9 in the other anime
  episodes. Without it the three other check episodes read 0.706 for the
  share and 0.954 / 0.909 for the flag: the flag then passes, the share
  still fails.
- **3D fails.** Repeats are rare (2 to 7 % of the frames) and are not
  animation on twos: by the default's cadence 379 of the 399 check shots
  move on every frame. The reviewer counted the default's dropped 3D
  frames: 27 % sit in runs of eight or more, 32 % are single frames. Which
  shots hold "at least 10 %" is mostly the band between strict and
  default: strict flags 8 % of the shots, the default 26 %.
- **Live action passes**, because there is next to nothing to find and the
  candidate finds next to nothing. Film grain keeps the stored difference
  up (nine in ten of the frames all three references keep are above 0.56);
  the frames under 0.05 are the end credits and title cards, which strict
  names too (flag 1.000 / 1.000 against strict at 0.05, on 6 shots). The
  signals are of the remux and the pictures of the proxy; the reviewer
  made 48×27 differences from the proxy itself and the gate passes with
  them too (0 and 3 of 710 shots flagged). This is one grainy title: how
  low the stored difference of a clean digital picture falls was not
  measured.

**Per frame** (check episodes, diagnostic):

| Footage | Setting | Reference | Frames | Held by the reference | Held by the candidate | Precision | Recall |
|---|---|---|---|---|---|---|---|
| anime | C1 0.2 | default | 133,626 | 48,774 | 50,229 | 0.842 | 0.867 |
| anime | C1 0.2 | strict | 133,626 | 33,082 | 50,229 | 0.631 | 0.958 |
| 3D | C1 0.05 | default | 32,928 | 2,151 | 2,270 | 0.516 | 0.544 |
| 3D | C1 0.05 | strict | 32,928 | 704 | 2,270 | 0.272 | 0.876 |
| live action | C1 0.05 | default | 81,274 | 4,673 | 1,085 | 0.995 | 0.231 |
| live action | C1 0.05 | strict | 81,274 | 1,164 | 1,085 | 0.960 | 0.895 |

**The stored difference in each band of the reference** (row 2; all
episodes, frames of scored shots). The last column says how often a frame
of the band has a lower stored difference than a frame all three keep:

| Footage | Band | Frames | 10 % | Median | 90 % | 99 % | Lower than a kept frame |
|---|---|---|---|---|---|---|---|
| anime | dropped by strict | 62,599 | 0.011 | 0.036 | 0.117 | 0.291 | 0.984 |
| anime | dropped by default, kept by strict | 27,774 | 0.024 | 0.120 | 0.438 | 1.298 | 0.931 |
| anime | dropped by loose, kept by default | 17,649 | 0.056 | 0.294 | 0.831 | 2.543 | 0.863 |
| anime | kept by all three | 128,588 | 0.255 | 2.683 | 24.076 | 67.631 | |
| 3D | dropped by strict | 1,439 | 0.000 | 0.009 | 0.068 | 0.190 | 0.989 |
| 3D | dropped by default, kept by strict | 2,190 | 0.019 | 0.065 | 0.183 | 0.370 | 0.947 |
| 3D | dropped by loose, kept by default | 3,357 | 0.035 | 0.104 | 0.312 | 1.080 | 0.897 |
| 3D | kept by all three | 57,699 | 0.139 | 1.048 | 11.605 | 39.831 | |
| live action | dropped by strict | 1,164 | 0.000 | 0.000 | 0.197 | 0.342 | 0.999 |
| live action | dropped by default, kept by strict | 3,518 | 0.231 | 0.328 | 0.460 | 0.720 | 0.978 |
| live action | dropped by loose, kept by default | 5,714 | 0.292 | 0.448 | 0.703 | 1.068 | 0.928 |
| live action | kept by all three | 70,887 | 0.557 | 1.587 | 7.116 | 21.354 | |

The clear cases are far apart; what no bar settles is that about a tenth
of the frames all three references keep have a stored difference as low as
a repeat's (anime: a tenth are under 0.255, where 99 % of strict's repeats
lie under 0.291). The bands overlap a little, so the anime rows add up to
236,610 frames, not 236,286.

**Every frame, dropped against kept** (row 3): how often a frame a
reference drops has a lower stored difference than a frame the same
reference keeps. This is the measure that counts for telling frames apart;
the table above leaves out the frames in between.

| Footage | Frames | Default | Strict | Loose |
|---|---|---|---|---|
| anime | 236,286 | 0.954 | 0.943 | 0.950 |
| 3D | 64,659 | 0.952 | 0.980 | 0.930 |
| live action | 81,274 | 0.971 | 0.997 | 0.949 |

**How far the held share is off** (row 3; check episodes, the chosen
setting; the candidate's share less the reference's, per shot):

| Footage | Setting | Reference | n | Mean | Over by more than 0.10 | Under by more than 0.10 | Half the shots within | 90 % of the shots within |
|---|---|---|---|---|---|---|---|---|
| anime | C1 0.2 | default | 1,645 | +0.004 | 0.154 | 0.122 | 0.017 | 0.266 |
| anime | C1 0.2 | strict | 1,645 | +0.118 | 0.368 | 0.010 | 0.043 | 0.375 |
| anime | C1 0.2 | loose | 1,645 | −0.065 | 0.082 | 0.253 | 0.035 | 0.336 |
| 3D | C1 0.05 | default | 399 | −0.004 | 0.078 | 0.090 | 0.019 | 0.168 |
| 3D | C1 0.05 | strict | 399 | +0.054 | 0.145 | 0.005 | 0.000 | 0.144 |
| live action | C1 0.05 | default | 770 | −0.027 | 0.000 | 0.071 | 0.000 | 0.057 |
| live action | C1 0.05 | strict | 770 | +0.000 | 0.001 | 0.000 | 0.000 | 0.000 |

For anime half the shots are within 0.02 of the default and a tenth are
off by more than 0.27: the share is close for most shots and far off for
some, to both sides.

**Cadence** (check episodes, the chosen setting against the default
reference; rows the reference, columns the candidate). Anime, the same
cadence in 0.790 of 1,645 shots:

| Reference \ candidate | 1 | 2 | 3 | 4+ |
|---|---|---|---|---|
| 1 | 867 | 14 | 15 | 13 |
| 2 | 83 | 97 | 19 | 29 |
| 3 | 43 | 3 | 139 | 110 |
| 4+ | 7 | 0 | 9 | 197 |

Read as two classes: of the 1,000 shots the candidate calls "on ones", 867
are on ones by the reference (0.867); of the 909 the reference calls on
ones, the candidate finds 867 (0.954). It tells twos from threes badly:
of 228 shots on twos it names 97, and 110 of the 295 on threes it takes
for stills. 3D and live action are nearly all on ones by both (0.942 and
0.981 the same); the few other shots are not told apart. No reader checked
a cadence.

**The errors by eye.** 59 frame pairs where the chosen candidate and the
default reference differ, the same two readers' rules, repeat / changed /
cannot tell. The plan asked only for "a sample"; these strata and their
sizes were set after the comparison had been seen (seed 20261116, drawn
from scored shots).

| Footage | Stratum | Frames in it | Pairs | Reader 1 | Reader 2 | What the readers saw |
|---|---|---|---|---|---|---|
| anime | held to the candidate, new to the reference | 16,061 | 12 | 0 / 11 / 1 | 1 / 10 / 1 | a mouth alone (3), sparks, embers, fire, dust, a falling streak (5), part of a small figure, a faint reflection, a slow camera move, one unclear: the reference is right |
| anime | new to the candidate, a repeat to the reference | 10,988 | 12 | 6 / 4 / 2 | 6 / 4 / 2 | block noise over the whole picture (6: the candidate is wrong); a glow, a dissolve, outlines that shift (4: the reference is wrong); clouds that may drift (2) |
| 3D | held to the candidate, new to the reference | 1,602 | 6 | 0 / 6 / 0 | 0 / 6 / 0 | a blink, a mouth, a small figure far away, a propeller: the reference is right |
| 3D | new to the candidate, a repeat to the reference | 1,592 | 6 | 1 / 4 / 1 | 1 / 4 / 1 | a mouth, hair, lights in the background (4: the reference is wrong) |
| live action | held to C1 0.05, new to the reference | 5 | 5 | 3 / 1 / 1 | 3 / 1 / 1 | the title card (the candidate is right) |
| live action | held to C1 0.2, new to the reference | 126 | 6 | 1 / 4 / 1 | 1 / 3 / 2 | credits fading in and out, a slight camera move |
| live action | new to C1 0.05, a repeat to the reference | 3,593 | 6 | 2 / 4 / 0 | 2 / 4 / 0 | a slight camera move (4: the reference is wrong), film grain (2) |
| live action | new to C1 0.2, a repeat to the reference | 3,461 | 6 | 0 / 3 / 3 | 2 / 4 / 0 | small moves of a figure, film grain |

So the 48×27 picture fails in the two ways the plan named, and the
reference's default in the third: small or faint motion is held to the
candidate; a re-encoded copy with block noise all over is new to it; and
slow motion is a repeat to the default reference.

### By-product: a plain motion score

Mean stored `diff[0]` per shot (0 to 255), all episodes. No claim is made
from it: it was compared with nothing.

| Shots | n | 5 % | 25 % | Median | 75 % | 95 % | Mean |
|---|---|---|---|---|---|---|---|
| anime | 2,942 | 0.06 | 0.48 | 2.06 | 6.73 | 22.13 | 5.32 |
| 3D | 798 | 0.18 | 0.82 | 2.39 | 5.17 | 14.89 | 4.09 |
| live action | 770 | 0.46 | 1.04 | 1.96 | 4.42 | 12.73 | 3.59 |

Per episode the median runs from 0.48 (`rz_31`) to 6.21 (`op_6`): titles
differ more than kinds of footage.

### What a decode pass would add

- The reference itself: which frames are repeats, at full size, with a bar
  that can be set. On the development PC plain `mpdecimate` over a
  24-minute 1080p episode took 18 to 34 s and decoding alone 14 to 19 s
  (software decode; the author's run and the reviewer's, under different
  load). Beside a 48×27 scale, as the shots pass has it, the reviewer
  measured 24 s against 19 s.
- What the stored signals cannot give at any bar: the frames with a mouth
  or a blink on a held drawing, and the copies under block noise.
- The shots pass already decodes every frame. A drop decision taken there
  would cost no second decode on new analyses; projects analysed before
  would need the video read again. An export decodes its clips anyway.
- It does not settle where a repeat ends and slow motion begins: that is
  the reference's own soft edge, and it needs labels by eye, not a decode.

### What a pack filter or sort could honestly promise

From the stored signals, for anime, with the bar at 0.2:

- **A sort**, "fewest held frames first": the held share ranks shots much
  as the reference does (Spearman 0.90 against the default, 0.88 against
  strict; 0.85 and 0.71 on `op_7`).
- **A coarse mark, not a number.** Of the 729 check shots it leaves
  unflagged (44 %), 633 are under 0.10 by the default (0.87) and 720 by
  strict (0.99). Of the 916 it flags, 862 hold at least 10 % repeats by
  the default (0.94) and 694 by strict (0.76). So "few repeated frames"
  and "has repeated frames" can be said, wrong by the default for 13 % and
  6 % of the shots so marked. "Mostly held" (half the frames or more) is
  weaker:
  precision 0.87 and recall 0.84 against the default, precision 0.57
  against strict. "Free of repeated frames" and "37 % repeats" cannot be
  said, and twos are not told from threes.
- **Not which frames.** Removing dead frames needs the frames themselves:
  of the frames the candidate calls held, one in six is new to the default
  reference and more than one in three to strict (precision 0.84 and
  0.63).
- **Live action**: stills (credits, title cards) can be marked; there is
  nothing else to mark, on the one grainy title tested. **3D**: nothing is
  promised.
- **Files made for another frame rate** (`op_7`) carry a repeat about once
  a second in every shot. The share does not show it (4 % of the frames),
  and a slow-motion tool would still stumble on it.

### Not verified

- The readers are two agents of one model; no person looked. The samples
  are 12 pairs a stratum for anime and 6 for 3D and live action: a rate of
  one in ten cannot be told from none on 6 pairs.
- Where the truth lies between strict and default. No reference was set by
  eye; a third of the frames in that band may be repeats or two thirds.
- Other encodes. The anime files are 1080p web encodes at 1.4 to 3 Mbit/s;
  a disc encode has less block noise, so its bar may differ. The fit and
  check episodes are of the same three series.
- One 3D title and one live-action title. For Breaking Bad the signals are
  of the remux and the pictures of the proxy: the reference's "repeats"
  there are partly the proxy's.
- The cadence classes (no eye check).
- Whether data was read before the plan was committed can only be told
  from file times; they agree.
- Projects whose signals are in an older format than 3 (not looked for).

### What the review changed

A reviewer without context (row 3) ran the reference again without the
tool (five single-setting runs on four episodes: every frame the same),
rebuilt `mpdecimate`'s rule in numpy (2,500 frames, three settings, no
difference), recomputed every table and all 4,510 rows of `shots.tsv`
with a script of their own, checked the alignment from the pictures, and
read 40 pages (the same verdict as reader 1 on 36 and reader 2 on 39,
never repeat against changed; the 24 reference pages were read with the
key seen, the 16 error pages blind). Verdict: pass after fixes. Changed
after it:

- "Per frame the stored number works" is gone: the 98 to 99.9 % compare
  only the two clear bands. Over every frame the measure is 0.95 to 0.97,
  and the per-frame precision and recall say the same as "not which
  frames".
- "No detector can be within 0.10 of a reference that is not within 0.10
  of itself" was wrong as logic and is gone. What the data show is in its
  place: no bar passes 0.73 against any of the three references, and the
  miss is to both sides.
- The anime flag misses the gate too: recall 862 of 958 is 0.8998, which
  the tables printed as 0.900.
- `op_7` is explained and shown apart.
- "Mostly held or on twos or threes" was not what the flag measures; the
  promise now names the shares measured.
- The plan's description of `frac` was wrong (see "The reference").
- "Within 0.10" is counted in whole frames: 0.723 became 0.724 for the
  anime check shots, single episodes moved by up to 0.003, no chosen
  setting moved.
- The tool opens the project copy as immutable. `mode=ro` had touched the
  write-ahead-log side files beside the copy's database (the database
  itself was not changed).
- The timing is given as both runs measured it; "3D repeats are stills"
  is replaced by the counts; live action's pass is said to rest on
  credits and grain; what was left unsaid about the eye samples is said.

## Files

| File | What |
|---|---|
| `episodes.tsv` | the ten episodes: key, footage, role, where the stored signals and the video's bench input are (folders outside the repository) |
| `ref/<key>.txt` | per episode and reference setting, which frames `mpdecimate` drops: a bit per frame, in hex |
| `shots.tsv` | one row per scored shot: frames `[start, end)`, the motion score, the held share and cadence by each reference, the held share at every bar of C1, the cadence at the chosen settings |
| `chosen.tsv` | the setting chosen per footage type |
| `tables.md` | every table, as the tool wrote it |
| `eye.reference.tsv`, `eye.errors.tsv` | the frame pairs read by eye: page, footage, stratum, episode, frame, frames in the stratum, both readers' verdicts and notes |

## How it was made

```sh
RUN="uv run --no-project --python 3.14 --with numpy --with pillow tools/motion-bench/held.py"
# 1. The reference: one decode per episode, three mpdecimate settings (about a minute each)
$RUN reference bench/motion app/src-tauri/ffmpeg/ffmpeg.exe
# 2. The comparison: shots.tsv, chosen.tsv, tables.md
$RUN compare bench/motion
# 3. The pages for the readers (a folder outside the repository), then their answers
$RUN pairs bench/motion app/src-tauri/ffmpeg/ffmpeg.exe <work dir> reference
$RUN pairs bench/motion app/src-tauri/ffmpeg/ffmpeg.exe <work dir> errors
$RUN read bench/motion reference <reader 1 part 1>+<part 2> <reader 2 part 1>+<part 2>
$RUN read bench/motion errors <reader 1 part 1>+<part 2> <reader 2 part 1>+<part 2>
```

The tool reads the stored signals from bench inputs (`bb.sig`,
`bb.segments.tsv`) or from a copy of a project (`shot_signals`,
`segments`), opened read-only and immutable, and the video a bench input's
`bb.path` names. A page shows the frame before, the frame, their difference eight
times stronger, and the 256-pixel patch that differs most, twice as large.
Each reader got the pages under shuffled numbers and these rules: repeat
(nothing in the scene moved; only noise or grain differs), changed
(something moved or changed, however small), cannot tell.
