# 3D animation bench: the first test set

Two episodes of Miraculous with a name for every face track, made on
2026-11-13 for the own-models plan (`bench/models/README.md`, "Before any
training"). The series is a tuning franchise of the 3D set and never trains a
model (row 14 of `bench/models/DECISIONS.md`); what the checks on it say is
row 16.

**Provisional.** The labels were made by language-model labellers, not by
people; the human audit of 150 items that freezes labels
(`bench/anime/README.md`, "Labels") is prepared and not done. It is one
franchise of the six the README asks for, and the labels sit on the tracks the
shipped pass makes, so they say nothing about faces it missed.

## Files

| File | What |
|---|---|
| `media.tsv` | the two episodes: key, minutes, size, frame rate, SHA-256 of the file used (the media are not in the repository) |
| `miraculous.gallery.tsv` | the 30 look codes with person, type and a description (`bench/anime/FORMATS.md`, "gallery.tsv") |
| `miraculous.exemplars.tsv` | per code the tracks whose clearest face is its picture in the labellers' gallery (`code key track frame box`) |
| `miraculous.labels.tsv` | a name for each of the 1,818 tracks (`FORMATS.md`, "Track labels") |
| `miraculous.labelling.tsv` | per label, in the same order: both labellers' answers and what happened after them (`adjudicated`: the two differed and a third reader decided; `second look`: neither could name the face; `majority`: a track that changes character, named by the count of ten moments; `code added`: a code added to the gallery from the notes) |
| `miraculous.spans.tsv` | opening and ending of each episode, found by eye and set on shot cuts |

The formats are the anime bench's. Both episodes are one project on one
timeline: the second episode's frames are offset by the first's.

| Media (`media.tsv`) | Episode | Frames | Offset | Tracks |
|---|---|---|---|---|
| `mir_s1e11` | S01E11, Horrificator | 32,423 | 0 | 806 |
| `mir_s5e21` | S05E21, Confrontation | 33,918 | 32,423 | 1,012 |

They were picked for a broad cast that comes back (the whole class is in
both), one early and one late: between them the villain has another form, a
student has new clothes, and several characters are new.

## How the labels were made

As the anime tuning labels were (`bench/anime/tuning.md`), with two labellers
on every track.

- **Tracks.** The face pass as the app runs it on 3D animation: `faces_run`
  with `ANIMATED=1` (`glintr100`, SCRFD-10G at three scales, threshold 0.5),
  built from a detached worktree of `036fb18f`; shots from `shot_signals` and
  `fuse_signals` without `--anime` (3D animation fuses its shots as live
  action does).
- **Sheets.** `ORDER=time identity_bench sheets`: 64 tracks a sheet, in the
  order they come on screen, never by a grouping. Per track the labeller saw
  its thumbnail, three moments (first, middle, last face, cut wider than the
  thumbnail) and the frame of the thumbnail with the face boxed. No labeller
  or reviewer saw a grouping or a score.
- **Gallery.** A scout per episode (a language-model agent with a draft cast
  written from memory) went through every sheet, corrected the descriptions
  and pointed at four to eight clear tracks per look; the pictures of every
  code were then checked by eye. Each look has its own code: a hero's masked
  self and a villain form are type `transformation`, another identity of a
  character type `mask`, so they are figures of their own
  (`bench/anime/FORMATS.md`; the owner's decision of 2026-10-31 that a hero
  and the everyday self are two characters). `GABRIEL` and `AUDREY` were added
  afterwards from the labellers' notes (two tracks each, both labellers named
  them).
- **Labellers.** Two readers per episode half, each a language-model agent
  (Claude Opus 5.5) without any other context: the rules, the gallery, its
  sheets, a folder of its own. They were told to enlarge a face before giving
  up on it.
- **Third reader.** A further agent decided the tracks the two named
  differently, from larger pages (five moments, the frame) with both answers
  shown in a random order, and looked again at the tracks neither could name.
- **Tracks that change character.** The 23 tracks either labeller flagged as
  jumping to another face were shown over ten moments to another agent: one
  character in seven or more of the ten gives the track that name, otherwise
  it is `?u` (left out of the scores). No track was split.

Rules that differ from the anime ones, or that the labellers asked about:

- A face on a poster, a photo, a statue, or a screen seen as an object in the
  scene is `n`; a call or broadcast that fills the frame shows the character.
  So the mayor on a hologram over a desk is `n` (5 tracks, 25 s), and a pass
  that puts those tracks with the mayor loses precision for it.
