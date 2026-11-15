# Body bench

What following people by their bodies adds to the face pass: characters
found while they face away, are too small or blurred for a face, or wear a
gas mask. Measured on the same episode and labels as `bench/identity`
(Breaking Bad S01E01, BluRay remux, 58 min, 794 shots, glintr100 face pass).

Three stages, from sure to unsure (`as_analysis::{bodies, body_tracker,
link}`, `as_identity::bodies`):

- **(a)** People are tracked through each shot; a body track tied to a face
  track is that face's character in its samples without a face too. More
  screen time and coverage — no new shots.
- **(b)** Faces the face pass left unassigned because they say too little
  (quality < 0.45), or unsure, take the character of the other faces on
  their body, where that character's face isn't on screen already.
  Measurable with the identity labels.
- **(c)** Bodies with no named face look for their person among the bodies
  with a face within ±3 min (one outfit per scene) by appearance (person
  ReID): the best character at ≥ 0.65 and ≥ 0.15 ahead of any other there —
  and no one if that character is on screen then. **Only this stage adds
  shots.**

## Models

InsightFace's `cheetah_l.zip` (model-zoo release, non-commercial licence
like the face models):

| File | SHA256 | What |
|---|---|---|
| `pp_det_med.onnx` | `6e9842a1…e22801` | PaddleDetection PP-YOLOE pedestrian detector, input `image` + `scale_factor` |
| `reid_0265.onnx` | `c15084e7…efef0f` | OpenVINO person-reidentification-retail-0277: 256×128 crop → 256-d |
| `reid_0265_b.onnx` | `9b51fe72…02ce939c` | its copy that batches: `tools/model-prep/inspect_onnx.py free-batch` (27 Reshapes pinned the batch at 1; outputs identical) |

Both exports run one frame at a time: `pp_det_med` returns every frame's
boxes as one list when given a batch. `cheetah_s`'s PicoDet
(`pp_det_small_320.onnx`) returns nothing on DirectML (103 people on the
CPU, 0 on the GPU over the same 100 frames); SCRFD's `scrfd_person_2.5g`
misses close-ups (2 % of faces taller than 30 % of the frame sit on a body).

## Commands

```sh
# Graph check (inputs, batch-1 shapes, DirectML-risky ops, batch parity)
uv run --project tools/model-prep python -I tools/model-prep/inspect_onnx.py info <model.onnx>
# Speed and GPU/CPU parity of the detectors and the ReID on 5 × 200 samples
cargo run --release -p as-analysis --example persons_bench -- <ort_dir> <persons_dir> <ffmpeg_dir> <bb_dir> 6000,20000,40000,60000,75000 200
# Every person of the episode's face-pass samples, once (≈ 4 min)
DETECTOR=pp_det_med:640x384:pad cargo run --release -p as-analysis --example bodies_run -- <ort_dir> <persons_dir> <ffmpeg_dir> <bb_dir> <out.bodies>
# Analysis on the CPU in a second: recall | tracks | score | reid | presence
BODY_STRIDE=6 cargo run --release -p as-analysis --example bodies_bench -- presence <faces.tracks> <out.bodies> <bb_dir> bench/identity/bb_s01e01.labels.tsv
# Sheets to look at, and their verdicts
cargo run --release -p as-analysis --example bodies_bench -- sheets <faces.tracks> <out.bodies> <bb_dir> <labels.tsv> <out_dir>
cargo run --release -p as-analysis --example bodies_render -- <ffmpeg_dir> <bb_dir> <out_dir>/items.tsv <out_dir>
cargo run --release -p as-analysis --example bodies_bench -- audit <out_dir>/items.tsv <verdicts.tsv>
```

`<faces.tracks>` is `faces_run`'s output (see `bench/identity`).

## Results (2026-10-16, RTX 3060 Ti, DirectML)

**Detectors** (`persons_bench`, 1000 samples at 1280×720; GPU and CPU boxes
identical for all but PicoDet):

| Detector | GPU ms/frame | CPU ms/frame | people/frame |
|---|---|---|---|
| pp_det_med 640×640, stretched | 7.35 | 41.9 | 1.11 |
| **pp_det_med 640×384, shape kept** | **4.75** | 25.3 | 1.00 |
| pp_det_small 320×320 | 2.51 | 7.0 | 0 (broken on DirectML) |
| scrfd_person 640×384 (batch 4) | 1.34 | 8.6 | 0.33 |
| reid_0265 (per crop, batch 32 / 1) | 0.66 / 1.53 | 5.35 | – |

**Labelled main-cast faces sitting on a detected body** (`recall`,
26,153 faces): pp_det_med 95.7 % at score ≥ 0.5, 99.3 % at ≥ 0.3; small
faces (< 8 % of the frame) 94.4 %. A third of all people found (score ≥ 0.5)
has no face of the face pass at that moment.

**Body tracks** (`tracks`, shape-kept detector): 2151 tracks, 855 carry a
face track; none of the 27 bodies with two or more named faces mixes two
people (0 of 60 faces on the wrong body; 1 of 83 before the review below
made the ties stricter). Every 2nd sample (stride 6): 1943 tracks, 0 of 60.

