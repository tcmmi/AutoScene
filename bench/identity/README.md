# Identity bench

How well AutoScene turns faces into characters, measured against labels a
person gave by looking at every face track.

## Breaking Bad S01E01 (`bb_s01e01.labels.tsv`)

Labels for the BluRay remux of the pilot (58 min, 1080p, 23.976 fps, 83,593
frames) — the file itself is not in the repository. One row per face track
of the face pass of 2026-10-14: `start end name boxes` (frames on the
decoded frame grid; `boxes` holds every detection as `frame:x0,y0,x1,y1`, a
share of the frame). `?` is someone unnamed (an extra, a photo, not a face),
`x` a face nobody could tell (gas masks, dark backs of heads) — left out of
the score.

Labels are anchored by time and place, not by track id, the way the app
re-anchors the user's decisions after a re-analysis: a later face pass whose
tracker joins or splits tracks still finds its names. A track takes the name
most of its frames have, among labels it shares most of the shorter of the
two with and whose face is where the track's face is during those frames.

```sh
# 1. Face pass over the episode (needs the files of `faces_run`'s doc comment)
cargo run --release -p as-analysis --example faces_run -- <ort_dir> <models_dir> <ffmpeg_dir> <bb_dir> <out>
# 2. Score (link, assign, margin, unsure override the defaults)
cargo run --release -p as-analysis --example identity_bench -- score <out>.tracks bench/identity/bb_s01e01.labels.tsv
```

The main cast is everyone named with at least 30 s on screen. BCubed is
reported per track and per frame on screen (a long, clear shot counts more
than a blurry glimpse).

BCubed only sees faces that became a track. The line `main cast faces
(labels)` measures end to end over the labelled faces instead: the share a
track still holds (`in a track`), and the share held by a track of the
character named like them (`on their character`). It catches a model or
tracker change that drops faces. The labels came from the w600k_r50 face
pass, so that pass holds every labelled face by construction.

Results on S01E01 (main cast; defaults of each embedder; labels before the
corrections of 2026-10-17):

| Embedder | per track P / R / F | screen time F | in a track | on their character |
|---|---|---|---|---|
| w600k_r50 (buffalo_l) | 0.987 / 0.824 / 0.898 | 0.953 | 1.000 | 0.976 |
| glintr100 (antelopev2) | 0.984 / 0.873 / 0.925 | 0.958 | 0.991 | 0.973 |
| AdaFace IR-101 (tools/model-prep) | 0.980 / 0.811 / 0.887 | 0.948 | – | – |

Label corrections (2026-10-17, checked on the thumbnails): the face in bed
at 79806–80393 is Skyler, not Walter; `brownhair_blurry` and `teary` are one
woman (a guest at the party), now both `teary`; the two `?` faces
at 50976 and 50988 are Walter (his reflection in a glass door, and him in
the dark).

## One person, one character

Two faces found in the same frame are two people — except when they are
one person seen twice: Walt Jr. and Walter in front of the clothing store's
mirror, Krazy-8 found twice by the tracker in one gap. Before
`as_identity::VERSION` 2 a single such pair kept two groups of one person
apart for good: Walt Jr. was two characters (32 + 15 tracks). Now faces
count as seen together only when both were found in the same frame, and
after the strict clustering a second one joins whole groups whose mean
faces are clearly one person's (`Params::copy_link`) when only a few of
their faces were seen together (`copy_share`: 15 % of the smaller group,
at least one) — one face of each such pair steps aside (see
`as_identity::ahc`). How alike two faces side by side are doesn't count:
a twin beside their twin in the same good light looks like a mirror image.
A group seen (nearly) only with a bigger one, most of whose faces are the
face beside them seen twice, is a reflection, not a character. Twins who
are on screen together in 4 or more of 20 shots stay two — also when
one of them is blurrier, or clearer in the shots they share, and one
episode at a time in a big series. Unit tests in `as-identity` cover each
rule: switching any of 18 off (or the share to 25 % or 0) fails one.