- A face in a mirror or behind glass is the character.
- The villain of S05E21 turns people into copies of herself: every copy is
  `REFLEKTA`, also two in one frame.

| Episode | Tracks | Both labellers the same | Both gave a code: the same | Decided by the third reader: first's answer / second's / neither | Second look at `x`: named / still open | Changing character: one name / `?u` | Final: named / `?e` / `n` / `x`, `?u` |
|---|---|---|---|---|---|---|---|
| `mir_s1e11` | 806 | 806 (100 %) | 790 of 790 | 0 | 1: 0 / 1 | 14: 9 / 5 | 785 / 12 / 3 / 6 |
| `mir_s5e21` | 1,012 | 1,003 (99.1 %) | 956 of 959 | 9: 1 / 5 / 3 | 3: 1 / 2 | 9: 5 / 4 | 966 / 14 / 26 / 6 |
| all | 1,818 | 1,809 (99.5 %) | 1,746 of 1,749 (99.8 %) | 9: 1 / 5 / 3 | 4: 1 / 3 | 23: 14 / 9 | 1,751 / 26 / 29 / 12 |

The two labellers' notes and flags differ (36 flags on the first episode), so
they did not copy each other; 3D characters are simply easier to tell apart
than drawn ones (the anime labellers agreed on 87 % of the tracks). Where the
labels are weakest: a labeller saw three moments of a track, so a track that
changes character between them was only caught when one of the three showed
it.

Not done: the labeller calibration on AOT Characters; shot presence (the main
cast comes from the labels' spans); keyframe ground truth (faces the detector
missed); the 150-item human audit.

## The pass as shipped, and what a perfect embedder would reach

