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
| groups (b) | 94.4 | 415 | 34.7 | 0.819 (0.708 to 0.921), 0.752 | 0.819, 0.752 | 0.288 (0.195 to 0.383), 0.330 |
| **walk (c)** | 119.9 | 540 | 60.2 | **0.985** (0.964 to 1.000), 0.965 | 0.972 (0.949 to 0.993), 0.957 | **0.778** (0.656 to 0.895), 0.735 |
| bridged walk (e), with the stepped-over shots | 152.8 | 641 | 93.1 | 0.829 (0.765 to 0.890), 0.795 | 0.787, 0.774 | 0.954 (0.895 to 0.997), 0.920 |
| bridged walk (e), without them | 135.0 | 603 | 75.2 | 0.982 (0.960 to 0.999), 0.966 | 0.951 (0.907 to 0.987), 0.948 | 0.947 (0.885 to 0.994), 0.913 |
| scenes, partners (g) | 138.9 | 621 | 79.2 | 0.968 (0.941 to 0.990), 0.941 | 0.912 (0.825 to 0.975), 0.914 | 0.956 (0.901 to 0.995), 0.925 |
| scenes, anyone (f) | 143.4 | 636 | 83.7 | 0.927 (0.882 to 0.968), 0.909 | 0.857 (0.751 to 0.939), 0.870 | 0.960 (0.905 to 0.999), 0.930 |
| whole scenes (d) | 190.0 | 741 | 130.2 | 0.692 (0.591 to 0.791), 0.592 | 0.629 (0.528 to 0.731), 0.566 | 0.967 (0.912 to 1.000), 0.938 |

The same as shots, over all 12 characters together (926 shots of their
scenes show a speaker and not the character):

| Set | Added shots | with a speaker | faces that do not speak | no labelled face | in a scene the character is not in | of the 926 held |
|---|---|---|---|---|---|---|
| groups (b) | 416 | 313 | 34 | 69 | 0 | 306 |
| walk (c) | 722 | 697 | 25 | 0 | 6 | 681 |
| bridged walk (e), with the stepped-over shots | 1,117 | 888 | 90 | 139 | 24 | 852 |
| bridged walk (e), without them | 903 | 872 | 31 | 0 | 17 | 845 |
| scenes, partners (g) | 950 | 894 | 56 | 0 | 34 | 857 |
| scenes, anyone (f) | 1,004 | 913 | 91 | 0 | 49 | 861 |
| whole scenes (d) | 1,563 | 925 | 229 | 409 | 77 | 869 |

Measure (2), the others' speech whose speaker is seen in the added shots and
in the set at all: own –, 0.783; groups 0.779, 0.906; walk 0.865, 0.907;
bridged walk 0.901, 0.925; (g), (f) and whole scenes 0.933, 0.958.

Per character, the walk and what would replace it (added shots, (1), speaker
shots held of those asked for):

| Character | Own shots | Walk (c) | Bridged walk, without the stepped-over shots | Scenes, anyone (f) | Whole scenes (d) |
|---|---|---|---|---|---|
| walter | 300 | 198, 0.89, 168/176 | 205, 0.89, 173/176 | 217, 0.85, 175/176 | 416, 0.45, 176/176 |
| jesse | 122 | 112, 0.99, 111/191 | 167, 0.98, 163/191 | 180, 0.93, 166/191 | 301, 0.56, 168/191 |
| skyler | 66 | 82, 1.00, 77/81 | 93, 1.00, 80/81 | 109, 0.96, 81/81 | 123, 0.85, 81/81 |
| hank | 39 | 21, 1.00, 21/42 | 42, 1.00, 42/42 | 56, 0.75, 42/42 | 97, 0.43, 42/42 |
| waltjr | 42 | 68, 1.00, 67/71 | 71, 1.00, 70/71 | 76, 0.95, 71/71 | 88, 0.82, 71/71 |
| krazy8 | 46 | 44, 0.93, 41/60 | 57, 0.93, 53/60 | 62, 0.92, 57/60 | 110, 0.55, 60/60 |
| gomez | 19 | 36, 1.00, 36/62 | 62, 1.00, 62/62 | 76, 0.82, 62/62 | 117, 0.53, 62/62 |
| marie | 22 | 45, 1.00, 45/45 | 45, 1.00, 45/45 | 45, 1.00, 45/45 | 50, 0.90, 45/45 |
| emilio | 26 | 50, 1.00, 50/125 | 85, 0.99, 84/125 | 93, 0.96, 89/125 | 158, 0.58, 91/125 |
| denim | 14 | 23, 1.00, 23/31 | 31, 1.00, 31/31 | 31, 1.00, 31/31 | 36, 0.86, 31/31 |
| mustache | 13 | 32, 1.00, 32/32 | 32, 1.00, 32/32 | 32, 1.00, 32/32 | 37, 0.86, 32/32 |
| bogdan | 8 | 11, 1.00, 10/10 | 13, 1.00, 10/10 | 27, 1.00, 10/10 | 30, 0.90, 10/10 |

