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
