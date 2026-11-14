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
| `miraculous.series.tsv` | the look across the series (2026-11-14): 41 projects of fifteen episodes, nine Blender films and the tuning pair, each grouped without and with the rule that keeps two tight groups apart (`footage`: `new`, `one tuning episode`, `tuning`; characters without / with; characters parted and joined; clear tracks named Marinette and Ladybug by the screen; whether one character holds both) |
| `miraculous.series.pages.tsv` | the 16 characters that rule changes there: project, tracks a side, the letters of two readers without context (`a` one figure in one look, `b` one person in two looks, `c` two different people) and what the rows show |

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

(As the app grouped 3D animation on 2026-11-13. Since 2026-11-14 a rule keeps
a hero and the everyday self apart, and the pass on both episodes reads
P 0.967 R 0.809 F 0.881: see "The hero and her everyday self" below. The
oracle's numbers of this part were not run again with it.)

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

## The hero and her everyday self: why they join, and the rule that keeps them apart

Looked into on 2026-11-13, after the scores above, and taken into the app on
2026-11-14 (row 22 of `bench/models/DECISIONS.md`). Nothing was written down
before the rule was found, and the labels that showed the problem are the
ones it was tuned on: they are tuning data for it. What it does on footage it
was not tuned on was laid down before that footage was grouped ("Across the
series", below).

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
76 clear tracks, 0.69 alike inside) that are 0.41 alike to each other. The
rule (`as_identity::ahc`, `Params::tight`, `tight_tracks`): two groups of at
least `tight_tracks` tracks each do not join when they are less alike to each
other than `tight` times the cohesion of the less coherent one. In the app it
is on in the space fitted to footage the user called animation, for the full
scan's faces, at 0.70 from 15 tracks a group; nowhere else.

| Main cast, figure level, per track | Before | With the rule |
|---|---|---|
| Both episodes, P / R / F | 0.907 / 0.807 / 0.854 | 0.967 / 0.809 / 0.881 [0.844, 0.907] |
| ... characters | 32 | 33 (Marinette 151 tracks, Ladybug 92) |
| ... main cast's time on their character | 0.918 | 0.959 |
| ... clips, precision / recall | 0.927 / 0.892 | 0.975 / 0.940 |
| ... `stress`, 27 runs: people split / characters holding two | 156 / 118 | 154 / 94 |
| S01E11 alone, P / R / F | 0.982 / 0.781 / 0.870 | the same |
| S05E21 alone, P / R / F | 0.941 / 0.788 / 0.858 | 0.973 / 0.788 / 0.871 |
| Five episodes (three unlabelled), the labelled tracks, P / R / F | 0.973 / 0.771 / 0.860 | the same; the bar is 0.423 there and the pair is two anyway |

The rule against the grouping before, per-track F: +0.027 on both episodes,
95 % [0.000, +0.063], and +0.013 [0.000, +0.030] on S05E21 alone (two of the
main figures change, so the lower end is 0). With the rule switched off
(`TIGHT=0`) the bench gives the old score files back to the letter.

How far it holds on these two episodes (the third reviewer's measurements,
and the author's):

- **The share has little room.** Over 42 amounts of footage (one episode
  whole and 0 to 20 twentieths of the other, both ways round) the two selves
  are 0.54 to 0.68 of their own cohesion alike. Of the 331 joins of one
  figure with at least 15 tracks a side that the grouping makes there
  without the rule, none lies under 0.70 and 15 lie under 0.75, the lowest at
  0.726 (15 tracks of Chloé, all of one episode, joining the rest of her).
  So on this footage a share from 0.68 to 0.73 does it. On the project of
  both episodes every share from 0.62 to 0.74 gives the table's numbers;
  0.60 leaves the pair one, 0.76 refuses a right join (F 0.867).
- **From 15 tracks.** In those 42 amounts the pair is one character in 37
  without the rule; with it from 30 tracks in 14 (a piece of one self joins
  the other before the rule is asked), from 20 in 2, from 15 in none. Under
  15 clear tracks a side the rule is idle.
- The villain's two forms stay one character (0.83 of their own cohesion):
  two looks with the same kind of mask.
- **Other footage.** The rule is not on for the anime embedder's faces, for
  live action, for footage that only looks like animation, or for Fast Scan
  (nothing was measured for SFace; on S01E11's Fast Scan tracks no share
  helped), and a Fast Scan cast of 3D animation is not grouped again for it.
  Switched on by hand, the three anime tuning franchises score the same in
  both scans (under the Best scan One Piece moves faces between two figures'
  lines, the score stays).

### Across the series

The rule was set on one pair of figures in one project. Before it was
committed, fifteen episodes were grouped without and with it; the footage,
the projects, the measures and what they would decide were written down
before any of it was grouped (`E:\as-ownmodels\3d\series\PLAN.md`, scratch;
its content is this part). No labels were made: what the rule changes was
read by people who know nothing of it.

**Footage.** Ten more episodes of the series, two a season, drawn by
`random.Random(20261114)` from the episodes not scanned before, and the five
scanned on 2026-11-13; the full pass as the app runs it on 3D animation.
Other 3D titles, as footage the rule should leave alone: nine Blender films
(Sintel, Sprite Fright, Charge, Spring, Agent 327, Cosmos Laundromat, Coffee
Run, Hero, Elephants Dream), each a project of its own.

| Key | Episode | Frames | Tracks |
|---|---|---|---|
| `mir_s1e03` | S01E03, The Pharaoh | 32,424 | 587 |
| `mir_s1e08` | S01E08, Rogercop | 32,423 | 700 |
| `mir_s1e11` | S01E11, Horrificator (tuning) | 32,423 | 806 |
| `mir_s2e13` | S02E13, Zombizou | 32,422 | 1,264 |
| `mir_s2e19` | S02E19, Sandboy | 32,422 | 410 |
| `mir_s2e20` | S02E20, Style Queen | 32,422 | 701 |
| `mir_s2e22` | S02E22, Malediktator | 32,422 | 798 |
| `mir_s3e16` | S03E16, Gamer 2.0 | 32,449 | 611 |
| `mir_s3e17` | S03E17, Stormy Weather 2 | 32,449 | 652 |
| `mir_s4e03` | S04E03, Gang of Secrets | 32,450 | 662 |
| `mir_s4e06` | S04E06, Furious Fu | 32,450 | 579 |
| `mir_s4e26` | S04E26, Strike Back | 33,574 | 586 |
| `mir_s5e01` | S05E01, Evolution | 32,907 | 665 |
| `mir_s5e16` | S05E16, Protection | 34,132 | 703 |
| `mir_s5e21` | S05E21, Confrontation (tuning) | 33,918 | 1,012 |

**Projects** (`miraculous.series.tsv`): each episode alone (15); neighbours
in air order (14); far pairs, the first eight episodes each with the one
seven places on (8); five in a row (3); all fifteen (10,736 tracks); the
nine films. 31 of the series' 41 projects hold no tuning episode, 8 hold one
of the two beside new footage, 2 are a tuning episode alone. The projects
share episodes, so they are far fewer trials than their number: the seven
projects of new footage in which the selves join rest on four of the thirteen
new episodes (S01E08, S02E22, S04E06, S05E16). Each project is grouped on one
timeline, as this bench groups (one media); a cast held as one media per
episode, as the app holds it, is what the app test below and the reviewers'
runs on the tuning episodes cover.

**Where the rule is on at all.** It runs only in a fitted space, and a space
is fitted only to footage with at least 200 pairs of clear faces seen in one
frame (`as_identity::adapt`). That holds for 40 of the 51 projects. It does
not for two episodes alone (S02E19 with 128 such pairs, S04E06 with 186) and
for none of the nine films (0 to 102): there the rule is not on, with the app
as it is.

**Measures.** (1) Every character of the grouping without the rule that the
grouping with it holds as two or more characters of at least 15 tracks each
("parted"), and the reverse ("joined": the rule keeps one join from being
made, and others then go another way). A page of faces per side, 24 a row,
read by two readers without context (Opus) who saw only the pages, in a
shuffled order and without being told which kind a page was: one figure in
one look, one person in two looks (an everyday self and a masked or costumed
one), two different people, or cannot be told. (2) A screen for the two
selves: every clear track is named by the labelled figures' mean faces of
this bench (the embedder's own space; a name needs a likeness of 0.5 and 0.1
over the second). One labelled episode naming the other is right for 0.957
and 0.986 of the tracks it names, for Marinette 85 of 98 and 54 of 54, for
Ladybug 19 of 20 and 51 of 52. A character "holds both selves" with at least
15 tracks of each name. The screen is made with the embedder under test: it
counts, the pages decide.

**What was laid down as the bar:** the rule goes in as it is if, on the
projects it was not set on, the projects with a wrong part are at most a
third of those with a right one, no Blender film has a wrong part, and
nothing is joined that is two people.

| Projects | n | Changed by the rule | The two selves one character, without / with |
|---|---|---|---|
| New footage: an episode alone | 13 | 4 | 4 / 1 |
| ... neighbours | 11 | 1 | 1 / 0 |
| ... far pairs | 6 | 2 | 2 / 0 |
| ... five in a row | 1 | 0 | 0 / 0 |
| One tuning episode beside new footage (neighbours, far pairs, fives, all fifteen) | 8 | 2 | 2 / 0 |
| A tuning episode alone | 2 | 1 | 1 / 0 |
| Blender films | 9 | 0 | (no such pair) |

So the join is no accident of one project: on new footage alone the two
selves are one character in 7 of 31 projects without the rule, and in 1 with
it. In projects of five or fifteen episodes they are two either way. Adrien
and Cat Noir, the other pair the screen names, are never one character, with
or without the rule, in any project.

**The pages** (`miraculous.series.pages.tsv`; the two readers gave the same
letter on all 16):

| What changed | Pages | Projects |
|---|---|---|
| The two selves parted | 10 | S01E08, S02E22, S05E16, S05E21 alone; three neighbours and two far pairs; the tuning pair |
| Two different people parted | 2 | S02E20 alone (the heroine in her mask and a blonde woman in sunglasses and a hat); S05E01 + S05E16 (a girl with long auburn hair and one with short red hair) |
| One figure parted in two (wrong) | 1 | S05E01 + S05E16: a red-haired figure in a white-and-blue mask, 20 and 17 tracks; the second row holds other masked faces too, and both readers called it a thin majority |
| One figure joined (right) | 3 | S05E01 + S05E16, S05E16 + S05E21, S02E22 + S05E16: a second small group of the unmasked girl (15 to 21 tracks, mostly side views) joins her once the masked self is out of her group |
| Two people or the two selves joined | 0 | |

On new footage: seven projects with a right part, one of them (S05E01 +
S05E16) also with the wrong one; nothing was joined that is two people. No
Blender film changed, but that says little: the rule was never on for them
(no fitted space; their casts are one to five characters of 28 to 190
tracks). Forced on in the embedder's own space at 0.70 and at 0.75 they group
the same as well (the fourth reviewer's runs). **The bar is met**, its clause
on the films emptily.

**Where the selves stay together.** S04E06 alone keeps the two selves on one
character (82 and 73 named tracks): no space is fitted for that episode, so
the rule is not on. Forced on in the embedder's own space it does not part
them at 0.70 and does at 0.75, which is above the lowest right join seen on
the tuning episodes (0.726). So a project too small for a fitted space (one
sparse episode, a short film) keeps a hero and the everyday self as one
character; whether the rule should run there, and at what share, was not
measured. 0.70 was not moved after this result.

### In the running app

Builds of the app with the change (and, for the rows on a moved face, with
the correction of row 23), on a scratch library (2026-11-14), driven through
its own commands; who is on which character was read from the
project's database against the labels of this bench (the app's 1,818 tracks
are the bench's).

| What was done | Result |
|---|---|
| S01E11 + S05E21 as a 3D animation series, analysed and grouped by the build of 2026-11-12 | 32 characters; one holds Marinette (158 tracks) and Ladybug (94) |
| That project opened by the new build | grouped again once at launch; 33 characters; Marinette keeps her character (157), Ladybug is a new one (94); no other character changes |
| The older cast named "Marinette" in the older build, then opened by the new one | parted the same way; the name stays with Marinette |
| The new cast, nobody named: one of Ladybug's faces moved to Marinette, grouped again, and once more | still two both times; the moved face stays with Marinette, Ladybug keeps her character (93 faces) |
| ... one face of an unlike character (Nino) moved to Alya, grouped again | two; Alya has her 127 faces and the moved one, Nino his 118 |
| ... Marinette named and one of Ladybug's faces moved to her, grouped again | still two; the moved face stays where it was put |
| ... Ladybug merged into Marinette by hand, grouped again | one character (158 + 94) |
| ... Marinette merged into Ladybug by hand, grouped again | one character |
| Three episodes: two unlike characters merged by hand (one into a named other), grouped again | one character, the name in place; a build without the second change left the name on the merged-in faces and made the character's own 112 faces a new one |

A merge by hand holds because of a second change made with the rule: a
character the user pinned faces to keeps its own faces
(`bench/identity/README.md`, "A merge by hand holds"). The rule itself knows
no pins.

**What this means for casts made before.** A cast of 3D animation from the
full scan is grouped once more at the next launch. Two selves that were one
character become two, also when the user had named that character: the name
stays with the self the name's pinned faces are of (the bigger one, as a
rule), and the other self goes on as a new character without a name, a
cover picked anew and no scenepack. If the pinned faces are of the smaller
self, the two stay one. Two selves merged by hand stay merged (with the
limit `bench/identity/README.md` names: a merged-in self that later outgrows
the character's own faces).

Not verified here: a second labelled 3D title (the 0.70 and the 15 stand on
this series); the readers of the pages are models, and the screen is the
embedder's own; the name look-up that follows a regrouping (it was off in
the app test: whether it proposes the new character back into the old one
was not seen); Fast Scan with the rule.

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