The two other readings give the same picture. With the stored cast and the
labelled scenes: walk 0.985 and 0.779 held, bridged walk without the
stepped-over shots 0.973 and 0.959, (g) 0.971 and 0.982, (f) 0.932 and
0.993, whole scenes 0.701 and 1. With the labels for both: 0.986 and 0.788,
0.976 and 0.973, 0.977 and 0.991, 0.940 and 1, 0.704 and 1. So neither the
stored cast nor the scenes the app finds cost much here: 57 of the 926
speaker shots lie in a scene the recipe cut apart from the own shots (34 of
Emilio's, 23 of Jesse's).

### What the numbers say

- **The walk meets the draft bar (0.985 against 0.80), and the bar says
  little.** Of the 579 shots in which the cast is sure of someone, 527 show
  a speaker of their scene (0.910). The walk can only add shots with such a
  face, so any rule of that kind passes. What counts is the gap to "any
  face" (f): 0.06.
- **0.1's walk leaves out about a quarter of the shots that show a speaker
  of the character's scenes** (held 0.778, pooled 0.735). Of the 245 it
  leaves out, 176 lie behind a shot that stops it: one in which the cast
  shows nobody, or nobody who shares a frame with the character. 57 lie in a
  scene found apart, 8 show nobody the cast knows, 4 only strangers to the
  character. A partner who never shares a frame with the character, which
  the proposal named as the walk's limit, is rare here.
- **The 6-second bridge closes that gap** (row 27): 0.947 held, at the same
  share of added shots with a speaker (0.982). Character by character
  against the walk: +0.169 held (+0.079 to +0.267), −0.002 on (1). With its
  stepped-over shots in the set, (1) is 0.829 and under 0.80 for 5 of the 12
  characters: those shots hold nobody the measure can credit (16 of 214
  show a speaker).
- **A walk is not needed for that** (row 28). Whole scenes with only the
  shots in which the cast shows anyone (f) hold as many speaker shots
  (0.960), and need no new query: a pack's shot already carries how many
  characters the cast shows in it. The partner rule (g) and the bridged walk
  add fewer shots without a speaker: (f) minus the bridged walk is −0.055 on
  (1) (−0.101 to −0.018). That gap sits in the party scene (Hank 0.75
  against 1.00, Gomez 0.82 against 1.00) and where the recipe joins two
  scenes: (f) then takes every face of the wrong scene (49 added shots in a
  scene the character is not in, against 17).
- **Against whole scenes**: 0.31 of the shots whole scenes add show no
  speaker (0.195 no labelled face, 0.113 faces that do not speak). The
  filtered sets drop those and keep 0.95 to 0.96 of the speaker shots, with
  under two thirds of the added shots.
- **Against shot groups**: groups hold 0.288 of the speaker shots. They
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
  title. Without Denim and Mustache the means of (1) and (4) are 0.982 and
  0.759 for the walk, 0.979 and 0.937 for the bridged walk, 0.913 and 0.953
  for (f); for Walter, Jesse and Skyler alone 0.962 and 0.829, 0.957 and
  0.941, 0.913 and 0.954.
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
- The review found three slips in `bench/identity`'s labels, not corrected
  here: `wavy_guest` is Skyler (four tracks at the party), the `jesse` at
  frame 10437 is a passer-by, and Jesse in shot 729 is `?`. They move no
  mean here by more than the third decimal. Jesse has 191 asked-for shots
  with the stored cast and 169 by the labels for that reason.

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
