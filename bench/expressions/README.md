# Expressions bench

What a face shows — smiling, laughing, crying, an angry look and more,
several at once — and how well ready-made models say it on this project's own
face crops. It serves the expressions slot (`bench/models/DECISIONS.md`,
rows 5 and 11): ready-made models are tested first, and a label ships for a
kind of footage only where it passes its gate.

The media and the crops are not in the repository; the files here hold
track numbers, frames, face boxes, character names and labels only.

## The labels

They name what is visible on the face in one picture, never a feeling:
`smile`, `laugh` (a laughing face gets `laugh`, not `smile`), `cry`, `angry`,
`shout`, `surprised`, `downcast`, `blush`, and the helpers `eyes_closed` and
`mouth_open`. A crop takes every label that applies. Besides them: `none` (a
face that shows none of these), `unclear` (a face the labellers could not
judge: dark, blurred, tiny, turned away, covered) and `noface` (no face in
the middle of the crop at all: the detector's mistake).

## The check sets (`<episode>.check.tsv`)

| File | Footage | Episode | Face tracks | Crops | Labelled tracks | Labelled crops |
|---|---|---|---|---|---|---|
| `bb_s01e01` | real life | Breaking Bad S01E01 | 1,138 | 3,248 | 302 | 876 |
| `bxm_s01e01` | 3D animation with a drawn look | Bass X Machina S01E01 | 1,473 | 3,893 | 307 | 841 |
| `rm` | 2D cartoon | Rick and Morty, a 23-minute compilation of season 5 | 1,283 | 3,447 | 326 | 880 |

All three are copies of projects of the owner's library, with the tracks the
app found. Of every face track up to three crops are cut
(`tools/slot-bench/face_crops.py`: the track in three runs, the detection of
highest quality in each; a square 1.6 times the face box, 256 px).

**Which tracks are labelled** (`expr_select.py`). Tracks are drawn, not
crops, and every crop of a drawn track is labelled. Two groups: `top`, every
track among the highest of an output that could stand for a label (the 3
highest tracks per output, the 6 highest for the rarer labels; 55 such
outputs, named in `expr_scores.py` before any label existed), which is what
finds the faces of rare labels at all; and `rest`, 150 of all other tracks
drawn at random, each with a `weight` (how many tracks it stands for: 6.57,
8.77 and 7.38). With the weights the labelled tracks stand for the whole
episode whatever output is scored, so the models that picked the `top` group
gain nothing from it in their scores (their intervals are a little tighter,
since their highest tracks are all there). Every number below is weighted.

**Who labelled.** Two labellers (AI agents) each read contact sheets of the
crops, thirty per sheet in a shuffled order (`crop_sheets.py`), blind to each
other, to the models' scores and to which group a crop is from (their
transcripts were checked for it: none opened another's file, a list or a
score). The crops the two answered differently went to a third labeller,
blind in the same way. Where two agree, that is the label. Else, with the
third: a face that all three judged gets the labels two of them gave (`none`
when there is no such label); a face that only two judged is `unclear`,
since those two did not agree; `noface` wants two votes; anything else is
`unclear` (`expr_check.py final`). No person has checked these labels yet:
the owner's audit of 100 is open.

How far the first two agree (`expr_check.py join`; kappa is agreement beyond
chance, 1 is perfect):

| | Breaking Bad | Bass X Machina | Rick and Morty |
|---|---|---|---|
| Crops answered alike | 80.1 % | 82.9 % | 77.7 % |
| Crops for the third labeller | 174 | 144 | 196 |
| `smile` | 0.80 | 0.96 | 0.76 |
| `laugh` | 0.79 | 0.92 | 0.82 |
| `cry` | 0.78 | – | 0.82 |
| `angry` | 0.76 | 0.82 | 0.88 |
| `shout` | 0.82 | 0.70 | 0.78 |
| `surprised` | 0.76 | 0.75 | 0.54 |
| `downcast` | 0.82 | 0.81 | 0.81 |
| `blush` | – | – | 1.00 (3 crops) |
| `eyes_closed` | 0.64 | 0.91 | 0.89 |
| `mouth_open` | 0.87 | 0.86 | 0.91 |
| `unclear` | 0.82 | 0.90 | 0.79 |
| `noface` | 0.67 | 0.85 | 0.89 |