The plan was written down before a track was labelled: the number that
decides, the oracle, the rule and how its interval is drawn. Scored as
`bench/anime/tuning.md` scores anime, with the bars the app uses for 3D
animation (the defaults, `animated` on, the footage's fitted space). The bars
were not swept.

```
ANIMATED=1 MAIN_SECS=15 BOOT=2000 FPS=24000/1001 LEVEL=figure \
GALLERY=bench/3d/miraculous.gallery.tsv SPANS=bench/3d/miraculous.spans.tsv \
SEGMENTS=miraculous.segments.tsv EPISODES=32423 ITEMS=miraculous.std.items.tsv \
identity_bench score miraculous.std.tracks bench/3d/miraculous.labels.tsv
```

The oracle is the third rung of the oracle ladder (`bench/anime/README.md`):
every named track gets the unit vector of its figure, every other track one of
its own, passed in as `EMB_OVERRIDE`; tracks, labels, bars and scoring stay.
The first two rungs (ground-truth boxes, ground-truth tracks) were not run:
the labels sit on the pass's own tracks.

Main cast (figures with at least 15 s on screen by the labels' spans), figure
level, per track, opening and ending left out:

| | Both episodes, one project | S01E11 alone | S05E21 alone |
|---|---|---|---|
| Tracks scored / main figures | 1,689 / 26 | 744 / 17 | 945 / 24 |
| The pass | P 0.907 R 0.807 **F 0.854** [0.805, 0.897] | P 0.982 R 0.781 F 0.870 [0.831, 0.896] | P 0.941 R 0.788 F 0.858 [0.810, 0.902] |
| Perfect embeddings | P 1.000 R 0.953 **F 0.976** | P 1.000 R 0.956 F 0.977 | P 1.000 R 0.952 F 0.975 |
| Share of the gap to 1 closed | **0.836**, 95 % [0.737, 0.897] | 0.825 [0.733, 0.904] | 0.827 [0.689, 0.909] |
| Characters, pass / perfect | 32 / 27 | 18 / 17 | 32 / 25 |
| Screen-time F, pass / perfect | 0.914 / 0.993 | 0.956 / 0.997 | 0.901 / 0.990 |
| Main cast's time on their character | 0.918 / 0.993 | 0.962 / 0.997 | 0.942 / 0.989 |
| Merges to clean | 5 / 0 | 1 / 0 | 4 / 0 |
| Clips, precision / recall (pass) | 0.927 / 0.892 | 0.992 / 0.943 | 0.952 / 0.898 |

By the label-oracle rung of the README's ladder the pass is embedder-bound on
these two episodes: perfect embeddings close 0.84 of the per-track gap, and
the lower end of the interval is above two thirds. The two episodes alone give
the same; they share the cast, the gallery and the labellers, so that shows
the number does not hang on one episode, not a second measurement. Other
readings of the same question stay above the bar: the main cast from 30 s (21
figures) 0.859 [0.797, 0.902]; the person level, where a hero and the everyday
self are one (22 persons; the pass P 0.975 R 0.753 F 0.850, perfect 0.975)
0.834 [0.773, 0.882]; per screen time 0.92.

Three things say that this is less room for an embedder than the number
suggests. A reviewer who had not seen the work found them in the score files;
each was run again afterwards.

1. **The largest error can be mended in the grouping.** Marinette
   and Ladybug are one character in the pass on both episodes (152 + 89
   tracks) and on S05E21 alone. The same embedder without the per-title
   fitted space keeps them apart (130 tracks, 99 % hers, and 81 tracks, 100 %
   hers), and so does the fitted space on S01E11 alone. The fit buys recall
   (the pass without it: P 0.977 R 0.663 F 0.790, 46 characters) and pays
   with this merge (precision 0.977 to 0.907). With Ladybug counted as
   Marinette the pass reads F 0.880 and has 0.959 of the main cast's time on
   their character: 0.026 of the 0.146 gap per track, and 0.041 of the 0.075
   the oracle wins over time, is a grouping matter that needs no training.
   (The share of the gap closed is then 0.801 [0.717, 0.865].) Adrien and Cat
   Noir stay two characters in every run. Why the pair joins, and a rule that
   holds it apart, is a section of its own below.
2. **Most of the rest sits on tracks of a few frames.** The pass leaves 132
   of the 1,639 main-cast tracks without a character (unsure or unassigned);
   113 of them are below the grouping's own bar for a clear track (quality
   under 0.45 or a single face), their median length is 3 frames, and
   together they hold 2.5 % of the main cast's frames. The labels name such a
   track from the scene, as the rules ask (a forehead behind someone, a lower
   face); the oracle gives it its figure's exact vector; an embedder sees
   only the crop. With perfect embeddings on the clear tracks alone, and the
   unclear ones placed as the pass places them, 0.32 of the gap closes (0.42
   to 0.48 under kinder readings of where they go). On the clear tracks alone
   the pass stands at 0.923, at 0.955 with the hero pair counted as one, the
   oracle at 0.993.
3. **The oracle knows the product's figures.** Its vectors put a hero and the
   everyday self at right angles, as the product wants them. Vectors per
   person (a model that sees through the mask), scored at the figure level,
   close 0.353 [0.048, 0.646]. Labels to train a 3D embedder on would have to
   be figures.

So the 0.12 of per-track F that perfect embeddings add splits into about 0.03
for the hero merge, 0.05 to 0.075 that needs tracks of a few frames read, and
0.02 to 0.04 on clear tracks.

What else the pass gets wrong, from who is in each character (`identity_bench
chars`): the villain's two forms and his everyday self are one character
(Monarch 10, Hawk Moth 7, Gabriel 2 tracks), Lila's other identity (1 track)
is with Lila; small second characters of Adrien, Rose, Kim, Mylene and
Reflekta (4 to 13 tracks each); 17 `n` tracks sit in main characters, mostly
the character's own face on a picture or a screen (the mayor's hologram, Kim's
picture on papers), which is the labelling rule and not a wrong face (with
every `n` left out the pass reads 0.858).

About the oracle itself:

- 3D animation is grouped in a space fitted per title, and `animated` forces
  that fit. Exact one-hot vectors are a degenerate input for it (strangers are
  exactly 0 alike, so the bar its seeds are cut at is 0): through it the
  oracle reads P 0.879 R 0.918 F 0.898. The plan had said so before any score
  and fixed the oracle's score as the higher of the fitted space on and off
  (`ADAPT=0`); the table holds the run with it off. Looked at afterwards: a
  nearly perfect embedder through the fitted space as the app runs it, cosine
  = (1 − a) · real + a · truth, reaches 0.972 at a = 0.5 and 0.971 at 0.97,
  and closes 0.805 [0.688, 0.875] and 0.799 [0.677, 0.871] of the gap. Through
  the path the app takes, the interval's lower end is at about two thirds,
  not at 0.74.
- What stops the oracle at 0.976: 41 main tracks without a character, each
  beside another track of its figure (19 share a frame with it, 22 overlap it
  in time). One figure found twice in a frame, which stopped the anime oracle
  at 0.84, is rare here: 41 of 1,751 named tracks (2.3 %), 13 of them the
  villain's copies.
- Checks of the scoring: the pass's own embeddings passed in the same way give
  its score back to the last digit; oracle vectors dealt to the tracks at
  random give F 0.084.
