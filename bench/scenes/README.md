# Scenes bench

Which shots belong to one scene, measured against scene boundaries a person
set. It serves the scenes slot (`bench/models/DECISIONS.md`, rows 6 to 10):
all of a character's scenes, with the shots in which the character is not to
be seen.

The media are not in the repository; the files hold frame numbers and names
only. Frames are on the decoded frame grid, as in `bench/shots`.

## Breaking Bad S01E01 (`bb_s01e01.scenes.tsv`)

The episode of `bench/identity` and `bench/shots` (58 min, 83,593 frames,
23.976 fps), 33 scenes. `frame` is the first frame of the scene's first shot,
`source_s` the scene's start in the source's seconds, `speakers` who speaks in
the scene and for how many seconds (`name:seconds`, most first; those under
1 s are left out).

The scenes and the speech turns come from **Serial Speakers** (Xavier Bost,
Vincent Labatut, Georges Linarès: "Serial Speakers: a Dataset of TV Series",
2020; Zenodo record 6815775, file `bb.json`, CC BY 4.0). Changed here: the
times are turned into frames of this episode, and each scene's speech turns
into one sum per speaker. The source's times follow a 25 fps disc that plays
the same pictures faster, so a frame here is its second × 25, with no shift:
751 of its 819 shot cuts lie on a boundary of the project's shots, 753 within
2 frames. A scene stands at the shot whose first frame is nearest to its
start: five starts fall into black or a fade and stand at the first shot
after it; one lies 40 frames into a shot of the title sequence, after a
dissolve, and stands at that shot.

```sh
cargo run --release -p as-project --example scenes_bench -- labels bb.json 1 1 <project_copy_dir> > bench/scenes/bb_s01e01.scenes.tsv
```

## Three more episodes, labelled here

`pb_s01e01` (Prison Break S01E01, 584 shots, 40 scenes), `bl_s01e01` (The
Blacklist S01E01, 1,003 shots, 28 scenes) and `bxm_s01e01` (Bass X Machina
S01E01, 3D animation, 679 shots, 22 scenes), from the owner's library. No
public scene labels exist for them. `frame` is the first frame of the scene's
first shot, `sure` whether both labellers were sure, `note` where and what.

Two labellers (AI agents) each read contact sheets of the episode's shots in
time order (`tools/slot-bench/shot_sheets.py`), blind to each other and to
any other label, and named each scene's first shot. A scene is a run of shots
with one place, one stretch of time and one action; cutaways, answer shots
and a continuous walk or drive stay in it; cutting back and forth between two
places of one action is one scene; a title sequence, a recap and the credits
are scenes of their own. The starts both labellers name are the labels
(`scene_merge.py`); the starts only one names are left out (4, 3 and 3, all
marked `maybe` by the one who named them). Their own files are in `check/`.

How far such labels can be trusted was checked first on Breaking Bad S01E01,
against the human labels above (`scenes_bench agree`):

| Labels | Scenes | Boundary P / R / F against the human labels | Overlap |
|---|---|---|---|
| Labeller A | 34 | 0.909 / 0.938 / 0.923 | 0.909 |
| Labeller B | 36 | 0.886 / 0.969 / 0.925 | 0.939 |
| The starts both name | 33 | 0.938 / 0.938 / 0.938 | 0.938 |

The two agree with each other at F 0.941 there, 0.951 on Prison Break, 0.947
on The Blacklist and 0.933 on Bass X Machina. Where they part from the human
labels it is over what a scene is (a bedroom after the laundry room the same
night, the logos after the credits), not over what is in the pictures. These
three episodes have no identity labels: the project's own cast says who is
where (the stored characters with at least 30 s on screen: 18, 9 and 13), so
their results measure the scenes alone, not wrong faces.

## The measure

`scenes_bench score` reads a COPY of a project (`project.db` and its `-wal`
in a folder of their own): the shots, the frames' colour signatures and the
characters the cast shows in each shot. It gives every gap between two shots
a score, 0..1, for a new scene there; scenes come from a threshold on it, so
a higher threshold only joins scenes and never reshuffles them.

- **A character's scenes** (the gate): for each main character (named in
  `bench/identity`, at least 30 s on screen; 12 here), the shots asked for
  are all shots of every labelled scene the character is seen in. The shots
  given are all shots of every scene under test with a shot of the character.
  Precision, recall and their F by shots; the mean over the characters. `true
  cast` takes where the character is from the identity labels, `stored` from
  the project's cast (the character most of their faces are on), so the
  second also pays for wrong faces.
- **Boundaries**: precision, recall and F of the gaps cut, the average
  precision of the scores (gaps with one and the same score count as if the
  labelled ones among them were spread evenly), and F when a boundary one
  shot off still counts (a found boundary answers for one labelled boundary
  at most, and the other way round).
- **Overlap**: each scene's best intersection over union with a scene of the
  other side, weighted by its frames, the mean of both directions.

A threshold chosen on the episode it is read on flatters it. `score` also
reads each half of the episode (by scenes) with the threshold that is best on
the other half; `tools/slot-bench/scene_titles.py` runs several episodes and
gives each one's result at its own best threshold, at the one threshold that
is best over all of them, and at the threshold that is best over the OTHER
episodes. That last number leaves out the threshold only: the weights and the
length of a short piece were chosen by comparing it over these same
episodes, so it flatters what a new title would get.

```sh
# Step A: what the project stores (colours, cast, what lies between shots)
SHORT=40,1 cargo run --release -p as-project --example scenes_bench -- score <project_copy_dir> bench/scenes/bb_s01e01.scenes.tsv bench/identity/bb_s01e01.labels.tsv
# Step B: a picture embedding of three frames per shot
cd tools/slot-bench && uv run shot_embed.py <ffmpeg> <video> <project_copy_dir> <media> <dinov2-small model.onnx> <resnet50_places365.pth.tar | -> <out_prefix>
SHORT=40,1 W=0,0 EMB=1:<out_prefix>.dino_cls.emb cargo run --release -p as-project --example scenes_bench -- score …
```

`SWEEP=1` lists a grid of settings, `STATS=1` how alike two shots in a row are
inside a scene and across a boundary, `DETAIL=1` every gap with its score.
Its unit tests run with `cargo test -p as-project --example scenes_bench`.

## How the scores are made

- **Two alike shots hold what lies between them together.** Each pair of
  shots up to 8 apart is as alike as its cues say (0..1); the link weakens by
  0.9 per shot it reaches over, to a half across black or a fade and to 0.8
  across a dissolve. A gap's score is 1 minus the strongest link over it. So
  an answer shot, a cutaway or a reaction between two shots of one place
  stays in their scene.
- **Cues.** Step A: the colours (the closest pair among five frames' stored
  signatures, 0.7) and the share of known characters two shots have in common
  (0.3; says nothing where a shot shows nobody the cast knows). Step B: the
  cosine of the closest pair among three frames' embeddings, measured from
  what shots 100 or more apart have in common in this title.
- **Short pieces join first** (`SHORT=<seconds>,<curve>`, added after the
  first test, row 7). The shots are joined into ever longer pieces, the
  weakest boundary first; a boundary beside a piece shorter than 40 s counts
  for that share of its score. The first runs cut scenes at cutaways, at a
  scene's first shot and all through montages.

## Results (2026-11-12, thresholds chosen on the episode)

794 shots, 33 labelled scenes. The stored cast is almost the labels' here
(with the labelled scenes it reaches 0.994), so `stored` is given.

| Scenes | Scenes found | Boundary P / R / F | F, one shot off | Average precision | Overlap | A character's scenes (F) | Other half's threshold |
|---|---|---|---|---|---|---|---|
| Every shot its own scene | 794 | 0.040 / 1.000 / 0.078 | – | – | 0.176 | 0.450 | – |
| One scene | 1 | – | – | – | 0.060 | 0.337 | – |
| A new scene at each fade to black | 18 | 0.235 / 0.125 / 0.163 | – | – | 0.404 | 0.698 | – |
| Step A | 174 | 0.145 / 0.781 / 0.244 | 0.263 | 0.195 | 0.514 | 0.832 | 0.764, 0.829 |
| Step A, short pieces join first | 41 | 0.425 / 0.531 / 0.472 | 0.500 | 0.390 | 0.616 | 0.892 | 0.785, 0.909 |
| Step B, Places365 ResNet-50 | 118 | 0.231 / 0.844 / 0.362 | 0.376 | 0.478 | 0.644 | 0.890 | 0.849, 0.854 |
| Step B, Places365, short pieces join first | 43 | 0.405 / 0.531 / 0.459 | 0.649 | 0.448 | 0.659 | 0.924 | 0.834, 0.924 |
| Step B, DINOv2-small | 108 | 0.290 / 0.969 / 0.446 | 0.446 | 0.542 | 0.728 | 0.880 | 0.826, 0.862 |
| **Step B, DINOv2-small, short pieces join first** | 37 | 0.556 / 0.625 / 0.588 | 0.735 | 0.589 | 0.762 | **0.931** | 0.839, 0.912 |
| The labelled scenes, stored cast | 33 | 1 / 1 / 1 | 1 | – | 1 | 0.994 | – |

- The stored colours are a weak sign of the place: step A finds the
  boundaries poorly (average precision 0.20). A general picture network
  (DINOv2-small, its class token) is the best single cue; the place network
  is behind it, and both together are no better than DINOv2 alone (0.532
  against 0.584 at 60 s).
- The cast does not help to find boundaries: with the share of characters two
  shots have in common the average precision falls from 0.532 to 0.442. The
  lead is in nearly every scene. The cast answers which scenes are a
  character's, not where they end.