**(b) faces named by their body** (`score`, main cast, per track; only
faces too unclear to stand on their own, quality < 0.45, or unsure about
the body's character):

| | P | R | F | screen time F | on their character |
|---|---|---|---|---|---|
| faces only | 0.984 | 0.873 | 0.925 | 0.958 | 0.973 |
| + unassigned faces (1, 0 wrong) | 0.984 | 0.876 | 0.927 | 0.958 | 0.973 |
| + unassigned and unsure (8, 0 wrong) | 0.980 | 0.886 | 0.931 | 0.961 | 0.975 |

Without the quality gate it named 16 (R 0.898), among them a clear face in
a poster.

**(c) calibration** (`reid`): asking face-anchored bodies by their sightings
without a face (84 queries) at ≥ 0.65 / margin 0.15 / ±3 min: 88 % of the
answers right, and right answers on 25 % of all 84 queries; 5 of 80 extras
get a cast name. Pessimistic — the query's own character is on screen, so
the occupancy rule can't be used there; on the real bodies without a face
it rules out everyone already in the shot.

(Reworded 2026-11-15. This line used to read "88 % right, 25 % answered".
The tool's second number is right answers over all queries, not the share
of queries that got an answer. So about 21 of the 84 were right and about
24 were answered, 29 %; the 88 % rests on those 24 answers, which leaves it
anywhere from 0.69 to 0.96 (Wilson, 95 %). The counts are worked back from
the two printed numbers: the run files of the remux were looked for on every
drive on 2026-11-15 and are gone, like the remux itself, so the 84 queries
cannot be asked again, with or without help from the same shot. The proxy's
queries in "The test set" below are a new set, not these.)

**More of the cast** (`presence`, stride 6, (b) with unsure, (c) at 0.65 /
0.15):

| | face s | face shots | +(a) s | +(c) s | +(c) shots |
|---|---|---|---|---|---|
| walter | 1522.9 | 302 | 142.5 | 101.0 | 26 (+8.6 %) |
| jesse | 466.4 | 117 | 42.9 | 44.2 | 10 (+8.5 %) |
| skyler | 436.2 | 66 | 28.5 | 3.8 | 1 |
| hank | 173.7 | 39 | 10.4 | 3.9 | 2 (+5.1 %) |
| krazy8 | 125.9 | 46 | 6.8 | 19.4 | 6 (+13 %) |
| main cast | | 713 | | | 45 (+6.3 %) |

**Looked at** (`bb_s01e01.audit.tsv`: frame, box, claim, verdict; judged by
Claude from the sheets). First round, all strata (stride 3, before the
review's fixes):

| | right | wrong | can't tell | precision (95 %) |
|---|---|---|---|---|
| (a) body without its face | 28 | 0 | 2 | 100 % (88–100 %) |
| (b) face named by its body | 13 | 1 (a face in a poster) | 2 | 93 % (69–99 %) |
| (c) body found by its clothes | 57 | 0 | 9 (tiny extras) | 100 % (94–100 %) |
| shots: who is in them | 38 | 0 | 1 | 100 % (91–100 %) |

Second round, the final code (stride 6), every (b) and (c) answer: 58 judged
in the first round (same person, nearby frame), 14 new.

| | right | wrong | can't tell | precision (95 %) |
|---|---|---|---|---|
| (b) face named by its body | 7 | 0 | 0 | 100 % (65–100 %) |
| (c) body found by its clothes | 57 | 1 | 7 (tiny extras) | 98 % (91–100 %) |

Main cast (c): 50 right, 1 wrong — a false person (chemical containers) in a
dark close-up in the RV, matched to an equally dark anchor of Walter. Two
right answers are a hand and an arm only: identity right, but no clip for a
pack (a prominence gate is needed). Still missed: backs of heads in the
foreground of over-the-shoulder shots, tiny or dark figures (7 of 39 shots
in the first round).

**Cost**: the detector is the cost — 5.5 ms per sample (with the resize).
Running in the face pass (decoding shared), every 2nd sample: ≈ 75 s for the
58-min episode, plus ≈ 15 s for ReID on ≤ 8 crops per body track — about
+40 % on the 225 s face pass. As its own pass it also decodes (119 s).

## In the app (2026-10-17)

An optional mode, off by default: Settings → General → "Find characters
without a visible face" (`AppSettings::find_bodies`). Turning it on places
every project's faces again and looks through the episodes analysed before
for their people. Turning it off drops everything bodies said and shows no
body at all — the user's word on one too; a project that couldn't be placed
again then catches up when it opens. The bodies stay stored, so turning it
on again needs no new pass. If finding people fails in the face pass, the
faces go on alone and the people pass tries the people on its own.

- **Models**: `pp_det_med.onnx` and `reid_0265.onnx` are fetched as members
  of `cheetah_l.zip` by HTTP range requests (central directory, then the one
  member), resumable and SHA-pinned like every model (`as_models::zip`). The
  ReID's 27 batch-pinned Reshapes are patched in memory when it loads
  (`as_infer::onnx::free_reshape_batch`); the file stays byte-identical, so
  `reid_0265_b.onnx` isn't needed.
- **In the face pass** (`as_analysis::faces`, `PeopleParams`): the person
  detector runs on every 2nd sample in its own thread, on the decoded frames
  the faces use. Bodies are tracked per shot without ReID (as clean as with
  it here, and cheaper), tied to faces at the end of each shot, and stored
  with each checkpoint chunk. Episodes whose faces were found before get a
  pass for their people alone, tied to their stored faces.
- **ReID crops**: the ≤ 32 clearest sightings per body (by score × size,
  not too dark), not ≤ 8: with ≤ 8, 4 of the 45 main-cast shots of (c) are
  lost; ≤ 32 embeds 17,687 crops (18,500 without a cap).
- **Who a body is** is derived after every clustering and never shapes a
  character: faces named by their body carry `via_body`, bodies take
  `face | clothes | conflict`. (c) also needs a best detection score ≥ 0.65.
  The user's word (Not them, Not a person, pinned) is kept and put back
  after a re-analysis like a face's.
- **A clip for a pack** needs a body that shows: height ≥ 15 % of the frame,
  height/width ≤ 1.6 (not a sliver or an arm), not a headless crop at the
  top edge (unless ≥ 60 % of the frame), not a blurred shoulder in the
  foreground. Such shots join the pack marked "No face", behind every shot
  with a face (in "Best first" and in a limit); "Only with their face"
  leaves them out.

**Measured** (BB S01E01; the face pass with `faces_run`, the rest in the
app on a scratch copy of the library):

| | |
|---|---|
| face pass with people (new episode) | 276 s instead of 221 s (+25 %); face tracks, chunks and sprites byte-identical with and without |
| pass for people alone (episode analysed before) | 187.5 s, 1682 bodies (758 by their face, 59 by clothes) |
| model download in the app (two zip members, 38 MB unpacked) | ≈ 3 s |
| Walter | 302 → 323 shots (+21 "No face"), 1559 → 1794 s on screen |
| Jesse, Skyler, Hank, Krazy-8 | +8, +1, +2, +3 shots |
| faces named by their body ((b)) | 9 (the bench run's 10 had 0 wrong: F 0.925 → 0.930) |
| mode off again | all 1138 face tracks, the characters and every pack exactly as before |
| app killed after 14 of 34 chunks, started again | resumes at the last chunk; bodies, links, identities and faces identical to an uninterrupted pass |

## The test set (2026-11-15)

Bodies whose face does not show, each with a name that does not come from
the pass under test. A later rule or model for "find a character without a
face" is judged on it. Everything below was fixed in row 26 of
`bench/models/DECISIONS.md` before the runs. Nothing in the app changed.

**Provisional.** Every title is a tuning title; a body model that is ever
trained needs sealed ones. There is one live-action title. The animation
labels that pools E and B rest on are model labels, and the owner's audit of
them is open. The readers of pools M, H and N are models too (Claude Opus);
no person has checked their names.

### Titles and runs

| Title | Episodes | People | Faces |
|---|---|---|---|
| Breaking Bad | S01E01, the 960×540 proxy | the app's own people pass (`faces_run PEOPLE=`) | the same run |
| Miraculous (3D) | S01E11, S05E21 | the app's own people pass (`ANIMATED=1`) | the same run |
| Jujutsu Kaisen | S2E03, S2E04 | `bodies_run`, tracked and tied by the bench at the app's settings | the shipped Standard scan's head tracks (`+anime3`) |
| Re:Zero | 31, 33 | as Jujutsu Kaisen | as Jujutsu Kaisen |
| One Piece | Wano A, B, C | as Jujutsu Kaisen | as Jujutsu Kaisen |

Each episode is run and grouped alone, as a project of one episode, and a
title's episodes are pooled afterwards. **No scan follows an anime character
by a body.** The anime numbers say what the live-action pair (`pp_det_med`,
`reid_0265`) would do on anime; they are not something the app does.

`bodies_run` ran on Breaking Bad and Miraculous as well, so that another
body model can be put in later (`REID=<file>`). Tracked by the bench at the
app's settings (`BODY_STRIDE=6 TRACK_REID=0 REID_TOP=32`) its bodies get the
same answer as the app's own people on every query both runs hold: 283 of
283 on Breaking Bad, 167 of 167 and 139 of 139 on the two Miraculous
episodes (compared by pool, frame and name; the runs differ by 0 to 2
queries of pool E).

### Pools

| Pool | A query | Where its name comes from | What answers it |
|---|---|---|---|
| E, easy | a body track tied to a face of the main cast, with at least 3 sightings while that face does not show | the labels of its faces, through the tie of faces to bodies | today's rule for clothes, asked with the appearance of those sightings |
| B, back (anime) | the body track under a named `back` head track of the labels (most of the head's boxes sit on it) | that label | today's rule, asked with the body's appearance while the back of the head shows |
| M, medium | a body track with no face track tied at all, drawn at random; a face of its figure is labelled somewhere in its shot | two readers and a third | what the pass itself made of the track |
| H, hard | as M, but no face of its figure is labelled in its shot | as M | as M |
| N, extras | a body whose faces the labels call `?`, `?e` or `n` and that the face pass named no one; and a drawn track the readers call a person outside the gallery or no person | the labels; the readers | as M: a name of the main cast is a wrong answer |

- **Names.** One name per figure: a figure's looks count as one name, a
  transformation or a mask is a figure of its own (`GALLERY=`, as in the
  identity benches); Breaking Bad by its label names. The main cast is every
  named figure with at least 30 s on screen in the title's labels. Opening,
  ending, eyecatch, recap and preview are left out. A body of a named figure
  below 30 s is in no pool.
- **Today's rule for clothes** (`as_identity::bodies`, the shipped bars): the
  most alike character among the bodies with a face at most 3 min away, if
  it is at least 0.65 alike and 0.15 ahead of the next character.
- **Pools E and B** are asked with that rule directly. The query's own track
  is never one of the bodies it is compared with. The rule "not where the
  character already is" cannot be used: the character is in the shot. Both
  are scored twice: with every body as a possible match, and with none from
  the query's own shot.
- **Pools M, H and N** take what the pass itself did with the track: a name
  by clothes or none, with the rule "not where the character already is" and
  the detector bar of 0.65.
- **Answered**: the query got a name. **Right**: that name is the query's
  (a character of the pass is called by the label name most of its tracks'
  frames have). Three numbers per pool: the share answered (answered of all
  queries), the share right (right of the answered), and right of all
  queries. No bar was swept.
- **Intervals** (95 %) are drawn over characters: per title the figures that
  have a query are redrawn with all their queries, 2,000 times; pooled,
  within each title. Extras have no character: Wilson's interval on the
  share that got a cast name. An interval of [1.00, 1.00] on a share right
  only says that no character had a wrong answer; with a handful of answers
  it is no evidence of 100 %.

Flags in the pool files: `far` (pool B: no face of the figure is labelled
within 48 frames, 2 s, of the back of the head; the proposal's stand-in for
"another shot"), `cut` (pool B: none in the same shot, by the real shot
cuts), `own` (pool B: the body carries a face of the figure at another
moment), `mixed` (pool E: the body's faces are more than one named figure's),
`clip` (pools M, H, N: the body shows enough to be a clip in a pack, by the
app's own test), `face`, `e`, `n` (pool N: from the labels; a person outside
the gallery; no person).

### The draw and the readers

Drawn from: body tracks with no face track tied, whatever the pass made of
them, at least 15 % of the frame high (median of the visible box), outside
the spans. Two groups, up to 100 of each per title, with a fixed seed: the
shots that hold a labelled face of the main cast (where pool M can come
from), and the other shots. Readers worked on Breaking Bad, Miraculous and
one anime franchise: the one with the most such tracks, counted before any
reading (One Piece 677, Jujutsu Kaisen 375, Re:Zero 276).

A reader saw one sheet per track, in the order the tracks come on screen:
the frame with the box, another moment of the track, the box enlarged at up
to three moments, the same shot at another moment, the shot before and the
shot after; and a gallery of the main cast the episode shows (faces; for
animation also two whole figures and the gallery's description). The answer
is a gallery name, `e` (a person outside the gallery), `n` (no person) or
`x` (cannot tell). Two readers read every track apart; a third decided where
they differed, with both answers shown in no particular order. No reader saw
an answer of the pass, a group of the draw, or the other reader's answers.

| Title | Drawn | Both readers the same | Decided by the third | Named | Outside the gallery | No person | Cannot tell |
|---|---|---|---|---|---|---|---|
| Breaking Bad | 200 | 192 | 8 | 87 | 86 | 13 | 14 |
| Miraculous | 200 | 192 | 8 | 134 | 21 | 10 | 35 |
| One Piece | 200 | 170 | 30 | 82 | 26 | 49 | 43 |

Drawn from 494 and 251 tracks of the two groups on Breaking Bad, 607 and 280
on Miraculous, 267 and 410 on One Piece: 100 of each, so pools M and H are
samples of a title, not all of it. On One Piece the person detector boxes
rocks, clouds and effects (49 of 200 are no person), and the two readers
split on a form the gallery does not show (Kaido as a dragon: `e` or `x`);
the third reader there was told that such a form is `e`, a rule the first
two did not have.

Two readers who are the same model reading the same sheets agree easily:
their agreement is not accuracy.

### Counts

| Title | E | B | B `far` | M | H | H `clip` | N from the labels | N from the readers | Characters with a query |
|---|---|---|---|---|---|---|---|---|---|
| Breaking Bad S01E01 (proxy) | 76 | – | – | 28 | 59 | 42 | 68 | 99 | 12 |
| Miraculous S01E11 + S05E21 | 154 | – | – | 39 | 95 | 85 | 8 | 31 | 21 |
| Jujutsu Kaisen S2E03 + E04 | 19 | 73 | 33 | – | – | – | 41 | – | 7 |
| Re:Zero 31 + 33 | 14 | 75 | 19 | – | – | – | 13 | – | 9 |
| One Piece Wano A, B, C | 45 | 23 | 11 | 42 | 40 | 23 | 62 | 75 | 12 |
| all | 308 | 171 | 63 | 109 | 194 | 150 | 192 | 205 | |

- Pools E, M and H together: 611 queries on five titles, 194 of them hard,
  on three titles. Without the readers' pools the three kinds of footage
  hold 76, 154 and 78 easy queries.
- **Anime has far fewer easy queries than the proposal guessed** (78; the
  guess was 135 to 275). A body tied to a drawn head rarely has three
  sightings without it.
- **Pool B.** The labels hold 322 named `back` head tracks of the main cast
  outside the spans. 171 sit on a body track; 151 sit on none (the
  live-action person detector found no body there, or the head did not fit
  one). Of the 171, 63 have no face of their figure within 2 s (`far`), 158
  have none in their shot by the real shot cuts (`cut`), and 7 sit on a body
  that carries the figure's face at another moment (`own`). The proposal's
  122 counted every named figure, with or without a body under the head.
- Miraculous has 12 such labels (5 on a body in S01E11); the app's people
  keep no appearance per sighting, so they are no queries there.

### Today's pass

The shipped pair and the shipped bars, on every pool (`bodies_bench table`).
"Characters" are the figures with a query in that line.

| Title | Pool | Queries | Characters | Answered | Right | Share answered | Share right | Right of all |
|---|---|---|---|---|---|---|---|---|
| Breaking Bad | E, as today | 76 | 12 | 20 | 16 | 0.26 [0.19, 0.34] | 0.80 [0.50, 1.00] | 0.21 [0.12, 0.27] |
| Breaking Bad | E, no help from the same shot | 76 | 12 | 19 | 16 | 0.25 [0.15, 0.30] | 0.84 [0.67, 1.00] | 0.21 [0.12, 0.26] |
| Breaking Bad | M, face in the shot | 28 | 9 | 0 | 0 | 0.00 [0.00, 0.00] | – | 0.00 [0.00, 0.00] |
| Breaking Bad | H, no face in the shot | 59 | 8 | 7 | 7 | 0.12 [0.00, 0.27] | 1.00 [1.00, 1.00] | 0.12 [0.00, 0.27] |
| Breaking Bad | H, bodies that would make a clip | 42 | 7 | 6 | 6 | 0.14 [0.05, 0.32] | 1.00 [1.00, 1.00] | 0.14 [0.05, 0.32] |
| Breaking Bad | E + M + H (E without the same shot) | 163 | 12 | 26 | 23 | 0.16 [0.10, 0.21] | 0.88 [0.71, 1.00] | 0.14 [0.08, 0.20] |
| Breaking Bad | N, extras | 167 | – | 2 | – | 0.01 [0.00, 0.04] | – | – |
| Miraculous | E, as today | 154 | 19 | 52 | 42 | 0.34 [0.25, 0.42] | 0.81 [0.64, 0.93] | 0.27 [0.18, 0.37] |
| Miraculous | E, no help from the same shot | 154 | 19 | 52 | 42 | 0.34 [0.25, 0.42] | 0.81 [0.65, 0.93] | 0.27 [0.17, 0.38] |
| Miraculous | M, face in the shot | 39 | 16 | 5 | 5 | 0.13 [0.03, 0.26] | 1.00 [1.00, 1.00] | 0.13 [0.03, 0.26] |
| Miraculous | H, no face in the shot | 95 | 19 | 20 | 17 | 0.21 [0.08, 0.34] | 0.85 [0.50, 1.00] | 0.18 [0.05, 0.31] |
| Miraculous | H, bodies that would make a clip | 85 | 19 | 20 | 17 | 0.24 [0.08, 0.37] | 0.85 [0.50, 1.00] | 0.20 [0.06, 0.35] |
| Miraculous | E + M + H (E without the same shot) | 288 | 21 | 77 | 64 | 0.27 [0.19, 0.35] | 0.83 [0.69, 0.93] | 0.22 [0.14, 0.31] |
| Miraculous | N, extras | 39 | – | 0 | – | 0.00 [0.00, 0.09] | – | – |
| Jujutsu Kaisen | E, as today | 19 | 5 | 3 | 1 | 0.16 [0.00, 0.22] | 0.33 [0.00, 1.00] | 0.05 [0.00, 0.19] |
| Jujutsu Kaisen | E, no help from the same shot | 19 | 5 | 3 | 1 | 0.16 [0.00, 0.22] | 0.33 [0.00, 1.00] | 0.05 [0.00, 0.19] |
| Jujutsu Kaisen | B, back of the head | 73 | 7 | 5 | 5 | 0.07 [0.01, 0.18] | 1.00 [1.00, 1.00] | 0.07 [0.01, 0.18] |
| Jujutsu Kaisen | B, no help from the same shot | 73 | 7 | 5 | 5 | 0.07 [0.01, 0.17] | 1.00 [1.00, 1.00] | 0.07 [0.01, 0.17] |
| Jujutsu Kaisen | B far, no face within 2 s | 33 | 7 | 3 | 3 | 0.09 [0.03, 0.18] | 1.00 [1.00, 1.00] | 0.09 [0.03, 0.18] |
| Jujutsu Kaisen | B far, no help from the same shot | 33 | 7 | 3 | 3 | 0.09 [0.03, 0.17] | 1.00 [1.00, 1.00] | 0.09 [0.03, 0.17] |
| Jujutsu Kaisen | E + M + H (E without the same shot) | 19 | 5 | 3 | 1 | 0.16 [0.00, 0.22] | 0.33 [0.00, 1.00] | 0.05 [0.00, 0.19] |
| Jujutsu Kaisen | N, extras | 41 | – | 0 | – | 0.00 [0.00, 0.09] | – | – |
| Re:Zero | E, as today | 14 | 7 | 1 | 1 | 0.07 [0.00, 0.43] | 1.00 [1.00, 1.00] | 0.07 [0.00, 0.43] |
| Re:Zero | E, no help from the same shot | 14 | 7 | 1 | 1 | 0.07 [0.00, 0.43] | 1.00 [1.00, 1.00] | 0.07 [0.00, 0.43] |
| Re:Zero | B, back of the head | 75 | 8 | 4 | 4 | 0.05 [0.00, 0.07] | 1.00 [1.00, 1.00] | 0.05 [0.00, 0.07] |
| Re:Zero | B, no help from the same shot | 75 | 8 | 4 | 4 | 0.05 [0.00, 0.07] | 1.00 [1.00, 1.00] | 0.05 [0.00, 0.07] |
| Re:Zero | B far, no face within 2 s | 19 | 6 | 1 | 1 | 0.05 [0.00, 0.09] | 1.00 [1.00, 1.00] | 0.05 [0.00, 0.09] |
| Re:Zero | B far, no help from the same shot | 19 | 6 | 1 | 1 | 0.05 [0.00, 0.09] | 1.00 [1.00, 1.00] | 0.05 [0.00, 0.09] |
| Re:Zero | E + M + H (E without the same shot) | 14 | 7 | 1 | 1 | 0.07 [0.00, 0.43] | 1.00 [1.00, 1.00] | 0.07 [0.00, 0.43] |
| Re:Zero | N, extras | 13 | – | 0 | – | 0.00 [0.00, 0.23] | – | – |
| One Piece | E, as today | 45 | 8 | 2 | 1 | 0.04 [0.00, 0.09] | 0.50 [0.00, 1.00] | 0.02 [0.00, 0.04] |
| One Piece | E, no help from the same shot | 45 | 8 | 2 | 1 | 0.04 [0.00, 0.09] | 0.50 [0.00, 1.00] | 0.02 [0.00, 0.04] |
| One Piece | B, back of the head | 23 | 9 | 3 | 3 | 0.13 [0.00, 0.32] | 1.00 [1.00, 1.00] | 0.13 [0.00, 0.32] |
| One Piece | B, no help from the same shot | 23 | 9 | 3 | 3 | 0.13 [0.00, 0.32] | 1.00 [1.00, 1.00] | 0.13 [0.00, 0.32] |
| One Piece | B far, no face within 2 s | 11 | 5 | 1 | 1 | 0.09 [0.00, 0.17] | 1.00 [1.00, 1.00] | 0.09 [0.00, 0.17] |
| One Piece | B far, no help from the same shot | 11 | 5 | 1 | 1 | 0.09 [0.00, 0.17] | 1.00 [1.00, 1.00] | 0.09 [0.00, 0.17] |
| One Piece | M, face in the shot | 42 | 11 | 0 | 0 | 0.00 [0.00, 0.00] | – | 0.00 [0.00, 0.00] |
| One Piece | H, no face in the shot | 40 | 7 | 0 | 0 | 0.00 [0.00, 0.00] | – | 0.00 [0.00, 0.00] |
| One Piece | H, bodies that would make a clip | 23 | 6 | 0 | 0 | 0.00 [0.00, 0.00] | – | 0.00 [0.00, 0.00] |
| One Piece | E + M + H (E without the same shot) | 127 | 11 | 2 | 1 | 0.02 [0.00, 0.03] | 0.50 [0.00, 1.00] | 0.01 [0.00, 0.02] |
| One Piece | N, extras | 137 | – | 1 | – | 0.01 [0.00, 0.04] | – | – |
| all titles | E, as today | 308 | 51 | 78 | 61 | 0.25 [0.20, 0.30] | 0.78 [0.66, 0.89] | 0.20 [0.14, 0.26] |
| all titles | E, no help from the same shot | 308 | 51 | 77 | 61 | 0.25 [0.20, 0.30] | 0.79 [0.67, 0.90] | 0.20 [0.14, 0.26] |
| all titles | B, back of the head | 171 | 24 | 12 | 12 | 0.07 [0.03, 0.12] | 1.00 [1.00, 1.00] | 0.07 [0.03, 0.12] |
| all titles | B, no help from the same shot | 171 | 24 | 12 | 12 | 0.07 [0.03, 0.11] | 1.00 [1.00, 1.00] | 0.07 [0.03, 0.11] |
| all titles | B far, no face within 2 s | 63 | 18 | 5 | 5 | 0.08 [0.04, 0.12] | 1.00 [1.00, 1.00] | 0.08 [0.04, 0.12] |
| all titles | B far, no help from the same shot | 63 | 18 | 5 | 5 | 0.08 [0.04, 0.12] | 1.00 [1.00, 1.00] | 0.08 [0.04, 0.12] |
| all titles | M, face in the shot | 109 | 36 | 5 | 5 | 0.05 [0.01, 0.10] | 1.00 [1.00, 1.00] | 0.05 [0.01, 0.10] |
| all titles | H, no face in the shot | 194 | 34 | 27 | 24 | 0.14 [0.07, 0.23] | 0.89 [0.67, 1.00] | 0.12 [0.06, 0.21] |
| all titles | H, bodies that would make a clip | 150 | 32 | 26 | 23 | 0.17 [0.09, 0.27] | 0.88 [0.67, 1.00] | 0.15 [0.07, 0.25] |
| all titles | E + M + H (E without the same shot) | 611 | 56 | 109 | 90 | 0.18 [0.14, 0.23] | 0.83 [0.71, 0.91] | 0.15 [0.10, 0.20] |
| all titles | N, extras | 397 | – | 3 | – | 0.01 [0.00, 0.02] | – | – |

What the table says, and no more than it:

- **Live action** (one title). Of the easy queries a quarter get a name
  (20 of 76), and 16 of those 20 are right. Of the hard ones 7 of 59 get a
  name, all 7 right. Pool M gets none (0 of 28): the character's face is in
  the shot, and the rule "not where the character already is" holds the
  body back, as it is meant to where the body is a second box on the same
  person.
- **3D animation** (one title). A third of the easy queries get a name
  (52 of 154), 42 right. Of the hard ones 20 of 95, 17 right.
- **Anime.** The live-action pair names almost nothing: 6 of 78 easy
  queries (3 right), 12 of 171 backs of heads (all 12 right), none of One
  Piece's 82 queries of pools M and H.
- **Help from the same shot** hardly matters to today's rule: without it
  one easy answer of 78 is lost.
- **Extras**: 3 of 397 get a name of the main cast.
- Every share right rests on few answers. Pooled, 109 answers of pools E, M
  and H are 0.83 right, somewhere from 0.71 to 0.91; per title the intervals
  are far wider.
- The three wrong answers of pool H are all on Miraculous (two bodies of
  one figure called another, one of a third). The wrong easy answers
  cannot be read that way: see the next part.

### Pool E's names, read again

Pool E's names come through the tie of faces to bodies, and that tie is
less sure on animation: of the faces on a body that carries two or more
named faces, the share on a body of another figure is 3 % on Breaking Bad
(2 of 66) and 24 % and 41 % on the two Miraculous episodes (16 of 67, 14 of
34); on the anime episodes 0 to 33 % of at most 16 such faces. Most such bodies
count as a conflict and are no query, so this was checked directly: a
sample of pool E's bodies went to two readers on the same sheets as the
draw, shown at their moments without a face. An item on which the two
differ counts as "cannot tell"; no third reader was used.

| Title | Bodies read | The label's name | Another answer | Cannot tell |
|---|---|---|---|---|
| Breaking Bad | 40 of 76 | 37 | 2 | 1 |
| Miraculous | 40 of 154 | 26 | 5 | 9 |
| Jujutsu Kaisen | 19 of 19 | 10 | 4 | 5 |
| Re:Zero | 14 of 14 | 7 | 1 | 6 |
| One Piece | 30 of 45 | 18 | 6 | 6 |
| all | 143 | 98 | 18 | 27 |

(One Piece's 30 were part of its draw, so there the third reader decided
where the two differed.)

Where the readers gave an answer, it is the label's name for 37 of 39 on
Breaking Bad, 26 of 31 on Miraculous, 10 of 14 on Jujutsu Kaisen, 7 of 8 on
Re:Zero and 18 of 24 on One Piece. The two on Breaking Bad were looked at by
the author: both are Hank's body with another guest's face tied to it, so
the readers are right and the tie is wrong. Of the 11 other answers on
anime, 7 say the box holds no figure or one outside the gallery (the body
track wandered off its figure). **So a share right on pool E has a ceiling
below 1: about 0.95 on live action and 0.7 to 0.9 on animation, by these
samples.** A rule that is right is scored wrong that often. The names of
pool E were left as the labels give them; nothing was corrected from the
readers.

### What is counted and what is estimated

Counted, from the files of this folder: every pool size, every answer, the
readers' agreement, pool E's names read again, the 322 `back` labels and
where they sit.

Not counted:

- **The 84 queries of 2026-10-16**: "about 21 right of about 24 answered" is
  worked back from two printed numbers. Their run files are gone.
- **Whether a name is true.** On Breaking Bad the face labels are a
  person's; on the other four titles they are model labels. The names of
  pools M, H and of half of N are two model readers' and a third's. No
  person has audited either.
- **How many such bodies a title holds.** Pools M and H are a draw of 100
  and 100 tracks per title. The share of the drawn tracks that the readers
  named a figure of the main cast: 87 of 200 on Breaking Bad, 134 of 200 on
  Miraculous, 82 of 200 on One Piece.
- **What the intervals leave out.** They redraw the characters of these
  episodes. They say nothing about wrong labels, wrong readers, other
  episodes or other titles.
- **Times.** No pass was timed for this row.

### Limits

- One live-action title, at 960×540 (the remux is gone). Body models are
  made for live action, so the verdict on one needs a second such title.
- The bars of today's rule were set on Breaking Bad (the remux, 2026-10-16):
  its numbers here are not from footage the rule never saw.
- Each episode is a project of its own here. The app groups a series as one
  cast; that was not run.
- Pool E's names come through the tie (above). Pools E and B are asked
  without the rule "not where the character already is", so they say how
  the appearance alone does, not what the app would show.
- "A face in the shot" means a labelled face. The labels sit on the tracks
  of the face pass, so a face that pass missed counts as no face, and such
  a body is in pool H.
- The readers name a part of a person (a hand, an arm, legs) when the scene
  makes clear whose it is. Such a body is a query, with or without the flag
  `clip`: 44 of the 194 hard queries are no clip by the app's own test.
- Pool N from the labels holds only bodies whose faces the face pass named
  no one; an extra the face pass put on a character is not in it.
- Pool B asks the body under the back of a head. 151 of 322 such heads have
  no body track under them and are not asked at all.
- Two readers of one model reading the same sheets: their agreement (96 %
  on Breaking Bad and Miraculous, 85 % on One Piece) is not accuracy.

### Draft gate (for the owner to change; not decided)

The proposal's text (`research_notes/Bodies and clothes/proposal.md`), with
today's numbers beside it. None of it is decided.

| The draft says | Today, on this set |
|---|---|
| Rules are judged on the pooled set, and reported per footage type beside it | pooled E + M + H: 611 queries, 56 characters. Live action 163, 3D 288, anime 160 (pool B's 171 are not in that sum) |
| A live-action model is judged only after a second live-action title is labelled | one title: Breaking Bad |
| At least +10 points answered, against today's pass on the same queries | today 0.18 answered [0.14, 0.23] over E + M + H; +10 points is 0.28 |
| ... at no less than today's share right, the interval drawn over characters | today 0.83 [0.71, 0.91], on 109 answers |
| ... and not lower on pool H alone | today 0.14 answered [0.07, 0.23], 0.89 right [0.67, 1.00], on 27 answers |
| Extras named (pool N) do not rise | today 3 of 397 |
| "On their character" of the identity bench does not fall; within +25 % of the people pass's time | not measured for this row |
| A rule and a model are judged apart | – |

What the owner has to settle before the gate can be used:

1. **What "no less than today's share right" means** when today's share is
   known only to within 0.71 to 0.91: the point (0.83), the lower end, or a
   paired count on the same queries.
2. **How the pools weigh.** Pooled, pool E is half of the 611 queries and
   Miraculous is 288 of them; pool E is also the pool whose names are least
   sure. A gate per pool, or per kind of footage, reads differently.
3. **Whether pool B belongs in the gate**, and with which reading: `far` (63)
   or the real shot cuts (158).
4. **Whether a hand or an arm is a find** (the flag `clip`), or only a body
   that would make a clip.
5. **Anime.** The live-action pair is the only thing measured there and
   names almost nothing; whether it is the pass to beat at all.
6. **A second live-action title** (The Blacklist or Prison Break S01E01
   need face labels first), and **an audit by a person** of a sample of the
   readers' names and of pool E's.

### Commands

Run files are outside the repository (`E:\as-ownmodels\bodies`); the
binaries were built from a detached worktree of the commit that registered
row 26.

```sh
# People as the app finds them (live action; 3D animation with ANIMATED=1)
PEOPLE=<persons_dir> [ANIMATED=1] faces_run <ort_dir> <models_dir> <ffmpeg_dir> <bb_dir> <out_prefix>
# Every person of every sample (any title; the app's detector setting)
DETECTOR=pp_det_med:640x384:pad bodies_run <ort_dir> <persons_dir> <ffmpeg_dir> <bb_dir> <out.bodies>

# One episode. Animation labels: GALLERY, SPANS, EPISODE=<first>,<end> of the
# project's timeline; anime: ANIME=1 GRID=6,10; 3D: ANIMATED=1.
# A .bodies file at the app's settings: BODY_STRIDE=6 TRACK_REID=0 REID_TOP=32.
KEY=<episode> INFO=<bb_dir>/bb.info.json [HARD=bench/bodies/<title>.hard.tsv] [CHECK=bench/bodies/<title>.check.tsv] \
  bodies_bench pools <faces.tracks> <run.found|run.bodies> <bb_dir> <labels.tsv> <out_prefix>
bodies_bench back <faces.tracks> <run.bodies> <bb_dir> <labels.tsv>

# The draw for the readers, its sheets, and the readers' answers joined
DRAW=<m>,<o>[,<e>] bodies_bench draw <faces.tracks> <run> <bb_dir> <labels.tsv> <out_dir>
GRID=3x2 NAME=item bodies_render <ffmpeg_dir> <bb_dir> <out_dir>/items.tsv <sheets_dir>
NAME=gallery bodies_render <ffmpeg_dir> <bb_dir> <out_dir>/gallery.items.tsv <sheets_dir>
NAMES=<out_dir>/gallery.txt bodies_bench audit <out_dir>/draw.tsv <first.tsv> <second.tsv> <third.tsv|-> <out.hard.tsv>

# The tables
bodies_bench table bb=bench/bodies/bb.today.tsv miraculous=bench/bodies/miraculous.today.tsv ...
```

`bodies_render`'s own JPEG files open in an image library but not in the
readers' image tool: the sheets were saved again as plain JPEGs before any
reader saw them.

### Files

| File | What |
|---|---|
| `<title>.pools.tsv` | every query of every pool: `pool key shot start end frame box name flags` (`key` is the episode; `frame` and `box` are one sighting of the body, to find it by in another run; frames are the episode's own) |
| `<title>.today.tsv` | today's answer to each query, in the same order: the name, its likeness and the runner-up's, with every body as a possible match and with none of the same shot (`-`: no answer; `~name` in pool N: called a character outside the main cast, which is no cast name) |
| `<title>.hard.tsv` | every drawn track (Breaking Bad, Miraculous, One Piece): group of the draw (`m`: a labelled face of the main cast in the shot, `o`: other shots, `e`: a body of pool E), both readers' answers, the name, and how it was reached (`same`, `third`) |
| `<title>.check.tsv` | pool E's bodies read again (Breaking Bad, Miraculous, Jujutsu Kaisen, Re:Zero; One Piece's are in its hard file): both readers' answers; `open` where they differ |
| `bb_s01e01.audit.tsv` | the audit of 2026-10-16 (above) |