Identical twins (or one actor in two roles) who are never seen apart
look like a reflection and lose their character; they can be given one
by hand (a character made by hand is pinned and stays).

glintr100, corrected labels:

| Grouping | characters | per track P / R / F | screen time F | in a track | on their character |
|---|---|---|---|---|---|
| before (VERSION 1) | 41 | 0.988 / 0.874 / 0.927 | 0.972 | 0.991 | 0.980 |
| VERSION 2 | 40 | 0.988 / 0.902 / 0.943 | 0.982 | 0.991 | 0.978 |

(`on their character` drops a little: the reflection of a mirror shot is
nobody now — its person is on screen in the same frames anyway.)

`identity_bench stress` on S01E01 — 120 mirror copies of random faces,
eight reflections shot after shot (0.53 – 0.65 like the face beside them,
as real ones are), the episode cut into 2, 4 and 8 episodes, 70 % of the
faces, link 0.25 – 0.4 and copy_link 0.5 – 0.7, the cast built up episode
by episode with characters kept: 27 runs, before 19 people split (Walt Jr.
in nearly every run), now 2 — the runs with 60 and 120 copies, where Walt
Jr.'s two halves get three or more mirror moments between them, as often
as twins seen together; characters holding two people: 0 before and after.
Reflections less than 0.5 like the face beside them stay a character of
their own: below that, identical twins always seen together look the same.

`identity_bench scale <tracks> <labels> <n>` simulates a series: the
episode `n` times over as `n` episodes, every named face keeping how much
it says about who it is but looking like another shot of that person.
People split (characters holding two people: 0 everywhere):

| episodes (faces) | 3 (3,414) | 5 (5,690) | 10 (11,380) | 20 (22,760) |
|---|---|---|---|---|
| before (VERSION 1) | 4 | 2 | 4 | 9 |
| VERSION 2 | 0 | 0 | 2 | 5 |

(A face seen twice at once stays a copy of its other face in every
simulated episode.) The groups left over at 10 and 20 episodes are rare
looks — mostly Jesse's — whose mean faces are 0.06 – 0.29 like the
person's, where two people's are up to 0.31: no grouping rule tells those
apart, a merge by hand does (it pins every face, so it holds through every
later grouping). Grouping 20 episodes takes about 6 s, as before.

What separates them, measured here: mean faces of one person's groups held
apart by a mirror or a stray face 0.71 – 0.83 (one face of 11 – 15 seen
together), of two people's groups 0.31 at most; people on screen together
80 % of the time or more (a class, a party): a face and the one beside it
0.24 at most — a face and its reflection 0.53 – 0.64.

When a grouping joins a character into another, its scenepack goes with
it (filters, shots picked or dropped). "Not them" doesn't: said of a
second character of one person, it may well have been said of that
person's own faces.

Lowering `FaceParams::min_track_quality` (0.05 → 0.02) or the norm floor
(14 → 13) for glintr100 moves `in a track` by at most 0.003 and costs
per-track F; the faces it drops are below any usable quality.

Characters without a visible face (bodies tracked through shots, found
again by their clothes): see `bench/bodies`.

## Two selves, two characters

A hero and the everyday self are one person and, to the viewer, two
characters. They are never on screen together, so no face seen beside
another holds them apart; and in the space fitted to 3D animation
(`as_identity::adapt`) the bar two groups join at lies where strangers still
look alike. On two episodes of a 3D series (`bench/3d`) a heroine and her
masked self are 0.400 alike there, the bar is 0.39986, and the two became one
character of 243 tracks.

Two tight groups with a gap between them stay two (`as_identity::ahc`):
groups of at least 15 tracks each (`Params::tight_tracks`) do not join when
they are less alike to each other than 0.70 (`Params::tight_animated`) of how
alike the less coherent of them is in itself. The rule is on in the space
fitted to footage the user called animation, for the full scan's faces, and
nowhere else: not for live action, not for footage that only looks like
animation, not for the anime embedder's faces, not for Fast Scan (nothing was
measured for SFace). The full scan's casts of 3D animation group at a version
of their own (`as_identity::ANIMATION_VERSION`), so only they were grouped
again for it.