- The rule for short pieces is worth 0.05 on a character's scenes with either
  step, and it holds from 20 s to 90 s (DINOv2: 0.912, 0.931, 0.939, 0.938).
- With the other half's threshold the first half reads 0.84 and the second
  0.91: the threshold is the open point, and one episode cannot settle it.
- The worst characters at 40 s are Emilio (0.74) and Krazy-8 (0.85), whose
  long scenes in the desert, with many set-ups, are still cut in two or
  three (recall 0.59 and 0.74), and Bogdan (0.79), whose short scenes are
  joined with a neighbour (precision 0.66). Nine of the twelve are at 0.9 or
  above.
- One wrong face gives a whole wrong scene. Here the only such case is a face
  the identity labels leave out as not to be told.
- Three frames per shot cost 36 s to decode and 38 s through DINOv2-small on
  the development PC's graphics card for this episode.

**After the label corrections of 2026-11-15** (`bench/identity`: three slips
in the face labels, six names; row 30 of `bench/models/DECISIONS.md`). The
Breaking Bad numbers of this file dated 2026-11-12 were measured with the
labels of that day and stay as they were. The same commands with the
corrected labels, a character's scenes by the stored cast:

| | 2026-11-12 | Corrected labels |
|---|---|---|
| DINOv2-small, short pieces join first (the bold row above) | 0.931 | 0.934 |
| Step A, short pieces join first | 0.892 | 0.894 |
| The labelled scenes, stored cast | 0.994 | 1.000 |
| The other half's threshold: first half, second half | 0.839, 0.912 | 0.932, 0.915 |
| Four episodes (row 8): own best threshold, one for all, the others' | 0.930, 0.917, 0.917 | 0.935, 0.921, 0.921 |
| ... the means of the four | 0.908, 0.887, 0.875 | 0.909, 0.888, 0.877 |
| The app's own pictures at 0.675 (row 12) | 0.931 | 0.936 |

Two bullets above read differently now: the one on the other half's
threshold and the one on a wrong face. The first half's 0.84 was a label
slip, not the threshold: a passer-by in front of the school was labelled
Jesse, so Jesse counted in a half he is not in, with no scene found. Without
that row the half reads 0.93 (the second half is then read at 0.600, not
0.575). And the wrong face behind 0.994 was the labels', not the cast's:
they had Jesse as `?` where Walter lifts him in the sand. The boundaries,
the scenes found and the shared threshold (0.675) are the same, and no gate
changes. By the true cast the same lines move less, and both ways (the bold
row 0.929 to 0.928).

The conversation section at the end of this file was written the same day,
before the correction. Its numbers with the stored cast did not move with
that correction; its
reading with the labels for both, and that reading's 30 rows in
`bb_s01e01.conversation.tsv`, are there as they are with the corrected
labels.

A second correction the same day (row 31: Walter's face in the shot at the
door, and seventeen faces behind gas masks marked as not to be told) moves
no number of the scenes tables above; the conversation section says what it
changes there.

The `?` rows of the labels were then read again (row 34: 37 rows got a name
of the main cast). No number of this file's scenes tables moves, and the
four-episode table is the same. In the bench's output "every shot its own
scene" by the true cast reads 0.447 for 0.444. Another number of that
output, which this file does not quote either, proved fragile on the way:
step A's second half, read with
the first half's threshold, is 0.912 by the stored cast, and with one more
row named, a face of three frames (Skyler at 16335 or at 16389, both kept
as `?`), it would be 0.831, because the first half's best threshold is a
near tie between 0.350 and 0.100. The conversation sections below, their
file of counts and row 29's result cell were made again with these labels
in `d3ed173c`.

## Results across four episodes (2026-11-12, row 8)

DINOv2-small's class token (weight 1) with the share of characters two shots
have in common (0.25), short pieces joining first (40 s). A character's
scenes, mean F over each episode's main cast:

```sh
uv run scene_titles.py <scenes_bench.exe> <titles.tsv> SHORT=40,1 W=0,0.25
```

The last three columns are `score`'s at the shared threshold (`AT=0.675`).

| Episode | Own best threshold | One threshold for all (0.675) | The other episodes' threshold | Scenes found / labelled | Boundary F, one shot off | Overlap |
|---|---|---|---|---|---|---|
| Breaking Bad S01E01 | 0.930 | 0.917 | 0.917 | 32 / 33 | 0.698 | 0.754 |
| Prison Break S01E01 | 0.948 | 0.926 | 0.926 | 31 / 40 | 0.812 | 0.834 |
| The Blacklist S01E01 | 0.936 | 0.936 | 0.889 | 24 / 28 | 0.640 | 0.760 |
| Bass X Machina S01E01 (3D) | 0.817 | 0.769 | 0.769 | 25 / 22 | 0.311 | 0.544 |
| **Mean** | 0.908 | 0.887 | 0.875 | | | |

- **Live action holds the gate (0.85) on three titles**, also with a
  threshold none of them chose: 0.917, 0.926, 0.889. The shared threshold is
  0.675 for 20, 30 and 40 s alike.
- **The 3D title does not** (0.77): its boundaries are barely found (average
  precision 0.17 against 0.35 to 0.48). Its scenes are short (a dream, a
  flashback), its sets look alike, and its stored cast is in pieces. One
  title; animation stays open.
- Across titles a little cast helps the measure (mean with the other
  episodes' threshold 0.875 against 0.842 without), though it lowers the
  boundaries' average precision on Breaking Bad. The colours beside the
  embedding do not (0.79 to 0.84 in four mixes).
- What the project stores alone (step A, 40 s): 0.892, 0.845, 0.790 and 0.817
  with one threshold, mean 0.836; with the other episodes' threshold 0.811.
  It is the fallback for a project without the embedding pass, and it is
  ahead on the 3D title.
- No gain from DINOv2's patch tokens in place of its class token (0.866 with
  one threshold), nor from leaving out the patches on a stored face, alone or
  with the body under it (0.884, 0.871): a close-up of one person looking the
  same in every scene is not what limits the links.

## Animation: three anime episodes more (2026-11-12, row 9)

`jjk_s2e03` (Jujutsu Kaisen S2E03, 456 shots, 28 scenes), `op_6` (One Piece,
Wano A, 501 shots, 14 scenes) and `rz_31` (Re:Zero 31, 382 shots, 14 scenes):
tuning episodes of `bench/anime`, labelled as above (the two labellers agree
at F 0.947, 0.867 and 0.929; the opening and ending songs, an eyecatch and
the preview are scenes of their own). They are in no project: `scenes_bench`
reads their bench inputs (shots, signals), who is where comes from
`bench/anime`'s track labels (`true cast`), and there is no stored cast, so
the cast is no cue there.

With the three live-action episodes and the 3D one, one threshold for all
seven (0.675), the live-action recipe:

| Episode | A character's scenes (F) | Boundaries' average precision |
|---|---|---|
| Breaking Bad, Prison Break, The Blacklist | 0.917, 0.926, 0.936 | 0.48, 0.35, 0.46 |
| Bass X Machina (3D) | 0.769 | 0.17 |
| Jujutsu Kaisen, One Piece, Re:Zero | 0.816, 0.712, 0.918 | 0.26, 0.09, 0.20 |

- **Animation is below live action**, and it wants other settings. What the
  project stores alone (the colours) finds anime boundaries better than
  DINOv2 does (average precision 0.48 against 0.26 on Jujutsu Kaisen, 0.46
  against 0.20 on Re:Zero): a place in a drawn film has its own flat colours,
  and the picture network looks at the characters.
- **For animation the colours go beside the embedding** (each weight 1, cast
  0.25, short pieces up to 60 s). On the four animation episodes alone, with
  the other three's threshold: Bass X Machina 0.861, Jujutsu Kaisen 0.882,
  One Piece 0.695, Re:Zero 0.952, mean 0.847. That setting was picked among
  seven tried on these four episodes, and the 3D episode swings by 0.25 with
  the threshold in some of them: provisional.
- For live action the colours beside the embedding still do not help (mean
  over the three with the others' threshold 0.880 against 0.906, and half as
  many scenes again are cut), so the two footage types keep their own
  settings. The app knows a project's footage type.
- One Piece stays at 0.70 to 0.73 whatever the setting: the episode is one
  long fight on a roof, cut by a recap, two eyecatch cards and three short
  looks elsewhere.

## In the app (2026-11-12, row 12)

The pass is in the app, off by default (Settings, General, "Read scenes").

- **What is stored.** For every shot three frames (at 1/6, 1/2 and 5/6 of
  it), each resized to 448 × 252 and given to DINOv2-small
  (`picture-dinov2-small` in `models/manifest.toml`, 88.5 MB, Apache-2.0,
  fetched when the pass first runs); the class token of each, as a unit
  vector in half floats, in `shot_pictures` (migration 17; about 2.3 KB a
  shot). The scenes themselves are not stored: they are worked out from the
  pictures when asked for and kept in memory until the pictures, the footage
  type or (live action) the cast change.
- **When it runs.** At the end of an episode's analysis, after the cast,
  also where the cast failed, not where the analysis was stopped; for
  episodes analysed before, when the setting goes on. It goes on from the
  shots it has read. Shots found again forget their pictures.
- **The recipes** (`as_analysis::scenes::Recipe`). Live action and mixed
  footage: pictures (weight 1) and shared cast (0.25), short pieces up to
  40 s join first, threshold 0.675 (row 8). Anime and animation: pictures
  and stored colours (each 1), no cast, 60 s, threshold 0.600 (rows 9 and
  12, provisional).
