# Scenes bench

Which shots belong to one scene, measured against scene boundaries a person
set. It serves the scenes slot (`bench/models/DECISIONS.md`, rows 6 to 8):
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
2 frames. Six scene starts fall into black or a fade; they stand at the first
shot after it.

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
  precision of the scores, and F when a boundary one shot off still counts.
- **Overlap**: each scene's best intersection over union with a scene of the
  other side, weighted by its frames, the mean of both directions.

A threshold chosen on the episode it is read on flatters it. `score` also
reads each half of the episode (by scenes) with the threshold that is best on
the other half; `tools/slot-bench/scene_titles.py` runs several episodes and
gives each one's result at its own best threshold, at the one threshold that
is best over all of them, and at the threshold that is best over the OTHER
episodes, which is what a new title would get.

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
| Step A | 174 | 0.145 / 0.781 / 0.244 | 0.411 | 0.196 | 0.514 | 0.832 | 0.764, 0.829 |
| Step A, short pieces join first | 41 | 0.425 / 0.531 / 0.472 | 0.515 | 0.390 | 0.616 | 0.892 | 0.765, 0.909 |
| Step B, Places365 ResNet-50 | 118 | 0.231 / 0.844 / 0.362 | 0.543 | 0.480 | 0.644 | 0.890 | 0.849, 0.854 |
| Step B, Places365, short pieces join first | 43 | 0.405 / 0.531 / 0.459 | 0.649 | 0.448 | 0.659 | 0.924 | 0.834, 0.928 |
| Step B, DINOv2-small | 108 | 0.290 / 0.969 / 0.446 | 0.647 | 0.546 | 0.728 | 0.880 | 0.826, 0.862 |
| **Step B, DINOv2-small, short pieces join first** | 36 | 0.571 / 0.625 / 0.597 | 0.774 | 0.589 | 0.783 | **0.931** | 0.839, 0.912 |
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

| Episode | Own best threshold | One threshold for all (0.675) | The other episodes' threshold | Scenes found / labelled | Boundary F, one shot off | Overlap |
|---|---|---|---|---|---|---|
| Breaking Bad S01E01 | 0.930 | 0.917 | 0.917 | 32 / 33 | 0.714 | 0.754 |
| Prison Break S01E01 | 0.948 | 0.926 | 0.926 | 31 / 40 | 0.828 | 0.834 |
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

Not measured yet: more animation (2D and 3D), a larger picture network, who
speaks across the cut (step C), scenes across the episodes of one project.