What the labels say about the footage itself: of an episode's crops 25.9 %
(Breaking Bad), 46.5 % (Bass X Machina) and 19.2 % (Rick and Morty) are
`unclear`, and 0.4 %, 8.1 % and 8.7 % hold no face. Nearly half of what the
app tracks as faces in Bass X Machina could not be judged for an expression
by agents reading 256 px crops.

Columns of a check file: `file` (the crop, `t<track>_f<frame>.jpg`), `track`,
`frame`, the face box as shares of the frame (`x1 y1 x2 y2`), `character`
(the project's name for whom the track shows, empty where it has none),
`group`, `weight`, the answers of the labellers (`a`, `b`, and `c` where the
third was asked) and `labels`.

## The models

`tools/slot-bench/expr_models.py` runs them over the crops; the files are
named in `bench/models/data.tsv`.

| Model | Made for | Size | Seconds per 100 crops |
|---|---|---|---|
| HSEmotion `enet_b2_8`, `enet_b0_8_va_mtl` | real faces, 8 emotions (the second also valence and arousal) | 29 and 15 MB | 0.4 to 1.8 (graphics card) |
| FMAE AffectNet-8, FMAE-IAT BP4D | real faces, 8 emotions; 12 facial action units | ViT-L, 0.3 billion weights (the files are 3.5 GB each) | 27 to 30 (processor) |
| Kagami-24k | drawn pictures, tags | 629 MB | 12 (graphics card) |
| WD EVA02-large v3 | drawn pictures, tags | 1.2 GB | 18 (graphics card) |
| SigLIP 2 so400m | any picture, against a pair of sentences per label | 1.6 GB | 13 (graphics card) |

py-feat's `face_multitask_v2` is not among them: its network cannot be
rebuilt from the weights file without its code. Of the taggers 50 tags about
faces are kept. An output is `<model>:<column>`; `mean:<label>` is the mean
of the percentile ranks of a label's candidate outputs.

## The measure and the gate

Per label and output (`expr_check.py score`): the average precision (AP),
and the precision at the highest threshold that still finds half of the
label's crops (P@R50) with a 95 % interval. **The gate of row 5: P@R50 at
least 0.80 and the interval's lower end at least 0.70.** `unclear` crops are
left out; `noface` crops count as showing nothing. Per track: a track shows
a label when one of its judged crops does, and its score is the highest of
its judged crops.

The interval is a bootstrap in which the project's characters are drawn with
all their tracks (a track without a character on its own), within each
group: one person's faces are not independent of each other, and in Breaking
Bad one character holds a third of the judged crops. It says how much a
number hangs on which people and tracks this episode happens to hold; it
says nothing about another title.

A label is not judged at all (`few`) where fewer than 20 tracks show it, or
fewer than 5 of them come from the random group: a track's crops are near
copies of each other, and without tracks of the random group the weighted
numbers rest on the models' own picks.

## Results: the best candidate per label, per crop

"Crops" and "tracks" are how many labelled crops and tracks show the label
(in brackets: tracks of the random group), "share" what share of the
episode's judgeable crops that stands for. "Best candidate" is the candidate
(or the mean of the candidates) of highest AP; it was picked on these same
crops, among four to nine (fifteen for "`smile` or `laugh`"), so its number
leans high.

Breaking Bad S01E01 (real life):

| Label | Crops | Tracks | Share | Best candidate | AP | P@R50 (95 %) | Gate |
|---|---|---|---|---|---|---|---|
| `smile` | 97 | 57 (22) | 12.5 % | `fmae_affectnet8:Happy` | 0.705 | 0.80 (0.44 to 0.93) | no |
| `laugh` | 34 | 23 (3) | 2.1 % | `mean:laugh` | 0.578 | 0.65 (0.27 to 0.94) | few |
| `smile` or `laugh` | 131 | 65 (24) | 14.6 % | `mean:smile` | 0.867 | 0.96 (0.69 to 1.00) | no |
| `cry` | 9 | 4 (0) | 0.4 % | `siglip2_so400m:crying` | 0.043 | 0.02 | few |
| `angry` | 21 | 15 (4) | 1.8 % | `enet_b2_8:Anger` | 0.340 | 0.13 | few |
| `shout` | 10 | 9 (0) | 0.4 % | `mean:shout` | 0.656 | 0.83 | few |
| `surprised` | 27 | 17 (2) | 1.6 % | `kagami_24k:surprised` | 0.365 | 0.31 | few |
| `downcast` | 15 | 12 (3) | 1.3 % | `enet_b0_8_va_mtl:Sadness` | 0.209 | 0.10 | few |
| `blush` | 0 | | | | | | |
| `eyes_closed` | 30 | 22 (5) | 2.9 % | `mean:eyes_closed` | 0.345 | 0.20 (0.08 to 0.69) | no |
| `mouth_open` | 323 | 164 (63) | 39.4 % | `wd_eva02_large_v3:open_mouth` | 0.858 | 0.88 (0.79 to 0.95) | **pass** |

Bass X Machina S01E01 (3D animation with a drawn look):

| Label | Crops | Tracks | Share | Best candidate | AP | P@R50 (95 %) | Gate |
|---|---|---|---|---|---|---|---|
| `smile` | 62 | 30 (4) | 5.5 % | `kagami_24k:smile` | 0.537 | 0.43 | few |
| `laugh` | 6 | 3 (0) | 0.3 % | `wd_eva02_large_v3:laughing` | 0.429 | 0.33 | few |
| `smile` or `laugh` | 68 | 31 (4) | 5.8 % | `kagami_24k:smile` | 0.590 | 0.46 | few |
| `cry` | 0 | | | | | | |
| `angry` | 135 | 66 (28) | 28.2 % | `mean:angry` | 0.832 | 0.79 (0.61 to 0.97) | no |
| `shout` | 16 | 9 (0) | 0.8 % | `wd_eva02_large_v3:screaming` | 0.622 | 0.57 | few |
| `surprised` | 31 | 19 (4) | 3.7 % | `kagami_24k:surprised` | 0.433 | 0.38 | few |
| `downcast` | 39 | 19 (8) | 9.2 % | `wd_eva02_large_v3:sad` | 0.774 | 0.94 | few |
| `blush` | 0 | | | | | | |
| `eyes_closed` | 51 | 31 (6) | 6.1 % | `mean:eyes_closed` | 0.968 | 0.98 (0.88 to 1.00) | **pass** |
| `mouth_open` | 273 | 133 (38) | 40.7 % | `mean:mouth_open` | 0.878 | 0.94 (0.87 to 0.99) | **pass** |

Rick and Morty (2D cartoon):

| Label | Crops | Tracks | Share | Best candidate | AP | P@R50 (95 %) | Gate |
|---|---|---|---|---|---|---|---|
| `smile` | 77 | 40 (12) | 8.8 % | `kagami_24k:smile` | 0.936 | 1.00 (0.97 to 1.00) | **pass** |
| `laugh` | 8 | 4 (0) | 0.3 % | `mean:laugh` | 0.779 | 0.80 | few |
| `smile` or `laugh` | 85 | 42 (12) | 9.1 % | `kagami_24k:smile` | 0.939 | 1.00 (0.97 to 1.00) | **pass** |
| `cry` | 14 | 10 (0) | 0.5 % | `mean:cry` | 0.745 | 0.88 | few |
| `angry` | 131 | 67 (31) | 20.3 % | `mean:angry` | 0.691 | 0.70 (0.52 to 0.93) | no |
| `shout` | 41 | 24 (5) | 3.3 % | `kagami_24k:screaming` | 0.513 | 0.50 (0.18 to 0.90) | no |
| `surprised` | 52 | 40 (12) | 5.4 % | `wd_eva02_large_v3:surprised` | 0.415 | 0.42 (0.21 to 0.70) | no |
| `downcast` | 32 | 21 (7) | 3.2 % | `mean:downcast` | 0.327 | 0.33 (0.14 to 0.80) | no |
| `blush` | 3 | 1 (0) | 0.1 % | `kagami_24k:blush` | 0.867 | 1.00 | few |
| `eyes_closed` | 59 | 34 (2) | 2.6 % | `kagami_24k:closed_eyes` | 0.725 | 0.74 | few |
| `mouth_open` | 378 | 183 (68) | 43.6 % | `mean:mouth_open` | 0.875 | 0.90 (0.80 to 0.97) | **pass** |

What the tables say:

- **Through the gate:** `mouth_open` on all three; `smile` (and "`smile` or
  `laugh`") on the cartoon; `eyes_closed` on Bass X Machina.
- **Real life, smiling:** against `smile` alone the best output reaches 0.80
  (0.44 to 0.93), against "`smile` or `laugh`" 0.96 (0.69 to 1.00): most of
  what the smile outputs get wrong is laughing faces. Even the two taken
  together miss the gate by its lower end, per crop; per track they pass
  (`mean:smile` 1.00, 0.71 to 1.00), and `smile` alone comes as near
  (`fmae_affectnet8:Happy` 0.93, 0.69 to 0.98). No output is shown to tell
  a laugh from a smile (`laugh`: 0.65 per crop, 0.84 per track, intervals
  from 0.27 and 0.33; too few tracks of the random group for a verdict).
- **Most cells cannot be judged from one episode.** On real life only
  `smile`, `eyes_closed` and `mouth_open` have the tracks for it; crying
  and blush have them nowhere, laughing nowhere, shouting on the cartoon
  only.
- **Where there are enough tracks and no pass:** `eyes_closed` on real life
  (0.20: the taggers that find closed eyes on drawn faces do not on real
  ones); `angry` on both animated titles (0.79 and 0.70, the first with an
  interval that reaches the gate's region: 0.61 to 0.97); `shout`,
  `surprised` and `downcast` on the cartoon (0.33 to 0.50).
- **No one model serves a label on all three kinds**, though some outputs
  cross over: the best `smile` output is a model for real faces on real
  life and a tagger on the cartoon, and neither kind does well on Bass X
  Machina (0.43, `few`); yet the taggers' `open_mouth` passes on real faces,
  and a real-face model's `Anger` is among the best on Bass X Machina.
- **Per track** the picture is the same, a little kinder (besides the above:
  `angry` on Bass X Machina 0.89, 0.69 to 1.00).
- **An output not named beforehand** does better for `mouth_open` on real
  life: the taggers' `teeth` (0.96, 0.86 to 1.00). It was found on these
  crops, so it counts as a hint only.
- **What the outputs flag besides:** of the crops at or above the threshold
  of `fmae_affectnet8:Happy` on Breaking Bad, 20 % are faces the labellers
  could not judge; of `kagami_24k:smile` on the cartoon, 1 %.

## What adds nothing

Both are measured on `mean:<label>`, which was fixed before the labels, so
nothing is picked here.

- **A crop's track.** Putting a crop's score together with those of the other
  crops of its track (their mean or their highest) lowers the AP of most
  labels on every title (real life `smile` 0.682 to 0.624 and 0.555,
  `mouth_open` 0.831 to 0.719 and 0.647; the cartoon's `mouth_open` 0.875 to
  0.804 and 0.740); where it rises, the label is rare or the gain small.
  The crops of a track lie seconds apart and an expression is a matter of a
  moment. Pooling over half a second to two seconds, as row 5 plans, is not
  tested by this: there are three crops per track.
- **The soundtrack.** YAMNet on the proxy's sound
  (`tools/slot-bench/sound_events.py`) hears laughter, crying or shouting
  within half a second of 0 to 10 % of the crops that show it, and of 0 to
  2.6 % of those that do not; its own AP is little above chance at best, and
  added to the face's score it lowers `laugh` and `shout` on every title
  (real life `laugh` 0.578 to 0.318). As a second vote it has nothing to
  give in this form.

## Would a trained head do better? (`expr_probe.py`)

A first, free look, and no model: per label a logistic regression on the
labelled crops of a check set, scored per crop as above on tracks it never
saw (the tracks in five parts, each part scored by a head trained on the
other four). Over four kinds of input, the models frozen: all 147 outputs of
the models; FMAE's pooled features (the AffectNet model, 1,024 numbers);
SigLIP 2's picture embedding (1,152); and the class and patch tokens of
DINOv2-small (768), the small encoder of the scenes slot. AP, for the labels
that at least 20 crops show (`few` as above); "best output" is the highest
AP of a single ready-made output or `mean:` on the same crops:

| Episode | Label | Crops | Best output | Head on the outputs | Head on FMAE | Head on SigLIP 2 | Head on DINOv2-small |
|---|---|---|---|---|---|---|---|
| Breaking Bad | `smile` | 97 | 0.705 | 0.739 | **0.833** | 0.631 | 0.359 |
| | `laugh` (few) | 34 | 0.578 | 0.570 | 0.521 | 0.462 | 0.470 |
| | `angry` (few) | 21 | 0.340 | 0.180 | 0.163 | 0.259 | 0.133 |
| | `surprised` (few) | 27 | 0.399 | 0.671 | 0.690 | 0.490 | 0.229 |
| | `eyes_closed` | 30 | 0.345 | 0.161 | 0.371 | 0.616 | 0.255 |
| | `mouth_open` | 323 | 0.881 | 0.903 | 0.910 | 0.799 | 0.747 |
| Bass X Machina | `smile` (few) | 62 | 0.537 | 0.459 | 0.486 | 0.338 | 0.095 |
| | `angry` | 135 | 0.832 | 0.808 | 0.763 | 0.674 | 0.555 |
| | `surprised` (few) | 31 | 0.433 | 0.373 | 0.438 | 0.211 | 0.144 |
| | `downcast` (few) | 39 | 0.774 | 0.709 | 0.475 | 0.313 | 0.185 |
| | `eyes_closed` | 51 | 0.968 | 0.899 | 0.461 | 0.744 | 0.119 |
| | `mouth_open` | 273 | 0.889 | 0.902 | 0.844 | 0.768 | 0.585 |
| Rick and Morty | `smile` | 77 | 0.936 | 0.901 | 0.641 | 0.614 | 0.235 |
| | `angry` | 131 | 0.691 | 0.760 | 0.738 | 0.629 | 0.288 |
| | `shout` | 41 | 0.651 | 0.502 | 0.198 | 0.254 | 0.148 |
| | `surprised` | 52 | 0.415 | 0.348 | 0.270 | 0.207 | 0.118 |
| | `downcast` | 32 | 0.367 | 0.426 | 0.478 | 0.358 | 0.029 |
| | `eyes_closed` (few) | 59 | 0.725 | 0.677 | 0.366 | 0.640 | 0.302 |
| | `mouth_open` | 378 | 0.875 | 0.935 | 0.885 | 0.800 | 0.668 |

- **Real life: a head on a face model's features does better at `smile`**
  (a smile that is not a laugh) than any ready-made output: AP 0.833
  against 0.705, P@R50 0.91 (0.79 to 1.00) against 0.80 (0.44 to 0.93),
  from about a hundred smiling crops. That is through the gate when the
  head has seen other tracks of the same people. With all tracks of a
  character kept in one part (`FOLD=character`) the AP is 0.784 and the
  P@R50 0.92, but the interval opens to 0.48 to 1.00: for people the head
  never saw the gain is likely and not shown. With ten times the weight
  decay it stands at 0.85 (0.69 to 0.99), with a tenth at 0.91 (0.78 to
  1.00). `surprised` rises as far (0.399 to 0.690) on too few tracks to
  mean anything yet; `laugh` and `angry` do not rise.
- **The encoder decides.** On real faces the model made for faces is ahead
  of the general one for `smile`, `surprised` and `mouth_open` (not for
  `eyes_closed`), and the small general one is far behind on nearly every
  label of every title: a head on the scenes slot's encoder is no way to
  this.
- **Drawn footage:** FMAE's features are far under the taggers for `smile`
  and `eyes_closed`, and level with or a little over the best output for
  `angry` and `downcast` on the cartoon. A head over all outputs is about
  level with the best single one; `angry` comes nearest to the gate with it
  (Bass X Machina P@R50 0.87, 0.65 to 0.97; the cartoon 0.78, 0.61 to 0.92).
- **One head for all kinds of footage: not settled.** Trained on the other
  two episodes only, a head is usually lower than one trained on the
  episode itself (FMAE, real life's `smile`: 0.607 against 0.833), not
  always (the head on the outputs, Bass X Machina's `eyes_closed`: 0.965
  against 0.899). Adding the other two episodes to an episode's own
  training crops helps the head on the outputs more often than not (12 of
  the 19 cells) and the head on FMAE's features less often (6 of 19).
- **The rare labels stay out of reach**, for the heads as for the ready-made
  outputs: there is too little to learn from and too little to score on.

What this is not: a model to ship (FMAE is a ViT-L and takes 28 s per 100
crops on the processor; a check set is not there to train on), nor a clean
experiment: which features and which weight decay are shown was decided
after seeing the numbers, the one cell where a head passes and no ready-made
output does (real life's `smile` on FMAE) is one of 76, and the parts of one
episode share its look and its light.

## Limits

- One episode per kind of footage, and the cartoon's is a compilation.
- The labels are agents' readings of 256 px crops on sheets of thirty; no
  person has checked them.
- Most labels have too few tracks for any verdict.
- The crops are the app's own tracks: what the app does not track is not
  here, and what it tracks wrongly is (`noface`). A character is the app's
  grouping too: it may have split one person in two.
- The best output per label was chosen on the crops it is scored on, among
  several; with eleven labels, three titles and two levels, a pass or a miss
  by a hair (several intervals begin at 0.69) is no finding.

## Reproduce

```sh
cd tools/slot-bench
uv run face_crops.py <ffmpeg> <proxy.mp4> <project_copy_dir> 0 <crops_dir>
uv run expr_models.py <models_root> <crops_dir> <scores_dir>
uv run expr_select.py <crops_dir> <scores_dir> <check_dir>
uv run crop_sheets.py <check_dir>/list.tsv <check_dir>/sheets
#   two labellers write <check_dir>/a/part*.tsv and <check_dir>/b/part*.tsv
uv run expr_check.py join <check_dir>
uv run crop_sheets.py <check_dir>/disputed/list.tsv <check_dir>/disputed/sheets
#   a third labeller writes <check_dir>/c/part1.tsv
uv run expr_check.py final <check_dir> <project_copy_dir> ../../bench/expressions/<episode>.check.tsv
uv run expr_check.py score <crops_dir> <scores_dir> ../../bench/expressions/<episode>.check.tsv <sound.tsv> <fps_num> <fps_den>
#   the trained heads: the embeddings of the labelled crops, then the heads
uv run expr_models.py --emb <emb_dir> --only <check.tsv> <models_root> <crops_dir> <emb_dir> fmae_affectnet8 siglip2_so400m dinov2_small
EMB=fmae_affectnet8 uv run expr_probe.py <name>=<crops_dir>,<scores_dir>,<check.tsv>,<emb_dir> …
```

Labelling took about 4.5 million tokens for the 2,597 crops (two readings of
each and a third of the 514 the two answered differently), about 1,700 a
crop.