- **The same numbers with the app's own pictures.** `scenes_run` (an example
  of `as-analysis`) reads Breaking Bad S01E01 as the app does: 2,379
  pictures of 794 shots in 49 s. Its vectors against `shot_embed.py`'s:
  cosine 0.9986 on average (the two resize differently). A character's
  scenes with them: 0.931 at 0.675 (0.917 with the Python pictures).
- **In the real app** (a build of the app on a copy of the owner's library,
  the episode found again from its 960 × 540 proxy, so analysed anew: 771
  shots): the model arrived in 9 s, the pictures of 771 shots were read in
  45.9 s, which is 47 s per hour of video on the development PC (RTX 3060
  Ti, DirectML, NVDEC), after 43 s for the shots and 331 s for the faces
  and people. A project made by the app before (schema 16) opened and moved
  to 17. A pass cancelled after 66 shots was recorded stopped and went on
  with the other 705; cancelled while it waited behind another analysis it
  stayed stopped. The animation recipe ran on Bass X Machina S01E01 (679
  shots, pictures in 34.0 s, the colours read from what the shots pass
  stored): whole scenes take its most seen character from 74 to 405 shots.

### The scenepack's "Whole scenes"

A switch beside the pack's order, shown where the setting is on or the pack
was saved with it. With it on, every other shot of the scenes the character
is seen in joins the pack, marked "Off screen":

- such a shot has no size of the character's, and the filters that speak of
  the character in a shot (size, alone, plugin filters) pass it by; the
  filters that speak of the shot itself (episode, flash cut, length) judge
  it;
- it ranks behind every shot of the character's own, so a limit cuts those
  shots first;
- it stays out ("Scene not in pack") when none of the character's own shots
  of its scene is in, and that is settled before the limit;
- an episode whose scenes are not read keeps its shots as they are, and the
  page says how many such episodes the pack takes shots from.

In the real app, on that episode: the lead's pack goes from 307 to 669 of
the 771 shots (he is in nearly every scene), the second lead's from 125 to
412 (9:24 to 26:02). A pack of 121 clips, 80 of them off screen, exported as
one file of 405.5 s with 121 chapters, as the pack said. A preview takes
13 ms without the switch, 73 ms the first time with it and 16 ms after.

The shots of a scene stay shots, by the owner's choice (2026-11-12, "Keep
per shot"): a dissolve or a dark piece between two of them is left out, as
between a character's own shots, and with handles two neighbours overlap.

Not done: a pack action of a plugin is not told which clips are off screen;
a shot in which the character is seen only as a small figure counts as off
screen.

### The cast in the animation recipe

Row 9 gave the cast a weight of 0.25 for animation too. The three anime
episodes of the bench have no stored cast, so the cast had no say there, and
on Bass X Machina, the one animation title with a cast, it gives nothing:

| Animation recipe, one threshold for the four episodes (0.600) | Bass X Machina | Jujutsu Kaisen | One Piece | Re:Zero | mean |
|---|---|---|---|---|---|
| with the cast (0.25) | 0.861 | 0.900 | 0.695 | 0.952 | 0.852 |
| without the cast | 0.866 | 0.900 | 0.695 | 0.952 | 0.853 |

The app therefore leaves the cast out for anime and animation: that is what
was measured on anime, and the anime cast is the one known to split
characters. A cast edit then leaves the scenes of such a project as they
are.

Not measured yet: more titles of each kind (the animation settings rest on
four episodes), a larger picture network, who speaks across the cut (step C),
scenes across the episodes of one project.

## Conversations from who is in which shot (2026-11-15, rows 25, 27, 28)

Asked by the speech research (`research_notes/Speech and dialogue`, rung 0):
can a pack hold a character's conversations from what a project already
stores, with no sound? Version 0.1 did it with a walk over the shots. This
is a bench only: nothing in the pack code or the app changed.

### The sets

For each main character of Breaking Bad S01E01 (the 12 of `bench/identity`
with at least 30 s on screen, each taken as the stored character most of
their faces are on):

| Set | What it holds |
|---|---|
| own (a) | the shots the cast shows the character in, by a face it is sure of |
| groups (b) | own, and the shots between two own shots of one of the app's shot groups (at most 6 s apart, no fade or black between) |
| walk (c) | 0.1's rule: from every own shot out both ways, shot by shot, while the next shot is in the same scene and holds the character or a partner |
| bridged walk (e) | the walk, which also steps over a stretch of up to 6 s that holds neither, when a shot that does follows in the same scene with no fade between; counted with the stepped-over shots and without them |
| scenes, partners (g) | every shot of the character's scenes that holds the character or a partner |
| scenes, anyone (f) | every shot of the character's scenes in which the cast shows anyone |
| whole scenes (d) | every shot of the character's scenes (the app's "Whole scenes") |

A partner is anyone whose face is on screen at the same time as one of the
character's for at least 0.2 s, anywhere in the episode (0.1's rule). (a) to
(d) were fixed before the first run (row 25). (e) was named after that run,
(f) and (g) after the run of (e), each before its own run (rows 27 and 28):
what they gain on this episode flatters them.

Each set is inside the next from the bridged walk without its stepped-over
shots on: that walk, (g), (f), (d).

### How it is scored

Who is truly in a shot comes from `bench/identity`, who speaks in a scene
from the `speakers` column above, and `bb_s01e01.speakers.tsv` says which
face label each speaker is (matched by hand from the contact sheets; two
matches are unsure, 7 s of speech). Over the shots a set adds to the own
ones:

1. the share that show a speaker of their labelled scene (a labelled face of
   someone who speaks there). The draft bar of the proposal is on this one:
   at least 0.80 for the walk. "In a scene of theirs" is the same share when
   an added shot in a scene the character has no shot in counts as no
   speaker (added after the review, see below);
2. of the seconds the others speak in the character's scenes, the share
   whose speaker is seen in the added shots, and in the set at all;
3. the share with nobody of the conversation: faces that do not speak in
   that scene, and no labelled face;
4. of the shots of the character's scenes that show a speaker and not the
   character, the share the set holds.

Three readings: as the app has it (the stored cast, and the scenes of the
live-action recipe on the app's own pictures: 31 scenes for the 33
labelled), the stored cast with the labelled scenes, and the labels for
both. Each number is the mean over the 12 characters, with the 95 % interval
of that mean when the characters are drawn anew, and pooled over all shots.

```sh
cargo run --release -p as-project --example conversation_bench -- <project_copy_dir> bench/scenes/bb_s01e01.scenes.tsv bench/identity/bb_s01e01.labels.tsv bench/scenes/bb_s01e01.speakers.tsv <pictures.emb> bench/scenes/bb_s01e01.conversation.tsv
```

`<pictures.emb>` is the app's pictures of the shots (`scenes_run`). The
project copy is opened read only. `bb_s01e01.conversation.tsv` holds every
count per reading, character and set (there `br-kept` is the bridged walk
without its stepped-over shots, `sc-part` is (g), `sc-any` is (f)). `SURE=1`
leaves out the unsure name matches, `DETAIL=<label>` lists a character's
shots. Unit tests: `cargo test -p as-project --example conversation_bench`.

### Results, as the app has it

794 shots; a character's own shots are 59.8 on average (332 s). Mean over
the 12 characters (95 % interval), then pooled:

| Set | Shots | Seconds | Added shots | (1) show a speaker | (1) in a scene of theirs | (4) speaker shots held |
|---|---|---|---|---|---|---|
| groups (b) | 94.4 | 415 | 34.7 | 0.819 (0.708 to 0.920), 0.752 | 0.819, 0.752 | 0.289 (0.197 to 0.387), 0.330 |
| **walk (c)** | 119.9 | 540 | 60.2 | **0.986** (0.966 to 1.000), 0.968 | 0.973 (0.950 to 0.994), 0.960 | **0.778** (0.657 to 0.894), 0.734 |
| bridged walk (e), with the stepped-over shots | 152.8 | 641 | 93.1 | 0.830 (0.765 to 0.891), 0.797 | 0.788, 0.776 | 0.954 (0.896 to 0.997), 0.920 |
| bridged walk (e), without them | 135.0 | 603 | 75.2 | 0.983 (0.962 to 0.999), 0.968 | 0.952 (0.908 to 0.988), 0.950 | 0.947 (0.886 to 0.994), 0.912 |
| scenes, partners (g) | 138.9 | 621 | 79.2 | 0.969 (0.943 to 0.990), 0.943 | 0.913 (0.825 to 0.975), 0.916 | 0.956 (0.901 to 0.995), 0.925 |
| scenes, anyone (f) | 143.4 | 636 | 83.7 | 0.928 (0.881 to 0.969), 0.911 | 0.858 (0.752 to 0.939), 0.872 | 0.960 (0.906 to 0.999), 0.929 |
| whole scenes (d) | 190.0 | 741 | 130.2 | 0.692 (0.593 to 0.791), 0.593 | 0.629 (0.526 to 0.731), 0.567 | 0.967 (0.912 to 1.000), 0.938 |

The same as shots, over all 12 characters together (921 shots of their
scenes show a speaker and not the character):