- Also looked at afterwards: every figure's tracks moved onto their mean real
  embedding (no spread inside a figure, `glintr100`'s own distances between
  figures) reach 0.929 with the fitted space and 0.900 without.

The anime scans stood at 0.34 to 0.50 on their labels (`bench/anime/tuning.md`,
three franchises, other labels): no measure of which footage is harder, but
the 3D pass is not in that state. This cast is also kind to any recogniser: a
school class with a hairstyle, glasses or a cap each.

## Fast Scan on 3D

The Fast Scan oracle of `bench/models/README.md`, on S01E11: `fast_run` with
`ANIMATED=1` as shipped (SCRFD-2.5G at 1280 and 320, SFace) and with
`EMBEDDER=arcface_glintr100.onnx NORMS=14,22`, scored with `GRID=6,8` (the
SFace run with `SFACE=1`) against the same labels; a Fast Scan track takes the
label it lies on.

| | SFace (as shipped) | `glintr100` | Perfect embeddings on the SFace run's tracks | ... on the `glintr100` run's |
|---|---|---|---|---|
| Tracks / faces | 645 / 2,324 | 675 / 2,347 | | |
| Main cast per track P / R / F | 0.900 / 0.729 / 0.805 | 0.904 / 0.868 / 0.886 | 1.000 / 0.931 / 0.964 | 1.000 / 0.945 / 0.972 |
| Main cast over time: in a track / on their character | 0.729 / 0.661 | 0.733 / 0.654 | 0.729 / 0.693 | 0.733 / 0.702 |
| Main cast clips, precision / recall | 0.928 / 0.705 | 0.914 / 0.747 | 1.000 / 0.801 | 1.000 / 0.829 |

`glintr100` groups Fast Scan's tracks better (per-track F +0.080, 95 %
[+0.008, +0.140]) but puts no more of the main cast's time on their character
(0.654 against 0.661; the gate asks for +0.02). So by the README's check the
slot is not opened. That is a verdict on `glintr100`, which is no strong
embedder for 3D, not on the room: perfect embeddings add 0.032 to 0.048 of
the time on character, above the gate, and 8 to 10 points of clip recall
(0.705 to 0.801, 0.747 to 0.829).