On `bench/3d` (main cast, per track): both episodes as one project P 0.907
R 0.807 F 0.854 before, P 0.967 R 0.809 F 0.881 with the rule; `stress` 156
people split and 118 characters holding two people before, 154 and 94 with
it. On fifteen episodes of the series in 41 projects the two selves are one
character in 10 without the rule and in 1 with it; of the 12 characters it
parts there, two readers call 9 the two selves, 2 two different people and 1
one figure parted wrongly; nine other 3D films group the same with and without
it. The share has little room: the selves are 0.54 to 0.68 of their own
cohesion alike on the tuning episodes (over 0.70 in one of thirteen others),
the least alike halves of one figure that the grouping joins 0.73.
`bench/3d/README.md` has the tables and what was laid down before the series
was grouped.

`identity_bench` takes `TIGHT=<share>` (the rule from that share on, in every
space; 0: nowhere) and `TIGHT_TRACKS=<n>`. Unit tests in `as-identity` cover
it; twelve ways of breaking the rule each fail one.

## A merge by hand holds

Merging character B into character A pins B's faces to A (`as-db`
`merge_characters`); A's own faces stay as the grouping found them. When the
cast was grouped again (an episode added, a newer version of the grouping),
B's pinned faces were one group and held A, and A's own faces were mostly too
unlike them to join that group: had the two been alike, the grouping would
have made one character of them, and nobody would have merged them by hand.
A's own faces then went on as a new character, and A's name stayed with the
faces that had been merged in. Seen in the running app on 2026-11-14 (three
episodes of the 3D series, footage type animation; the code is the same for
every kind of footage): a character named after one girl, a boy's character
merged into it, the cast grouped again: the name was on the boy's 119 faces
and four of hers, and her other 112 faces were a new, unnamed character.

Now a character the user pinned faces to keeps the faces it had
(`keep_own_faces` in `as_identity::identify`): the unpinned group that holds
most of what a character held besides the pins (over half, by quality), and
was that character more than any other, goes on as that character together
with the group pinned to it. The same project grouped again with it: one
character, the name in place (119 + 111 faces, and 103 of the third
episode). The heroine and her masked self merged by hand, either way round,
stay one character at every later grouping, on two episodes and on five
(where the two groups are under the bar and the merge was undone before).

What it does not do, and what stays as it was:

- **A group that is not most of the character does not follow.** A named
  character (a few of its typical faces are pinned) whose faces a better
  grouping parts in two keeps the part its pinned faces are in; the other
  part goes on as a new character, as before. So a wrong join inside a named
  character can still come apart, and so do a hero and the everyday self
  that an older grouping had put on one named character. If the pinned faces
  are in the smaller part, the bigger one is most of the character: the two
  stay one, where before the name went with the smaller part alone.
- **A group that was mostly another character does not follow** a pin: a
  face once placed with a character the user made of three faces does not
  drag the forty faces of its real character along.
- **One face moved to an unnamed character still carries its group.** The
  moved face is that character's only pin; it groups with its own person,
  whose group is then pinned to the character; and the character's own faces
  are most of what it held: the two become one. That is what happened before
  too (then by the grouping joining them, or by the moved face's group
  taking the character from its own faces). A pinned group that was mostly
  another character should leave the pinned face with the character and
  keep its own; that is not built.

Unit tests in `as-identity`: a merge of two unlike characters in three sizes,
with new episodes and a second grouping after it; a named character that
lets go; a group that was mostly another character. Six ways of breaking
`keep_own_faces` each fail one (`never called`, any share, nearly all, the
pins counted, the least of its earlier characters, the faces left behind).
Nothing changes without pins (the function returns at once), and the
bench's runs have none: their scores are the same to the letter.