| Set | Added shots | with a speaker | faces that do not speak | no labelled face | in a scene the character is not in | of the 921 held |
|---|---|---|---|---|---|---|
| groups (b) | 416 | 313 | 34 | 69 | 0 | 304 |
| walk (c) | 722 | 699 | 23 | 0 | 6 | 676 |
| bridged walk (e), with the stepped-over shots | 1,117 | 890 | 88 | 139 | 24 | 847 |
| bridged walk (e), without them | 903 | 874 | 29 | 0 | 17 | 840 |
| scenes, partners (g) | 950 | 896 | 54 | 0 | 34 | 852 |
| scenes, anyone (f) | 1,004 | 915 | 89 | 0 | 49 | 856 |
| whole scenes (d) | 1,563 | 927 | 227 | 409 | 77 | 864 |

Measure (2), the others' speech whose speaker is seen in the added shots and
in the set at all: own –, 0.783; groups 0.779, 0.906; walk 0.865, 0.907;
bridged walk 0.901, 0.925; (g), (f) and whole scenes 0.933, 0.958.

Per character, the walk and what would replace it (added shots, (1), speaker
shots held of those asked for):

| Character | Own shots | Walk (c) | Bridged walk, without the stepped-over shots | Scenes, anyone (f) | Whole scenes (d) |
|---|---|---|---|---|---|
| walter | 300 | 198, 0.90, 164/172 | 205, 0.89, 169/172 | 217, 0.85, 171/172 | 416, 0.45, 172/172 |
| jesse | 122 | 112, 1.00, 112/192 | 167, 0.99, 164/192 | 180, 0.93, 167/192 | 301, 0.56, 169/192 |
| skyler | 66 | 82, 1.00, 77/81 | 93, 1.00, 80/81 | 109, 0.96, 81/81 | 123, 0.85, 81/81 |
| hank | 39 | 21, 1.00, 21/42 | 42, 1.00, 42/42 | 56, 0.75, 42/42 | 97, 0.43, 42/42 |
| waltjr | 42 | 68, 1.00, 65/69 | 71, 1.00, 68/69 | 76, 0.95, 69/69 | 88, 0.82, 69/69 |
| krazy8 | 46 | 44, 0.93, 41/60 | 57, 0.93, 53/60 | 62, 0.92, 57/60 | 110, 0.55, 60/60 |
| gomez | 19 | 36, 1.00, 36/62 | 62, 1.00, 62/62 | 76, 0.82, 62/62 | 117, 0.53, 62/62 |
| marie | 22 | 45, 1.00, 45/45 | 45, 1.00, 45/45 | 45, 1.00, 45/45 | 50, 0.90, 45/45 |
| emilio | 26 | 50, 1.00, 50/125 | 85, 0.99, 84/125 | 93, 0.96, 89/125 | 158, 0.58, 91/125 |
| denim | 14 | 23, 1.00, 23/31 | 31, 1.00, 31/31 | 31, 1.00, 31/31 | 36, 0.86, 31/31 |
| mustache | 13 | 32, 1.00, 32/32 | 32, 1.00, 32/32 | 32, 1.00, 32/32 | 37, 0.86, 32/32 |
| bogdan | 8 | 11, 1.00, 10/10 | 13, 1.00, 10/10 | 27, 1.00, 10/10 | 30, 0.90, 10/10 |

The two other readings give the same picture. With the stored cast and the
labelled scenes: walk 0.986 and 0.779 held, bridged walk without the
stepped-over shots 0.974 and 0.959, (g) 0.972 and 0.982, (f) 0.932 and
0.993, whole scenes 0.702 and 1. With the labels for both, as corrected on
2026-11-15 (see "What they cannot say"): 0.986 and 0.782, 0.974 and 0.972,
0.974 and 0.991, 0.938 and 1, 0.700 and 1. So neither the
stored cast nor the scenes the app finds cost much here: 57 of the 921
speaker shots lie in a scene the recipe cut apart from the own shots (34 of
Emilio's, 23 of Jesse's).

### What the numbers say

- **The walk meets the draft bar (0.986 against 0.80), and the bar says
  little.** Of the 579 shots in which the cast is sure of someone, 528 show
  a speaker of their scene (0.912). The walk can only add shots with such a
  face, so any rule of that kind passes. What counts is the gap to "any
  face" (f): 0.06.
- **0.1's walk leaves out about a quarter of the shots that show a speaker
  of the character's scenes** (held 0.778, pooled 0.734). Of the 245 it
  leaves out, 176 lie behind a shot that stops it: one in which the cast
  shows nobody, or nobody who shares a frame with the character. 57 lie in a
  scene found apart, 8 show nobody the cast knows, 4 only strangers to the
  character. A partner who never shares a frame with the character, which
  the proposal named as the walk's limit, is rare here.
- **The 6-second bridge closes that gap** (row 27): 0.947 held, at the same
  share of added shots with a speaker (0.983). Character by character
  against the walk: +0.169 held (+0.078 to +0.267), −0.003 on (1). With its
  stepped-over shots in the set, (1) is 0.830 and under 0.80 for 5 of the 12
  characters: those shots hold nobody the measure can credit (16 of 214
  show a speaker).
- **A walk is not needed for that** (row 28). Whole scenes with only the
  shots in which the cast shows anyone (f) hold as many speaker shots
  (0.960), and need no new query: a pack's shot already carries how many
  characters the cast shows in it. The partner rule (g) and the bridged walk
  add fewer shots without a speaker: (f) minus the bridged walk is −0.055 on
  (1) (−0.102 to −0.017). That gap sits in the party scene (Hank 0.75
  against 1.00, Gomez 0.82 against 1.00) and where the recipe joins two
  scenes: (f) then takes every face of the wrong scene (49 added shots in a
  scene the character is not in, against 17).
- **Against whole scenes**: 0.31 of the shots whole scenes add show no
  speaker (0.195 no labelled face, 0.113 faces that do not speak). The
  filtered sets drop those and keep 0.95 to 0.96 of the speaker shots, with
  under two thirds of the added shots.
- **Against shot groups**: groups hold 0.289 of the speaker shots. They
  bridge between two own shots only, so a scene in which the character is
  seen once gets nothing.
- 0.1's cap of 180 s removes no shot here (the longest stretch is 174 s).
  Without the two unsure name matches measure (1) is the same to three
  decimals.

True by the way a set is built, so no result: the walk, the bridged walk
without its stepped-over shots, (g) and (f) add no shot without a labelled
face; (4) cannot fall from the bridged walk to (g) to (f) to whole scenes;
whole scenes hold every asked-for shot when the scenes are the labelled
ones; measure (2) is the same for (g), (f) and whole scenes, because one
sight of a speaker earns all their seconds.

### What they cannot say

- **One episode, and 12 characters that are not 12 draws.** Hank and Gomez
  have the same sets from the bridged walk on, Denim and Mustache too; these
  four have most of their shots in the party scene, and Denim and Mustache
  are guests who never speak. The intervals are too narrow for another
  title. Without Denim and Mustache the means of (1) and (4) are 0.983 and
  0.759 for the walk, 0.980 and 0.937 for the bridged walk, 0.914 and 0.953
  for (f); for Walter, Jesse and Skyler alone 0.966 and 0.829, 0.960 and
  0.941, 0.916 and 0.955.
- **Speakers are seconds per scene, not turns.** A speaker's face in a shot
  does not say that they speak in it, or with the character. A listener who
  never speaks counts as nobody. A scene the character only passes through
  counts as theirs: Jesse is seen for 10 s in the ride-along scene, and its
  shots of Hank, Walter and Gomez are asked for.
- **Only tracked faces count.** A shot of the conversation without a face
  (a back, a wide shot, hands, an insert) counts against a set whatever it
  shows. So (1) cannot rank whole scenes, groups or the bridged walk with
  its stepped-over shots against the sets that add only shots with a face,
  and nothing here says whether an editor wants those shots.
- (e), (f) and (g) were thought of after seeing this episode. The numbers
  cannot choose between them: the gaps are 0.015 to 0.055 and rest on a few
  characters.
- The speaker `unknown` (13 s) stays in what the others speak and can never
  be seen. The column of (1) over "scenes with speech" leaves out the one
  scene in which only `unknown` speaks.