Fast Scan does find the faces here. The full pass's main-cast tracks that
Fast Scan found at least once (the same face in the same frame) hold 92.8 %
of the main cast's time (567 of 736 tracks; the ones never found have a
median length of 6 frames). It looks at a track about once a second (24
frames between a track's faces), and the time measure counts a labelled face
as held only where a look lies close by, so "in a track 0.729" is mostly the
time between looks, and the gate's measure is capped by the sampling whatever
the embedder. What no embedder brings back shows in the clips: recall 0.80 to
0.83 with perfect names. `stress` was not run (the gain the gate asks for
first is not there).

## The hero and her everyday self: why they join, and a rule that keeps them apart (not adopted)

Looked into on 2026-11-13, after the scores above. Nothing was written down
beforehand, and the labels that showed the problem are the ones the rule was
tried on: they are tuning data for it. No reviewer has read this part.

**Why they join.** Mean similarity of clear tracks, both episodes as one
project:

| | The embedder's own space | The fitted space |
|---|---|---|
| Inside Marinette / inside Ladybug | 0.603 / 0.630 | 0.667 / 0.696 |
| Marinette to Ladybug | 0.428 | 0.400 |
| Adrien to Cat Noir | 0.249 | 0.211 |
| Monarch to Hawk Moth | 0.571 | 0.596 |
| Strangers (two clear faces in one frame), median / 95 % | 0.257 / 0.502 | 0.153 / 0.400 |
| The bar two groups join at | 0.480 | 0.39986 |

Of the 325 pairs of figures, Marinette and Ladybug are the third most alike
in the embedder's space and the second most alike in the fitted one (behind
the villain's two forms). The fitted space sets its bar where 95 % of
strangers lie below; the pair's 0.400 sits on that bar; and one person in two
looks never shares a frame with herself, so nothing forbids the join. The
pairs just below (Alya and Sabrina, 0.388) do share frames. Without the fit
the bar is 0.480, raised by how alike strangers look: that holds the pair
apart and splits everyone else (F 0.790). So the embedder does not tell the
two apart well in either space; where the bar lies decides.

**A higher bar** is a narrow fix. `LINK` in the fitted space: 0.42 still one
character; 0.44 two (P 0.968 R 0.798 F 0.875); 0.46 F 0.862; 0.48 0.842; 0.52
0.809.

**Two tight groups stay two.** What marks the pair is not how alike the two
are but how much less alike than each is in itself: two large groups (146 and
76 clear tracks, 0.69 alike inside) that are 0.41 alike to each other, 0.59 of
their own cohesion. Of the 116 joins of two groups with at least 5 tracks
each that a plain average-linkage clustering makes in the fitted space, this
is the lowest such share; the lowest join of one figure's own tracks is 0.68,
0.73 where both sides hold at least 10 or 20 tracks, and 0.95 where both hold
at least 30. A prototype of the rule (`Params::tight` and `tight_tracks`, off
by default; a patch, in no commit): two groups of at least `tight_tracks`
tracks each do not join when they are less alike to each other than `tight`
times the cohesion of the less coherent one.

| Both episodes, main cast, figure level | As shipped | The rule at 0.70, from 30 tracks |
|---|---|---|
| Per track P / R / F | 0.907 / 0.807 / 0.854 | 0.967 / 0.809 / 0.881 [0.844, 0.907] |
| Characters | 32 | 33 (Marinette 151 tracks, Ladybug 92) |
| Main cast's time on their character | 0.918 | 0.959 |
| Clips, precision / recall | 0.927 / 0.892 | 0.975 / 0.940 |
| `stress`, 27 runs: people split / characters holding two | 156 / 118 | 154 / 94 |

The rule against the shipped grouping: +0.027 per-track F, 95 % [0.000,
+0.065] (two of the 26 figures change, so the lower end is 0). The share of
the gap perfect embeddings close is then 0.800 [0.713, 0.867].

How far it holds:

- Every setting from 0.62 to 0.72 with 5 to 30 tracks gives these numbers;
  from 30 tracks, every setting up to 0.85. At 0.60 the pair joins; at 0.76
  from 20 tracks or fewer a right join is refused (F 0.862 to 0.867).
- S01E11 alone is unchanged up to 0.72 (the pair is two there anyway); at 0.76
  from 10 tracks it falls from 0.870 to 0.849.
- **S05E21 alone is not fixed** at a safe setting. Ladybug has 24 tracks there;
  the pair comes apart only at 0.72 from 10 tracks or fewer (F 0.858 to
  0.871), one step from where right joins break. The rule needs groups large
  enough to know their own cohesion: more footage in a project, not less.
- The villain's two forms stay one character (0.83 of their own cohesion):
  two looks with the same kind of mask.
- Other footage: on the three anime tuning franchises, both scans, every score
  is the same at 0.70 from 10, 0.70 from 30 and 0.80 from 30 (the rule never
  applies); on live action (Breaking Bad, Fast Scan's tracks with `glintr100`)
  likewise.

**Not adopted.** One pair of one title carries it; the room between "the pair
comes apart" and "right joins are refused" is known only here; a single
episode is not helped; and no other 3D title is labelled. To take it into the
app: where it is on (the fitted space only, or everywhere), a new
`as_identity::VERSION` (every 3D project groups again), unit tests, and a
second 3D title. The patch and the scripts are in the side-session notes
(`3d-hero-merge`).

## Not verified

- Anything outside these two episodes of one series. The intervals redraw
  the 26 main figures of this project; they say nothing about another
  episode, another title, label errors or a project of another size (one
  episode alone and two together group the same faces differently). The
  README asks for six tuning and six sealed franchises per footage type.
- The first two rungs of the oracle ladder. Faces the detector missed are not
  in the test set; "in a track 1.000" is true by construction.
- The labels against a person. Both labellers are the same model reading the
  same gallery, so their agreement is not accuracy; a label error counts as
  embedder gap, because the oracle is built from the labels. The 150-item
  audit is prepared in the scratch folder `E:\as-ownmodels\3d\audit` (pages,
  sheet, gallery; `scripts\audit.py check` gives the error rates once the
  sheet is filled).
- Whether a 3D embedder could read the tracks of a few frames: quality is
  `glintr100`'s own measure, so some may only be unclear to it.
- The fitted space's own constants (they have no switch); the bars. A coarse
  probe by the reviewer (16 settings of `ASSIGN`, `MARGIN`, `CORE_QUALITY`,
  `LINK`, `MIN_COH`, on these labels) moved F between 0.836 and 0.863.
- The two episodes as an app project with one media per episode (the bench
  puts every track on one timeline with one media id).
- Pass times: a training run held the graphics card throughout (23 to 25
  frames a second for the full pass, which says nothing about the pass).