## Rescan

Rescan judges faces against every character and, for unclear ones, reads them
again at the source's own resolution (`as_analysis::rescan`,
`as_identity::judge`). `rescan_bench` scores it on a COPY of a project:

```sh
cargo run --release -p as-project --example rescan_bench -- <project_copy_dir> [labels.tsv]
READ=<ort_dir>,<models_dir>,<ffmpeg_dir> cargo run --release -p as-project --example rescan_bench -- <project_copy_dir>
```

The owner's pins, asked about blind, on the Miraculous film's 4K original
(2026-11-07): of 81 hard pins (below `core_quality`) 9 were clear and right
from the stored faces, 26 when read again, at 0 and 2 clear and wrong; Everyone
else went from 4 clear matches to 25. Clear faces gain nothing (173 right, 5
wrong became 167 and 7), so only unclear ones are read. 104 s per 100 faces
from a hard disk. **Live action is unproven:** no original was at hand.

Stored faces alone, all pins and labels of five casts: 209 clear verdicts
right, 6 wrong; on Breaking Bad no stranger got a clear match, but none of the
17 faces on the wrong character was noticed, and a "maybe" is right less than
half the time. Those stay suggestions for the person.

A summary of a character hides its rare looks (Wang Fu's disguise, 13 of his
34 tracks, went unrecognised), so every face is now compared with every shot:
a character scores the mean of its 2 best shots. A character on screen around
the face (fading over 480 frames, never one on screen at the same time) gets
a bonus that can make a guess but never a clear answer: looks alone do that.
Only a face too unclear to tell gets it; a sharp face that looks like no one
is no one, whoever is close by.
A reading far below the stored face's quality is dropped. Miraculous, 74 hard
and 226 clear pins asked blind: clear 33 right 1 wrong and 176 right 2 wrong,
guesses 22 right 2 wrong and 31 right 4 wrong. Everyone else: 32 clear. Actors
need stricter bars (`Tuning::of`): on Breaking Bad's stored faces 1 stranger
of 106 in Everyone else gets a guess, and none a clear answer.

Two groups, clearest 8 faces each (`likeness`): one person split by looks
0.33-0.79 (median about 0.65) in live action, people seen together 0.06 at the
median, 0.27 at most on Breaking Bad, 0.57 on The Blacklist.

## Fast Scan

Fast Scan finds faces the way the Scener app does (YuNet, SFace, a few
frames per shot; `as_analysis::fast`), so its tracks are sparser: score them
with its grid and SFace's parameters.

```sh
cargo run --release -p as-analysis --example fast_run -- <ort_dir> <models_dir> <ffmpeg_dir> <bb_dir> <out>
GRID=6,8 SFACE=1 cargo run --release -p as-analysis --example identity_bench -- score <out>.tracks <labels.tsv>
```

`GRID=<stride>,<gap>` is the grid its presence spans are read on (6,8 at
24 fps: a look at most every 2 s), `SFACE=1` starts from `Params::sface`.
`in a track` and `on their character` count a labelled face only when a
track has a detection in exactly that frame, which a pass that looks once
a second rarely has; compare passes over time instead (a labelled face is
held when a track is on screen then, its face where the label's is).

Both passes on the same inputs (2026-10-27). Breaking Bad from its 960×540
playback proxy — **the proxy, not the original remux** (which is gone), so
lower than the original's numbers above; Miraculous (3D animation, 4K
HEVC) against its extended labels:

| | BB proxy: full pass | BB proxy: Fast Scan | Miraculous: full pass (Scales, 0.5) | Miraculous: Fast Scan |
|---|---|---|---|---|
| tracks / faces | 1126 / 31,887 | 1129 / 4343 | 2834 / 51,016 | 2279 / 6383 |
| main cast per track P / R / F | 0.989 / 0.910 / 0.948 | 0.974 / 0.761 / 0.854 | 0.946 / 0.696 / 0.802 | 0.910 / 0.458 / 0.609 |
| per screen time P / R / F | 0.993 / 0.961 / 0.977 | 0.993 / 0.940 / 0.966 | 0.944 / 0.843 / 0.890 | 0.932 / 0.634 / 0.754 |
| main cast faces held / on their character (over time) | 0.970 / 0.955 | 0.804 / 0.779 | 0.990 / 0.937 | 0.681 / 0.586 |
| stress (27 runs) | 4 split, 0 merged | 0 split, 0 merged | | |

Fast Scan groups what it finds about as well as the full pass; what it
loses is faces. YuNet holds 90% of the labelled main-cast faces at the
looks of BB (SCRFD 97%) and fewer of a CGI film's. Scener keeps YuNet's
faces from 0.85: on the BB proxy that holds 76% at the looks and puts 65%
of the main cast's screen time on their character; Fast Scan keeps them
from 0.7 and lets the less sure ones count for less (without that, faces
from 0.7 split Walter in most stress runs).

Since 2026-11-11 Fast Scan's detector is SCRFD-2.5G, the smaller sibling
of the full pass's, looking at each frame twice (at 640 and 320 pixels; 3D
animation at 1280 and 320) instead of YuNet — Scener's detector — looking
once at up to 1280 (`as_analysis::fast`). The passes on the BB proxy that
day (`fast_run`'s doc comment says how to run the earlier ones):

| | YuNet-s, one look (the first Fast Scan) | YuNet-n, 960 + 320 | SCRFD-2.5G, 640 + 320 (now) |
|---|---|---|---|
| tracks / faces | 1129 / 4343 | 1147 / 4500 | 1080 / 4404 |
| main cast per track P / R / F | 0.973 / 0.752 / 0.848 | 0.980 / 0.762 / 0.857 | 0.976 / 0.834 / 0.900 |
| per screen time P / R / F | 0.991 / 0.937 / 0.963 | 0.986 / 0.910 / 0.947 | 0.985 / 0.962 / 0.974 |
| main cast over time, in a track / on their character | 0.735 / 0.712 | 0.763 / 0.732 | 0.797 / 0.781 |
| stress (27 runs) | 0 split, 3 merged | 1 split, 18 merged | 0 split, 0 merged |
| time (graphics card) | 40 s | 44 s | 49 s |

SCRFD-2.5G's scores count for quality from 0.5 to 0.8 as YuNet's did from
0.7 to 0.95; without that, 2 runs of the stress test split someone and 1
merged. Looking bigger does not help live action: at 960 and 320 pixels it
holds as many faces (0.803) but puts fewer on their character (0.754, one
merge), and so does SCRFD-10G at 640 and 320 (0.814 / 0.764, three merges):
the faces a bigger look adds are the small ones SFace tells apart worst.
On a processor alone the pass takes 11% longer than with YuNet-n (a
1080p episode: 221 s against 199 s).

### Would a better embedder help Fast Scan? (2026-11-11)

Asked before training one (`bench/models`): `fast_run` puts an
ArcFace-style graph in SFace's place with `EMBEDDER=<file>`. On the BB
proxy, scored with `GRID=6,8` and no `SFACE=1`, each at the best bars of a
sweep (link 0.30–0.55, assign 0.22–0.45); glintr100 with its own norms
(14–22), w600k_mbf — InsightFace's small embedder (buffalo_s, 13.6 MB) —
with the norms at the same share of faces (10.39–16.98, `identity_bench
norms`). A graph other than glintr100 runs on the processor (`DEVICE=cpu`):
it has no goldens to prove itself on a graphics card.

| | SFace (as shipped) | glintr100 | glintr100 and SCRFD-10G at both looks | w600k_mbf |
|---|---|---|---|---|
| link / assign | `Params::sface` | 0.30 / 0.28 | 0.30 / 0.28 | 0.40 / 0.28 |
| tracks / faces | 1080 / 4404 | 1072 / 4358 | 1088 / 4476 | 1233 / 4556 |
| main cast per track P / R / F | 0.976 / 0.834 / 0.900 | 0.982 / 0.918 / 0.949 | 0.982 / 0.915 / 0.947 | 0.985 / 0.722 / 0.833 |
| per screen time F | 0.974 | 0.984 | 0.981 | 0.925 |
| main cast over time, in a track / on their character | 0.797 / 0.781 | 0.791 / 0.782 | 0.811 / 0.801 | 0.801 / 0.779 |
| main cast clips, precision / recall | 0.981 / 0.920 | 0.988 / 0.937 | | |
| stress (27 runs) | 0 split, 0 merged | 0 split, 0 merged | 0 split, 0 merged | |
| time (graphics card) | 55 s | 81 s | 87 s | |
| time (processor alone, two runs) | 208–224 s | | | 162–171 s |

The strongest embedder here groups Fast Scan's tracks better — fewer short
tracks stay unsure or unassigned (per-track F 0.949 against 0.900) — but
hardly changes what a person gets: the main cast's time on their character
stays where it was (0.782 against 0.781), and their packs gain 1.7 points of
recall. Fast Scan loses faces, not names: a track holds 80% of the main
cast's time, and of that SFace already puts 98% on the right character.
Only with the full pass's detector at both looks as well does the time on
their character reach 0.801 (with SFace that detector merged three
characters) — four times the detector's work and the full pass's embedder,
which is the full scan on fewer frames rather than a Fast Scan. w600k_mbf
tells these small faces apart worse than SFace does (Walter in four
characters), though the pass is a fifth to a quarter faster with it on a
processor.

So neither a ready-made embedder nor a student of glintr100 takes SFace's
place: the gate asked for 0.02 more time on their character
(`bench/models/README.md`), and the teacher itself gives 0.001. What would
help Fast Scan is finding and holding more faces. One title, at the proxy's
960×540.

Time on the film's last 22,319 frames (warm file cache, idle RTX 3060 Ti,
16 threads; two runs each): the full pass 133–149 s for 3D animation,
113–134 s for live action; Fast Scan 39–57 s decoding on the GPU (NVDEC),
87–112 s decoding in software — the decoder, not the models, sets its
pace. Whole: the film 371 s with Fast Scan, the BB proxy 40–50 s. (Whole
runs of the full pass shared the GPU with another app and aren't
comparable.)