- The copy was analysed while characters were also looked for by their
  bodies: 8 sure faces were placed that way. Without them no count changes
  (the review's check). The two flash cuts a default pack leaves out are
  counted.
- The review found three slips in `bench/identity`'s labels: `wavy_guest`
  is Skyler (four tracks at the party), the `jesse` at frame 10437 is a
  passer-by, and Jesse in shot 729 was `?`. They were corrected on
  2026-11-15 (row 30), after this section was written. No number of the two
  readings with the stored cast changed. The reading with the labels for
  both is given above as it is with the corrected labels (before: 0.986 and
  0.788, 0.976 and 0.973, 0.977 and 0.991, 0.940 and 1, 0.704 and 1), and
  those 30 of the 288 rows of `bb_s01e01.conversation.tsv` were made again.
  Jesse has 191 asked-for shots with the stored cast and 192 by the labels
  (169 before: the labels did not have him in the scene of shot 729, and
  had him in front of the school). A second correction the same day (row
  31) gave Walter's face in the shot at the door its name, so that shot is
  no longer asked of him: 925 speaker shots for 926 over all characters,
  every set but the own shots holds one less, and the tables above are as
  they come out now
  (pooled, the bridged walk without its stepped-over shots holds 0.912 for
  0.913; no mean of the tables moved). It also marks seventeen faces behind
  gas masks as not to be told, which changes no count here. A third change
  the same day (row 34) gave 37 unnamed faces a name of the main cast, and
  the tables and sentences of this section are now what the tool writes
  with those labels: 921 asked-for shots for 925, Walter 172 for 175,
  Walt Jr. 69 for 71, Jesse 192 for 191; two more added shots with a
  speaker for every set but the groups; no mean moved by more than 0.004
  (the walk's (1) 0.986 for 0.985, the three leading characters' 0.966 for
  0.962). The cells of rows 25, 27 and 28 hold the numbers of the run as it
  was, before the three corrections.

### The review

A reviewer without context read the rows, the tool and the code it follows,
and recomputed every count with a script of their own, written from the
rows: no difference in 288 rows, nor in any mean. The name matches hold on
the contact sheets they looked at. Taken from the review into this section:
the base rate beside the bar, what is true by construction, and no ranking
by (1) across sets that do and do not add shots without a face. Changed in
the tool after it: (4) no longer asks for shots that show the character
where the cast missed them (16 of 942; the row said so, the code did not);
every measure has its interval and its pooled share; and the count "in a
scene of theirs" was added, which row 25 did not register.

### For the owner

- 0.1's walk as it was is not what to port: it stops at the first shot
  without a known partner and loses a quarter of the speaker shots.
- The idea holds. The cheapest form is a filter on "Whole scenes": only the
  shots in which the cast shows someone. It needs no walk and no new query.
- Stricter forms, at the price of one query for who shares frames with whom:
  only shots with the character or a partner, or the bridged walk. They keep
  strangers of a crowd scene out, and the walk also stays closer to the
  character where two scenes were joined. This episode cannot say whether
  that is worth it.
- Not answered by any number here: whether a conversation pack should keep
  the shots without a face that lie inside a conversation.
- Before a port: a second title, and a look by eye at one such pack.

## Conversations on a second title: Prison Break S01E01 (2026-11-15, row 29)

The owner's answer to the question above: a second title first. Prison Break
S01E01 (584 shots, 40 labelled scenes, 43 min), read from a copy of the
owner's project: five episodes, the cast grouped over all five, 138 stored
characters, 53 of them in this episode. The same tool, the same eight sets,
the same measures. Row 29 fixed all of it, and what counts as the same
finding, before any label was made.

### Labels made by readers

The episode has no identity labels and no speaker labels. Pictures and sound
come from the project's 960 × 540 proxy, decoded read only.

Row 29 says the file with its subtitles was no longer on the PC. **That was
wrong**: it had been moved into the library's folder, and the search by name
that found it ended after the run. The labels are still what the row
registered. What was missed: the file is 1920 × 1080 and carries English
subtitles as pictures (PGS). They say when a line is shown and what it says,
not who says it; drawn onto the readers' sheets they could make the speaker
marks surer than stills alone. Not tried.

- **Sound**: `sound_events.py` (YAMNet) scores speech every 0.48 s; a step
  with 0.5 or more counts as speech heard: 1,376 s of the episode's 2,588 s.
- **Sheets** (`tools/slot-bench/speak_sheets.py`): every shot as a row of six
  pictures spread over it (two rows from 6 s, three from 12 s), with the
  seconds of speech heard in it.
- **Cast pages** (`speak_cast.py`, `pb_s01e01.cast.tsv`), made by eye before
  any reading: 39 labels for the stored characters with three face tracks or
  more in the episode, the project's names where set (13), a few words on
  the look for the others. Two stored characters taken for one woman share a
  label. Five of the 45 were left off: their pictures show more than one
  person, or no face a reader could tell (characters 39, 40, 89, 104, 134;
  row 29 had not foreseen that).
- **Two readers** (AI agents; each reading split over 13 agents of 12
  sheets; rules in `tools/slot-bench/speak_reader.md`) wrote per shot whose
  face is seen, by a cast label or `other`, and who of them speaks: speech
  is heard and the mouth changes between the pictures. Blind to each other,
  to where the cast puts anyone and to every result. Their own lines are in
  `check/pb_s01e01.speak.a.tsv` and `.b.tsv`; `pb_s01e01.shots.tsv` holds
  both per shot (`speak_merge.py` had nothing to mend).
- **What counts as true**: a shot shows the names both readers give. A
  person speaks in a scene when each reader marks them speaking in a shot of
  it. `other` is never credited, as `?` and `unknown` above.

The two readers give the same list of faces in 542 of the 584 shots (0.928)
and the same list of speakers in 550 (0.942). Of the 1,376 s of heard
speech, 1,044 s lie in shots in which both mark the same named person
speaking, 44 s in shots in which both mark someone unnamed, 236 s in shots
in which neither marks anyone (a voice from outside the picture, or a mouth
they could not read).

### The readers against Breaking Bad's labels

Checked first, as row 29 asks. Two more readers read the 374 shots of the 17
odd-numbered labelled scenes of Breaking Bad S01E01 in the same way, with
cast pages of the 43 names of `bench/identity` that had three tracks or
more (`check/bb_s01e01.cast.tsv`, `.shots.tsv`, `.speak.a.tsv`, `.b.tsv`).
The hand labels are those corrected the same day (rows 30, 31 and 34); the pages
were drawn before, with `wavy_guest` still on them, a name no reader used.
The corrections move only the last line of the table (0.955 and 0.760 before
them).

| Question | Readers against the hand labels and Serial Speakers |
|---|---|
| Does the shot show a speaker of its scene? | the same answer in 353 of 374 shots: **0.944** (the bar: 0.90), kappa 0.879 |
| How often is the answer yes? | labels 0.636, readers 0.634 |
| The same, in the 252 shots in which the cast is sure of someone | the same answer in 0.956; yes by the labels 0.937, by the readers 0.893 (11 shots only the labels, none only the readers) |
| Who speaks in the scenes? | the readers find 21 of the labels' 27 speakers, 0.943 of their 407 s of speech; all 21 they name speak by the labels |
| Who is seen in a shot? | of 334 names both readers give, 322 are the labels' (0.964); the labels give 421 (0.765 found) |

The bar is met. The readers err on the careful side: they miss small
speakers and small faces, and name no speaker who is none. What row 29
feared, a listener taken for the speaker, did not show: where the cast shows
a face, the readers say "speaker" less often than the labels, not more. So
measure (1) on Prison Break is more likely a little too low than too high.

```sh
cargo run --release -p as-project --example conversation_bench -- agree <bb_project_copy_dir> bench/scenes/bb_s01e01.scenes.tsv bench/identity/bb_s01e01.labels.tsv bench/scenes/bb_s01e01.speakers.tsv bench/scenes/check/bb_s01e01.shots.tsv
MEDIA=0 cargo run --release -p as-project --example conversation_bench -- shots <pb_project_copy_dir> bench/scenes/pb_s01e01.scenes.tsv bench/scenes/pb_s01e01.shots.tsv bench/scenes/pb_s01e01.cast.tsv <pictures.emb> bench/scenes/pb_s01e01.conversation.tsv
```

### Results, as the app has it

The app's recipe finds 31 scenes for the 40 labelled (15 of its 30
boundaries are labelled ones). 18 main characters: the cast labels whose
stored character has at least 30 s of faces in the episode; their own shots
are 30.3 on average (172 s). A speaker of their scene is shown by 439 of the
487 shots in which the cast is sure of someone (0.901). Mean over the 18
characters (95 % interval), then pooled:

| Set | Shots | Seconds | Added shots | (1) show a speaker | (1) in a scene of theirs | (4) speaker shots held |
|---|---|---|---|---|---|---|
| groups (b) | 46.0 | 217 | 15.7 | 0.918 (0.851 to 0.972), 0.866 | 0.918, 0.866 | 0.491 (0.351 to 0.633), 0.410 |
| **walk (c)** | 55.1 | 274 | 24.8 | 0.950 (0.924 to 0.974), 0.919 | 0.948 (0.919 to 0.973), 0.915 | **0.769** (0.637 to 0.891), 0.687 |
| bridged walk (e), with the stepped-over shots | 65.7 | 306 | 35.4 | 0.880 (0.817 to 0.934), 0.812 | 0.870, 0.794 | 0.898 (0.823 to 0.962), 0.853 |
| bridged walk (e), without them | 58.3 | 288 | 28.1 | 0.938 (0.906 to 0.967), 0.903 | 0.928 (0.889 to 0.962), 0.885 | 0.824 (0.710 to 0.926), 0.751 |
| scenes, partners (g) | 63.4 | 311 | 33.1 | 0.936 (0.904 to 0.966), 0.893 | 0.880 (0.810 to 0.942), 0.810 | 0.861 (0.764 to 0.944), 0.813 |
| **scenes, anyone (f)** | 75.9 | 356 | 45.7 | **0.892** (0.854 to 0.930), 0.855 | 0.800 (0.710 to 0.882), 0.713 | **0.990** (0.983 to 0.996), 0.990 |
| whole scenes (d) | 88.2 | 386 | 57.9 | 0.743 (0.688 to 0.797), 0.686 | 0.671 (0.579 to 0.755), 0.570 | 1.000 |

The same as shots, over all 18 characters together (578 shots of their
scenes show a speaker and not the character):

| Set | Added shots | with a speaker | faces that do not speak | no face both readers see | in a scene the character is not in | of the 578 held |
|---|---|---|---|---|---|---|
| groups (b) | 283 | 245 | 21 | 17 | 0 | 237 |
| walk (c) | 446 | 410 | 36 | 0 | 6 | 397 |
| bridged walk (e), with the stepped-over shots | 637 | 517 | 86 | 34 | 17 | 493 |
| bridged walk (e), without them | 505 | 456 | 49 | 0 | 13 | 434 |
| scenes, partners (g) | 596 | 532 | 64 | 0 | 54 | 470 |
| scenes, anyone (f) | 822 | 703 | 117 | 2 | 145 | 572 |
| whole scenes (d) | 1,043 | 716 | 154 | 173 | 197 | 578 |

Per character (added shots, (1), speaker shots held of those asked for):

| Character | Own shots | Walk (c) | Scenes, partners (g) | Scenes, anyone (f) | Whole scenes (d) |
|---|---|---|---|---|---|
| michael | 191 | 119, 0.87, 97/117 | 128, 0.84, 99/117 | 176, 0.85, 117/117 | 236, 0.64, 117/117 |
| sucre | 46 | 27, 0.93, 24/25 | 39, 0.82, 25/25 | 53, 0.72, 25/25 | 73, 0.52, 25/25 |
| veronica | 45 | 29, 0.97, 27/48 | 45, 0.93, 34/48 | 66, 0.92, 48/48 | 73, 0.84, 48/48 |
| lincoln | 46 | 33, 0.85, 27/74 | 74, 0.88, 49/74 | 117, 0.86, 74/74 | 142, 0.72, 74/74 |
| pope | 17 | 11, 1.00, 11/11 | 11, 1.00, 11/11 | 11, 1.00, 11/11 | 14, 0.79, 11/11 |
| westmoreland | 14 | 48, 0.92, 44/59 | 68, 0.90, 58/59 | 72, 0.85, 58/59 | 90, 0.70, 59/59 |
| lj | 34 | 10, 1.00, 8/32 | 12, 1.00, 10/32 | 39, 0.85, 31/32 | 45, 0.76, 32/32 |
| abruzzi | 27 | 42, 0.93, 39/45 | 61, 0.85, 44/45 | 73, 0.79, 44/45 | 100, 0.60, 45/45 |
| sara | 25 | 14, 0.93, 13/13 | 25, 0.84, 13/13 | 41, 0.80, 13/13 | 60, 0.55, 13/13 |
| bishop | 15 | 11, 1.00, 11/11 | 11, 1.00, 11/11 | 11, 1.00, 11/11 | 17, 0.65, 11/11 |
| kellerman | 18 | 17, 1.00, 17/17 | 17, 1.00, 17/17 | 17, 1.00, 17/17 | 19, 0.89, 17/17 |
| home_man | 13 | 10, 1.00, 10/10 | 10, 1.00, 10/10 | 10, 1.00, 10/10 | 11, 0.91, 10/10 |
| blonde_woman | 14 | 8, 1.00, 8/27 | 16, 1.00, 16/27 | 30, 0.87, 26/27 | 32, 0.84, 27/27 |
| maricruz | 9 | 7, 1.00, 7/7 | 7, 1.00, 7/7 | 8, 0.88, 7/7 | 8, 0.88, 7/7 |
| cap_inmate | 5 | 18, 0.83, 15/27 | 22, 0.86, 19/27 | 35, 0.83, 26/27 | 46, 0.67, 27/27 |
| lean_inmate | 7 | 12, 0.92, 11/27 | 20, 0.95, 19/27 | 33, 0.88, 26/27 | 44, 0.70, 27/27 |
| hoodie_inmate | 9 | 5, 1.00, 4/4 | 5, 1.00, 4/4 | 5, 1.00, 4/4 | 6, 0.83, 4/4 |
| hale | 10 | 25, 0.96, 24/24 | 25, 0.96, 24/24 | 25, 0.96, 24/24 | 27, 0.89, 24/24 |

With the stored cast and the labelled scenes, (1) and the speaker shots
held: walk 0.953 and 0.768, bridged walk without the stepped-over shots
0.937 and 0.824, (g) 0.924 and 0.861, (f) 0.893 and 0.990, whole scenes
0.743 and 1. Measure (2) is coarse here (speech counts for a person only in
shots in which both readers see them speak) and is not set beside Breaking
Bad's: the others' speech whose speaker is seen in the added shots is 0.627
for the walk and 0.676 for (f) and whole scenes.

### The three conditions of row 29

| Condition | Result | Holds |
|---|---|---|
| (i) the walk holds fewer speaker shots than (f): (f) minus the walk on (4), character by character, interval above 0 | +0.221 (+0.104 to +0.349) | yes |
| (ii) (f) holds 0.90 or more of the speaker shots | 0.990 (0.983 to 0.996) | yes |
| (iii) (f) meets 0.80 on (1), and whole scenes lie below (f) | 0.892; whole scenes 0.743, (f) minus whole scenes +0.149 (+0.111 to +0.187) | yes |

All three hold as registered. The review (below) shows that each says less
than it seems:

- **(i)**: the walk is inside (f), so the difference can only be 0 or more;
  the condition asks whether the walk misses anything for four characters or
  more, not which way. Its size is fed by scenes the character never speaks
  in: 168 of the 578 asked-for shots lie in such scenes (Lincoln far away in
  two shots of the yard scene: 14 asked for). Over only the scenes in which
  the character speaks it is **+0.141** (+0.049 to +0.252; 16 characters).
- **(ii)**: the cast pages were made from the stored characters, so the
  readers can only name people the cast already has. Of the 442 speaker
  shots the cast shows nobody in 3. (f) keeps every shot in which the cast
  shows anyone, so it is near 1 by that alone. With `other` credited as one
  person it is 0.974.