### Where it pays off

The work each pass is, counted from its ONNX graphs at the engine's input
sizes, in multiply-adds per minute of 24 fps video:

| Pass | Multiply-adds per minute of video |
|---|---|
| Full scan, live action or mixed | 10.0–10.6 T (SCRFD 3.3–3.9, ArcFace R100 6.7) |
| Fast Scan, live action | 0.09 T (about 110× less) |
| Full scan, anime | 25–34 T |
| Fast Scan, anime | about 7.8 T |
| Shots pass (both) | 1.2 T |

On graphics built for AI work (matrix units the runtimes use: NVIDIA's
tensor cores, AMD's RDNA 3 and newer, Intel Arc) the full scan waits for
the decoder, like Fast Scan: on the RTX 3060 Ti its models take about 2.6 s
per minute of video. Without them the work shows. On this desktop's
processor the full scan's models alone take 0.62–0.87× the video's length,
on a 4-core laptop processor an estimated 1.3–2.6×; built-in graphics sit
between, with 1/40 (UHD 620) to 1/2 (Apple M Pro class) of the RTX card's
FP32. (Those timings were taken while the machine was shared: read them as
orders of magnitude.)

So on Automatic, new projects start with Fast Scan where the graphics
aren't built for AI work (`as_project::scan`). Two kinds of footage keep
the full scan, where Fast Scan costs most: 3D animation (0.586 of the main
cast on their character instead of 0.937, above), and live action while
people without a visible face are found, whom Fast Scan can't find.