- **(iii)**: it holds on plain (1) only. Counted strictly ("in a scene of
  theirs") (f) is **0.800** (0.710 to 0.882), pooled **0.713**, and 10 of
  the 18 characters reach 0.80. Plain (1) is close to a base rate: in 29 of
  the 32 scenes with named speech every shot with a named face shows a
  speaker, and any shot in which the cast is sure of someone scores 0.901.
  That whole scenes lie below (f) could hardly fail: the shots they hold
  beyond (f) are those in which the cast shows nobody.

### What the second title says

- **0.1's walk again leaves out about a quarter of the speaker shots**:
  held 0.769, against 0.778 on Breaking Bad.
- **For another reason.** On Breaking Bad the walk stopped at a shot without
  a known partner, and partners who never share a frame with the character
  were rare (4 of 245 missed shots). Here 102 of the 181 it leaves out show
  only such people, 73 lie behind such a shot, 6 show nobody the cast knows.
  Prison Break cuts conversations as single faces: the judge speaking to
  Michael and Veronica (0 of 6 and 1 of 12 shots held),
  the bank teller (0 of 6), Lincoln behind the mesh with LJ (1 of 12), a
  phone call. This is the limit the proposal named for the walk; one title
  showed it rarely, the other often.
- **So the partner rule is not safe.** (g) holds 0.861 and the bridged walk
  0.824 here, against 0.956 and 0.947 on Breaking Bad. Partners are counted
  inside the episode; counted over the project's five episodes the walk
  holds 0.790 and the gap to (f) is +0.200 (the review's count).
- **(f) holds nearly every speaker shot on both titles** (0.990 here, 0.960
  there), in part by the way the labels were made here. It does not ask who
  shares a frame with whom.
- **Its price is paid in the scenes.** The app's recipe joins two labelled
  scenes nine times here (apartment and bank, meat plant and infirmary,
  death-row cell and visit, and six more), and (f) takes every face of the
  joined scene: 145 of its 822 added shots lie in a scene the character has
  no shot in ((g) 54, the walk 6). Sara: 21 of her 41 added shots are the
  mobsters' scene, joined to the infirmary. Counted strictly, (f) is 0.800
  against the walk's 0.948: about +0.22 of the speaker shots for −0.15 of
  the added shots that show a speaker of the character's own scene. With
  the labelled scenes (f) is 0.893 and the walk 0.953.
- **Against whole scenes** (f) is the better set on every count: it drops
  221 added shots, 13 of them with a speaker, and scores 0.892 against 0.743
  (strict 0.800 against 0.671).
- **Against shot groups**: groups hold 0.491 of the speaker shots (0.289 on
  Breaking Bad).
- With either reader alone as the truth the picture is the same: the walk
  0.959 and 0.950 on (1), 0.771 and 0.768 held; (f) 0.899 and 0.892, 0.992
  and 0.990 (not registered: a check of the rule that both must agree).

### What it cannot say

- **One more episode of one more live-action series.** Nothing on anime or
  3D animation, where the cast is weaker and no speaker labels exist.
- **Not 18 draws.** Two of the 18 main characters are extras the readers
  never name (`cap_inmate`, `lean_inmate`: they stand behind Michael and
  Westmoreland in two long shots), with the same sets from the bridged walk
  on; Kellerman and Hale share six of eight sets; eight characters have one
  or two scenes. Without the two extras: (i) +0.189, (ii) 0.993, (iii)
  0.897, strict 0.805. When the scenes are drawn anew in place of the
  characters (the review's count): (i) +0.090 to +0.319, (ii) 0.984 to
  1.000, (f) on (1) 0.747 to 0.938, strict 0.587 to 0.869. So "(f) meets
  0.80" is a point, not a secured bound.
- **The labels are readers' work from 320 × 180 stills.** Small faces become
  `other`: 7 of the 39 labels are never used by either reader, the yard's
  inmates among them, so in the yard the truth knows Michael, Westmoreland,
  Abruzzi and nobody else. Someone unnamed is never credited (44 s of
  speech). One speaker was lost by the cast page: the plant boss on the
  phone is `hairnet_man` without his hairnet, and both readers wrote
  `other` (that counts against (f)). Two speaker marks by both readers are
  not borne out by the pictures (Westmoreland in shot 137, LJ in shot 54);
  without them (iii) is 0.885, strict 0.794.
- **What the check on Breaking Bad does not cover**: other readers made the
  Prison Break labels, from cast pages made from the stored cast and not
  from hand labels; they mark the one named person in the picture as
  speaking in 0.78 of the shots with speech, the Breaking Bad readers in
  0.70; the check did not use the app's scenes. What it does cover beyond
  the single shot (the review's count): the sets of Breaking Bad's read
  scenes scored by the readers in place of the hand labels give the walk
  0.840 held for 0.852 and (f) 0.983 for 0.995.
- **Joined scenes only add shots.** No speaker shot lies in a scene found
  apart here, so this episode cannot show what a wrongly split scene costs
  (f); Breaking Bad is the only evidence for that (57 of 921 shots).
- **A scene counts as a character's when the cast shows them in it once**,
  and the labelled scenes are coarse: two of them start at an act break
  graphic and hold a whole dialogue after it.
- "As the app has it" is after the owner's own corrections of this cast (23
  pinned tracks in the episode, 16 names).
- Only faces count, here too: nothing says whether a pack should keep the
  shots without a face that lie inside a conversation. Measure (2) cannot be
  read here.

### The review

A reviewer without context got the row, the tool, the label files and
copies of both projects, and no conclusion. Recomputed with scripts of
their own: every number of the readers' check (with the labels as they were
and as corrected), all 4,896 cells of `pb_s01e01.conversation.tsv`, every
mean, interval, pooled share and paired difference. No difference. They
looked at twenty-odd shots on the sheets: who is seen was right wherever
they could tell and errs toward `other`; who speaks was right or not to be
told from stills, and doubtful in the two shots named above.

Taken from the review into this section: what each condition can and cannot
show, the strict count beside the bar, the base rate, the scenes in which
the character does not speak, the scenes drawn anew, the lost speaker, the
limits of the check. Deviations from row 29 it names, all stated here: five
stored characters left off the cast pages; partners counted inside the
episode; the truth's scenes are always the labelled ones; the row does not
say whether the mean or the pooled share meets a bar (both do on the
registered measures). `speak_merge.py` would have mended two kinds of rows
and mended none. The order of the work: the row was committed at 14:59, the
sheets drawn at 15:01, the cast pages at 15:07, the first reading at 15:10.

### For the owner, after two titles

- **0.1's walk and the partner rule: not to port.** On the second title
  they miss real conversations, because people who talk to each other are
  often never in one frame.
- **The filter on "Whole scenes"** (only the shots in which the cast shows
  someone) is better than "Whole scenes" as it is on both titles, needs no
  walk and no new query, and holds nearly every shot that shows a speaker.
- **It is as good as the scenes are.** Where the scenes pass joins two
  scenes, the filter takes the other scene's faces too: on Prison Break 18
  in 100 of the shots it adds lie in a scene the character is not in.
  A walk is cleaner there and pays for it with a quarter of the speaker
  shots. Fewer joined scenes would help the filter more than any walk.
- Still open: a look by eye at one such pack; the shots without a face
  inside a conversation; animation.

## The second title read again with its subtitle lines (2026-11-15, row 33)

Row 29's correction said what was missed: the Prison Break file with its
subtitles is on the PC. The owner asked for the speaker labels again, with
the lines on the sheets. Row 33 fixed the reading and its rule before any
new sheet was read. No check against Breaking Bad is possible this time: its
file with subtitles is gone, so nothing here measures the new marks against
labels made by hand.

### What was read

- **Lines**: `tools/slot-bench/speak_lines.py` cuts each of the 602 English
  subtitle lines (pictures, PGS) out of a frame of the 1080p file, wherever
  in the frame it stands (24 lines of the opening credits stand at the top).
- **Sheets**: the first reading's pictures, unchanged (without `LINES` the
  tool writes the first sheets byte for byte), and under each shot the lines
  on screen during it; `<<` and `>>` mark a line that began before the shot
  or goes on after it. The cast pages are the first reading's.
- **Readers**: two new ones, blind to each other and to the first labels
  (each reading split over 19 agents of 12 sheets), with the rules of the
  first reading and a section on lines (`speak_reader.md`): the mouth
  decides; the words tell a speaker from a listener who is spoken to; a
  person whose mouth cannot be read is marked when the words can only be
  theirs.
- **Files**: `pb_s01e01.lines.shots.tsv` (both readers per shot; the merge
  had nothing to mend), the readers' own lines in
  `check/pb_s01e01.lines.speak.c.tsv` and `.d.tsv`, the table in
  `pb_s01e01.lines.conversation.tsv`.

```sh
MEDIA=0 cargo run --release -p as-project --example conversation_bench -- compare <pb_project_copy_dir> bench/scenes/pb_s01e01.scenes.tsv bench/scenes/pb_s01e01.shots.tsv bench/scenes/pb_s01e01.lines.shots.tsv bench/scenes/pb_s01e01.cast.tsv
MEDIA=0 cargo run --release -p as-project --example conversation_bench -- shots <pb_project_copy_dir> bench/scenes/pb_s01e01.scenes.tsv bench/scenes/pb_s01e01.lines.shots.tsv bench/scenes/pb_s01e01.cast.tsv <pictures.emb> bench/scenes/pb_s01e01.lines.conversation.tsv
```

### The two readings against each other

| | First reading (pictures) | Second reading (pictures and lines) |
|---|---|---|
| The two readers of a reading give the same faces / the same speakers | 0.928 / 0.942 of 584 shots | 0.920 / 0.949 |
| Seconds of heard speech in shots in which both mark the same named person | 1,044 of 1,376 | 1,091 |
| Both mark the one named person in the picture as speaking (1 s of speech or more) | 192 of 245 (0.784) | 195 of 256 (0.762) |
| A shot shows a speaker of its scene | 0.757 of the shots | 0.745 |

- "Shows a speaker of its scene": the same answer in 571 of 584 shots
  (0.978, kappa 0.941).
- Who speaks in which scene: 67 pairs of person and scene are in both
  readings, 3 only in the first, 2 only in the second.
- Who is seen: the same names in 551 shots (0.943).
- Who speaks, shot by shot: the same names in 519 shots (0.889). Over the
  313 shots in which either reading has a mark it is 248 (0.792): **65 shots
  differ**. The second reading adds a mark in 42, drops one in 21, names
  another person in 2.

### The table with the new labels

As the app has it, mean over the 18 characters (first reading in brackets):

| Set | (1) show a speaker | (1) in a scene of theirs | (4) speaker shots held |
|---|---|---|---|
| groups (b) | 0.942 (0.918) | 0.942 (0.918) | 0.497 (0.491) |
| walk (c) | 0.954 (0.950) | 0.952 (0.948) | 0.778 (0.769) |
| bridged walk (e), without the stepped-over shots | 0.934 (0.938) | 0.924 (0.928) | 0.831 (0.824) |
| scenes, partners (g) | 0.933 (0.936) | 0.877 (0.880) | 0.866 (0.861) |
| scenes, anyone (f) | 0.887 (0.892) | 0.795 (0.800) | 0.990 (0.990) |
| whole scenes (d) | 0.739 (0.743) | 0.667 (0.671) | 1.000 |

572 shots are asked for (578). Row 33's rule:

| What row 33 asks | Result | Holds |
|---|---|---|
| (i) (f) minus the walk on (4), interval above 0 | +0.212 (+0.098 to +0.336) | yes |
| (ii) (f) holds 0.90 or more on (4) | 0.990 (0.983 to 0.996) | yes |
| (iii) (f) meets 0.80 on (1), whole scenes below it | 0.887; whole scenes 0.148 below (0.110 to 0.186) | yes |
| The walk's and (f)'s mean on (4), (f)'s on (1) and "in a scene of theirs" move by 0.05 at most | +0.010, 0.000, −0.005, −0.005 | yes |

By the rule as registered, row 29 holds. Counted strictly, (f) is now just
under the bar it sat on (0.795).

### What the review found

A reviewer without context recomputed every line of the three printouts and
both tables with a script of their own (no difference), and looked at all 65
shots in which the readings differ, on the second readers' sheets.

- **The rule could hardly have failed.** The table uses a mark once per
  person and scene: 69 such pairs carry it, and the 65 shots move 5 of them.
  With every face a reader sees marked as speaking whenever speech is heard,
  the four numbers move by −0.005, 0.000, +0.032 and +0.018 and all three
  conditions stay true. (f) on (4) cannot move at all: it depends on faces
  and cast, not on who speaks. So "row 29 holds" says that the table does
  not feel the shot-by-shot marks. It does not confirm the labels.
- **The two readings agreeing is no evidence that either is right.** They
  share pictures, cast pages and blind spots. Two readers of one reading
  agree on who speaks in 0.94 to 0.95 of the shots; two readers of different
  readings in 0.87 to 0.88. In 18 shots drawn from those in which the
  readings agree, the reviewer found both kinds of shared error once each:
  a likely listener whom all four readers marked (shot 241), and a speaker
  all four missed (shot 305).
- **The look at the 65 shots: the sheets bear out the second reading in 58,
  the first in 4, and cannot tell in 3.** In 28 of the 58 only the words
  show it, the same words the second readers had; where a picture is part
  of the evidence it is 30 to 4.
- **What the first reading got wrong: listeners.** In 16 shots both first
  readers marked a person whose mouth is still while a line is said to or
  about them: the judge while counsel speaks, Michael while the judge passes
  sentence, Westmoreland while two others talk about him. This is what row
  29 feared, and the check on Breaking Bad did not show it. Three of these
  marks had made someone a speaker of a scene in which he says nothing; all
  five scene lists that differ go the second reading's way.
- **What the second reading adds**: 42 speakers, 40 of them borne out, 22 by
  the words alone (a mouth too small, turned away, behind a mesh).
- **Its own errors**: one mark pulled by a line onto the man in the picture
  (shot 402, a line that runs over a cut), one name from the story where no
  face can be matched (shot 352), three right marks dropped, one of them
  because the line under the shot was someone else's (shot 553).
- **The gauge shows little.** 0.784 against 0.762 is the net of 15 marks
  gone and 12 new ones on the same shots, and most new marks are in shots
  with several people, where it does not look. It shows only that the second
  readers did not mark the one person in the picture wholesale.
- **The seconds of heard speech are often wrong, and the rules let them
  overrule the lines**: 34 shots have a line under them and 0.0 s measured,
  so that nobody may be marked there (26 of them with a second or more of
  line); 19 shots have a second or more measured and no line. Row 33 did not
  say that the old rule "with 0.0 s nobody speaks" stays; the reader rules
  kept it. This matters little for the table and much for any use of the
  marks shot by shot.
- **Row 33's sample was not a spread.** "Every k-th of them" came out of the
  tool as the first 40 of the 65 (a whole-number step); three of the four
  shots that speak for the first reading lie in the other 25. The reviewer
  looked at all 65; the tool now spreads its picks evenly.

### What follows

- **For the port, nothing changes.** The table and its conditions are the
  same with either reading, because they rest on who speaks in a scene and
  not on who speaks in a shot.
- **For the labels**: the second reading is the better supported of the two
  on the shots where they differ, and not proven better: nothing here checks
  either against sound. Row 29's numbers stay the registered ones; both
  label files are in the repository.
- **What would test the marks**: a check against sound on a sample of shots,
  those in which the readings agree among them; or a title with hand labels
  and subtitles. The rule that 0.0 s of measured speech forbids a mark should
  go before any such reading is used shot by shot.

## A look by eye at the filter's packs (2026-11-16, row 35)

The owner's answer to "Port the filter on Whole scenes now?": eye check
first. Row 35 fixed the look before any sheet or video was made: six
characters by a rule on the tables (per title the two with the most own
shots, and the one whose (f) is lowest "in a scene of theirs": walter, jesse
and bogdan; michael, sucre and sara), what is made for the owner to watch,
what is counted, and one line that would speak against the plain filter.

### What was made

- **For the owner's eye** (`tools/slot-bench/pack_look.py pack`, outside the
  repository, in `E:\as-ownmodels\scenes\eye\packs\`): for each of the six
  characters sheets of their whole scenes in order (green frame: an own
  shot; yellow: added by the filter; dimmed with a red bar: left out by it),
  and two videos cut from the proxy, `<title>_<character>.filter.mp4` (what
  the filter gives) and `.scenes.mp4` (whole scenes), with the same colours
  as a bar at the top of the picture. Nothing of the pack code is used.
- **For the count**: two readers (AI agents), blind to each other and to
  every result, sorted every shot of both episodes in which the cast shows
  nobody (97 and 215 shots), each seen between the shot before and the shot
  after it, into `talk`, `place`, `other` and `nothing`
  (`pack_look_reader.md`; their files are `check/pb_s01e01.drops.a.tsv`,
  `.b.tsv` and the same for `bb`). They give the same kind in 91 of 97
  shots (0.938) and in 210 of 215 (0.977).

```sh
DETAIL=<character> cargo run --release -p as-project --example conversation_bench -- … > <detail.tsv>   # from its "<character>, as the app has it" line on
uv run pack_look.py drops <ffmpeg> <video> <project_copy_dir> <media> <sheets_dir>
uv run pack_look.py pack <ffmpeg> <video> <project_copy_dir> <media> <detail.tsv> <out_prefix>
uv run pack_look.py tally <project_copy_dir> <media> <reader_a.tsv> <reader_b.tsv> <detail.tsv> [...]
```

### The count

| Character | Whole scenes | Left out by the filter | of it `talk` by both readers | Holes (median; up to 6 s) |
|---|---|---|---|---|
| michael | 427 shots, 1,905 s | 60 shots, 144 s (7.6 %) | 76 s (0.525) | 23 (3.1 s; 19) |
| sucre | 119 shots, 564 s | 20 shots, 52 s (9.3 %) | 35 s (0.669) | 10 (3.4 s; 9) |
| sara | 85 shots, 304 s | 19 shots, 39 s (12.9 %) | 17 s (0.430) | 8 (2.7 s; 7) |
| walter | 716 shots, 3,146 s | 199 shots, 496 s (15.8 %) | 330 s (0.665) | 66 (3.0 s; 54) |
| jesse | 423 shots, 1,544 s | 121 shots, 273 s (17.7 %) | 225 s (0.825) | 47 (2.4 s; 41) |
| bogdan | 38 shots, 244 s | 3 shots, 18 s (7.4 %) | 9 s (0.478) | 1 (8.7 s; 0) |

A hole is a left-out stretch between two kept shots of one found scene. Of
the seconds the filter leaves out, `talk` by both readers is 0.541 for the
three of Prison Break, 0.716 for the three of Breaking Bad and 0.676 for
all six (with each shot counted once: 0.525, 0.673, 0.641). Row 35's line
was a third. **By the row, the look speaks against porting the plain
filter.**

### Why that number says less than the row claims

A reviewer without context recomputed every number of the tally (no
difference), judged 40 evenly spread shots per title and every shot on which
the readers differ, and read all sheets of three characters.

- **`talk` was drawn too wide, and that is the author's rule, not the
  readers'.** The readers were told: part of the conversation *or action*
  around it, also when the picture shows what someone does, holds or looks
  at. Under that rule every faceless shot that belongs to the story is
  `talk`; left for the other kinds are opening views, titles, credits and
  black. So the line could hardly not be crossed, and "the plain filter cuts
  into conversations" does not follow from it. The readers themselves were
  right by their rules in about nine of ten sampled shots and never far off.
- **Read strictly** (a person of the scene, or their hands, is in the
  picture) the share is about 0.32 to 0.38 of the left-out seconds, with a
  span of 0.19 to 0.49 from the sample: at a third, not above it.
- **Inside scenes of people talking** the reviewer's sample gives about a
  tenth to a quarter of the left-out seconds, on seven shots. Most `talk`
  seconds lie in action, in scenes of one person alone and in one montage.
- **Six characters are two overlapping sets.** Every left-out shot of sucre
  and of sara is also michael's; walter's whole scenes are 716 of the
  episode's 794 shots. The tool's total counted a shared shot once per
  character; it now also prints each shot once.
- **The count of holes up to 6 s** was not in the row. Carried through by
  the reviewer it speaks against such a rule: it would take back under half
  of the `talk` seconds (46 % and 38 %), and about a quarter of them lie
  before the first or after the last kept shot of a scene, where no hole
  rule reaches.

### What the sheets show

From the reviewer's reading of all sheets of sara, sucre and jesse, and the
author's of two:

- **Where two people talk, the filter keeps nearly every shot.** Sara's two
  infirmary scenes lose one shot of 5 s. Sucre's cell scenes lose none.
  Jesse's long two-person scenes are nearly whole.
- **What it cuts inside talk**: single wide or from-behind shots of the two
  speakers (Jesse: six shots of 3 to 9 s), and cutaways (a dog five times, a
  licence plate; in Sucre's yard scene the tower and the man they talk
  about). A viewer would notice a jump in picture and sound each time.
- **What it guts**: stretches in which faces are hidden. Of the cook's 44
  shots 10 stay; of the drive in the gas mask 1 of 25.
- **What it rightly leaves out**: opening views, titles, the police outside
  the bank. The filter leaves out 7 to 18 % of whole scenes by seconds.
- **The fault an eye meets first is not the filter's**: scenes that are not
  the character's. Sara's pack opens with Michael's wall of clippings and
  the whole bank robbery, because her photo hangs on that wall; the meat
  plant is joined to her infirmary scene. Sucre's pack holds Michael's visit
  to Lincoln, and a yard scene in which the reviewer could not find him. Jesse's holds the raid through one face that is not his. Of a
  filtered pack, the shots from labelled scenes in which the character has
  no own shot are 55 % for bogdan, 30 % for sara, 13 % for sucre and 7 % for
  michael. "Whole scenes" has all of them as well.
- Part of what is left out shows a face the cast did not give to anyone
  (walter: 24 shots, 73 s).

### What it cannot say

- Two episodes, in effect two leads' scene sets; the third character of each
  title was picked as the filter's worst case.
- Stills without sound: whether speech runs over a left-out shot is what
  matters for a conversation pack, and nobody measured it here.
- A line of "a third" for which the row gave no reason.
- No pack that keeps holes was made or looked at.

### For the owner

- **Watch two of the videos before deciding**:
  `E:\as-ownmodels\scenes\eye\packs\pb_sucre.filter.mp4` (8.5 min) and
  `bb_jesse.filter.mp4` (21 min), each beside its `.scenes.mp4`. Your eye on
  these counts more than any number above.
- **What the look supports**: in talk the filter does what it was meant to
  do and loses single faceless shots; in action and montage it loses whole
  stretches; and the packs carry scenes that are not the character's,
  filter or not.
- **What it does not support**: that the filter "cuts into conversations" as
  a measured fact, or that keeping short holes is the fix.
- **Two things worth their own rows**, neither tried here: keep the shots
  without a face over which the conversation goes on, chosen by the speech
  marks the bench has and not by a length; and do not let one face on a
  photo, or one wrong face, make a scene the character's.
