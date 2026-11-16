# Identity bench

How well AutoScene turns faces into characters, measured against labels a
person gave by looking at every face track.

## Breaking Bad S01E01 (`bb_s01e01.labels.tsv`)

Labels for the BluRay remux of the pilot (58 min, 1080p, 23.976 fps, 83,593
frames) — the file itself is not in the repository. One row per face track
of the face pass of 2026-10-14 (since 2026-11-15 one of them is cut in two:
1,415 rows): `start end name boxes` (frames on the
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

Label corrections (2026-11-15; row 30 of `bench/models/DECISIONS.md`). Found
by the reviewer of the conversation bench (`bench/scenes`). Checked on
frames of the 960×540 proxy and against the stored cast of the project the
scenes bench reads (the app's own analysis of that proxy), which had all of
them right:

- `wavy_guest` is Skyler: four tracks at the party (15573–15643,
  15648–15671, 17325–17460, 17580–17740; 131 boxes, 16 s). At the door the
  face of the `skyler` row before them (15552–15570) goes on as the first of
  them, and in the stored project one face track of the app holds both.
  In the two wide shots the face is 28 pixels wide; the row stays on one
  woman, in the grey cardigan over a pregnant belly, beside Marie, and no
  other row there is Skyler's.
- The `jesse` at 10437–10470 is a passer-by in front of the school, a
  blurred face under a beanie that crosses the frame in 1.4 s: now `?`. The
  face itself cannot be told; Jesse is not in that scene, and the app's cast
  gives the track to no one.
- The `?` at 73512–73568 is Jesse, lying in the sand in the red vest, with
  the bruised eye, while Walter lifts him. 16 of the row's 19 boxes are on a
  track the app's cast has on Jesse; the first three, where an arm covers
  the face, on a short track it gives to no one.

Six names changed, no box. A reader who was shown only unnamed crops beside
a gallery of the main cast, with six rows nobody questions mixed in, gave
the new name for four of the six rows and could not tell the two wide shots
at that size; there the name rests on what the shot shows around the face
and on the app's cast. A reviewer without context looked at frames of its
own and came to the same six names.

Left at that point, and corrected next ("A second correction" below): the
last two boxes of the `skyler` row at the door (15570 and 15573), which sit
on Walter's face during the kiss; and five `?` rows in the RV that are faces
behind a gas mask, which the rule above calls `x`.

The app's face pass with its people on the proxy, as run for `bench/bodies`
on 2026-11-15 (1,126 tracks; `score` with its defaults), with the labels
before and after:

| Labels | main cast per track P / R / F | screen time P / R / F | everyone named, per track P / R / F | in a track | on their character | over time: in a track, on their character |
|---|---|---|---|---|---|---|
| before | 0.990 / 0.906 / 0.946 | 0.994 / 0.960 / 0.977 | 0.985 / 0.898 / 0.939 | 0.970 | 0.955 | 0.962, 0.941 |
| corrected | 0.996 / 0.909 / 0.951 | 1.000 / 0.961 / 0.980 | 0.994 / 0.900 / 0.945 | 0.970 | 0.956 | 0.962, 0.942 |

Most of the precision the pass seemed to lack was these labels: Skyler's
character held the four tracks of the "guest" (94 % of it hers before, 100 %
now) and Jesse's the track of the shot in the sand (99 %, now 100 %); the
passer-by counted as a face of Jesse's that the pass had left unassigned.
`stress` gives the same 27 runs with both (4 people split, no character
holding two).

Every Breaking Bad number of this file dated before 2026-11-15 was measured
with the labels of its day and stays as it was. What else reads the labels,
run again on the same inputs with programs built from one commit (with the
labels before, each gave the tables and files on record again):

| Bench | Before | Corrected |
|---|---|---|
| Scenes (`bench/scenes`), a character's scenes: DINOv2-small, 40 s, own best threshold | 0.931 | 0.934 |
| ... step A, 40 s | 0.892 | 0.894 |
| ... the four-title table's row: own best, one threshold for all (0.675), the others' | 0.930, 0.917, 0.917 | 0.935, 0.921, 0.921 |
| ... the app's own pictures at 0.675 | 0.931 | 0.936 |
| ... the labelled scenes with the stored cast | 0.994 | 1.000 |
| ... each half with the other half's threshold | 0.839, 0.912 | 0.932, 0.915 |
| Conversations (`bench/scenes`): the two readings with the stored cast | | no number changes |
| ... the labels for both: the walk's added shots with a speaker, speaker shots held | 0.986, 0.788 | 0.986, 0.782 |
| Bodies, the test set (`bench/bodies`): Breaking Bad's pools M and H | 28, 59 | 29, 58 (one unanswered query) |
| Rescan (below): faces on the wrong character that went unnoticed | 17 | 12 |

The first half of the episode read 0.839 because the passer-by made Jesse a
character of a half he is not in, with no scene found. No gate, bar or
threshold that was decided changes, and the shared scenes threshold stays
0.675. The numbers that rest on the app's cast rose. Those that take the
labels for the cast as well moved a little both ways: the conversation
walk's held share 0.788 to 0.782, a character's scenes by the true cast
0.929 to 0.928 at the first line's threshold. The body bench's draft gate,
which is not decided, reads 0.21 for 0.20 on one of its lines (107 of 521
for 107 of 522).

A second correction the same day (row 31), on the owner's word again. It
started from the two things the reviewer of the first had found, the row at
the door and five gas-mask rows, and takes in what a look at their scenes
turned up.

- The `skyler` row at the door ended on Walter's face. Its last two boxes
  (15570 and 15573, during the kiss) are a row of their own now, `walter`
  (15570–15576), and the `skyler` row ends at 15570 with its six boxes. The
  `?` at 15642 in the same shot is his face too, as he turns: `walter`. A
  pin of the owner in the project has that track on a character of Walter's
  faces.
- Seventeen rows of faces behind a gas mask were `?` and are `x`, as the
  rule above says: four in the cold open (669–1008), six in the cook
  (60843–61177) and seven in the RV as it is driven off (73857–74510), the
  scene the cold open shows first. They are Walter's and Jesse's. Every `?`
  and `x` row of the cold open and of the last third of the episode was
  looked at; no row with a name there is a masked face.

No box changed. The file has 1,415 rows now, one more than the pass of
2026-10-14 had tracks. With the app's face pass on the proxy as above: the
main cast's P, R and F are the same to three decimals, and `stress` is the
same. `x` leaves out 18 tracks for 9. Of the labelled faces of the main cast
0.955 are on their character for 0.956, a step of rounding: the face at
15642 is one more labelled face of Walter's, and that pass has no track on
it, nor on the two boxes at the kiss.

What else reads the labels: no number of the scenes tables moves. The
conversation bench asks one speaker shot less of Walter, whom the shot at
the door shows now: 925 for 926 over all characters. Eight bodies under
masked faces leave the body bench's pool of extras, four of Walter's and
four of Jesse's (159 for 167 on Breaking Bad). Rescan counts 100 strangers
in Everyone else for 106. (Row 30 said of five of these rows that as `x`
they change no score. That held for this bench's scores only.)

Left for the owner, seen in passing and not changed: at least thirteen `?`
rows of the last third are faces of the main cast, and not all of them are
hard to see. Emilio from the front at 70020–70035 and in the beanie at
68436–68585; Walter at the RV at 59301–59325, at 59778–59814 and at
78537–78543; faces in bed at 81159–82050. The owner's pins in the project
name some of them already. Where the pass puts such a face on its
character, the bench counts a stranger on that character. Apart from the
cold open, the first two thirds of the file were not looked at this way.
(They were, in the read that follows.)

All `?` rows read again (2026-11-15; rows 32 and 34 of
`bench/models/DECISIONS.md`). Rows 30 and 31 had found faces of the main
cast under `?`, so every `?` row was read: 322 rows. Two readers each saw
one sheet per row (the whole frame with the box at the first and the last
sighting, the box enlarged at up to four) and cast pages of the 42 names
with three rows or more. They saw nothing of the labels, of the app's cast
or of each other. A third read the 48 rows on which the two differed (43 `?`
rows and 5 controls). Sixty rows that have a name were mixed in as controls.
The readers are a model (Claude Opus); no person has checked them.
`bb_s01e01.read.tsv` holds every answer, what the rule made of it and what
was written.

| | Rows |
|---|---|
| `?` rows read | 322 |
| both readers the same label | 87: 58 of the main cast, 29 of other names |
| both `e` (a person on no cast page), both `n` (no real face), both `x` (cannot tell) | 49, 31, 112 |
| the two differ | 43; in 5 the third gave a label that one of them had given |
| a label of the main cast by the rule of row 32 | 59 |
| ... written into the file | 37: Walter 23, Walt Jr. 6, Jesse 3, Emilio 2, Gomez 2, Skyler 1 |
| ... kept `?` by the author | 22: the face cannot be read, or the box lies on two heads, and only the place and the clothes tell (16); a reader had marked the name a guess (5); the author is not sure of the man (1) |
| a label outside the main cast by the rule, not written | 33 |

The author's line. A row is named when the face can be read at one sighting
at least, or when it is the same face three frames from such a sighting in
the row beside it. It is also named when the head is seen near and sharp
but turned away, in a shot where it can be no one else: hair, ear and jaw
are seen, the face is not. A row keeps `?` when the head is covered (a
hood, a beanie, an arm), dark, blurred or far, so that only the place and
the clothes tell, and when the box lies on two heads.

Eight of the 37 are named with the face turned away: all six of Walt Jr.
(8859–8868, 16473–16497, 17259–17277, 17271–17280, 17424–17460,
54219–54234) and two of Walter (54312–54315, 59778–59814). The file
already names such rows (Walt Jr. in the striped shirt).

The line is a judgement, and two reviews moved rows across it. The first
found seven named rows that it keeps: six faces that cannot be read and one
box that lies mostly on another head. The second found three more, of
Emilio, called a fourth borderline, and found two kept rows of Jesse whose
profile can be read; the author took back all four and one more box on two
heads. The numbers here are of the state after both. A third review then
checked the counts and numbers of that state and judged no face again.
With the eight turned-away rows kept as `?` too, the main cast would read
R 0.896 and F 0.944 per track, for the 0.893 and 0.943 below.

One of the new names makes a second row of one name in a frame: Walt Jr. at
17271, where the same head was found twice. The file has that kind already
(11 such pairs for 10).

On the 60 controls (20 of them Walter) no reader gave a wrong label: 109 of
their 120 answers were the row's label and 11 were `x`. By the rule 53
controls get their label and 7 none, six of the seven with a face lower than
the median `?` face. So the read errs towards leaving a face unnamed, not
towards a wrong name. How often it would name a stranger is not measured:
every control has a name.

The app's face pass on the proxy as above, with the labels before and after
the read:

| Labels | main cast per track P / R / F | screen time P / R / F | everyone named, per track P / R / F | in a track | on their character | over time: in a track, on their character |
|---|---|---|---|---|---|---|
| before the read | 0.996 / 0.909 / 0.951 | 1.000 / 0.961 / 0.980 | 0.994 / 0.900 / 0.945 | 0.970 | 0.955 | 0.962, 0.942 |
| after | 0.998 / 0.893 / 0.943 | 1.000 / 0.955 / 0.977 | 0.995 / 0.887 / 0.938 | 0.968 | 0.948 | 0.960, 0.934 |

Recall falls and precision rises a little. The 37 rows add eight scored
tracks of the main cast, and the pass has one of them on its character
(Gomez's, which is the rise in precision): the faces that got a name are
mostly hard ones, in profile, dim, small or turned away. The bench was
kinder to the pass on recall than it is now, and it is still kind, as the
list below shows. `stress` gives the same 27 runs. Numbers of this file
dated before the read stay as they were measured.

What else reads the labels, before and after the read:

- Scenes (`bench/scenes`): no number of that file's scenes tables moves,
  and the four-title table is the same. In the bench's own output "every
  shot its own scene" by the true cast reads 0.447 for 0.444.
- Conversations (`bench/scenes`): 921 speaker shots are asked for over all
  characters, for 925, and where a cell of that bench's tables moves, it
  moves in the third decimal; the check of that bench's readers finds 322
  of their 334 names in the labels, which give 421 (before 321 and 419).
  The conversation sections, their file of counts and row 29's result cell
  were made again with these labels in `d3ed173c`, with the same numbers.
- Bodies (`bench/bodies`): four bodies leave the pool of extras (155 for 159
  on Breaking Bad). Two of them had been named Walter by the pass and
  counted as wrong; they are Walter's.
- Rescan (below): of the faces in Everyone else 17 are people of the cast
  and 99 strangers, for 16 and 100; of the unsure ones 7 and 24, for 6 and
  25; 9 faces on the wrong character went unnoticed, for 12.

Left for the owner:

- The 22 rows the readers named and the author kept as `?`: frames
  7989–7998 and 54279–54282 as Walt Jr.; 9957–9990, 10008–10032,
  15087–15090, 16383–16389, 17754–17760, 28032–28038, 34623–34626,
  37335–37338, 49323–49326, 59553–59556, 59562–59565 and 73305–73308 as
  Walter; 16335–16338 and 16389–16392 as Skyler; 16797–16800 as Hank;
  68436–68439, 68541–68544, 68562–68585 and 68571–68574 as Emilio (the man
  in the beanie at the car: the scene tells who he is, the face does not);
  16938–16979 as Gomez (a clear profile; a pin of the owner has it as a
  character of its own). By the rule at the top of this file a face that
  cannot be told is `x`.
- The eight rows named with the face turned away (above), if such rows
  should rather be `x`.
- Ten rows to which one reader gave a name of the main cast and the third
  did not: 7047–7109, 30630–30636, 30720–30729, 81984–82008 and
  82041–82050 as Walter;
  34605–34608 as Gomez; 39660–39663, 41019–41028 and 41910–41913 as Jesse;
  55932–55947 as Walt Jr. The first reviewer of this read calls the two at
  the car wash (30630, 30720), the two in bed (81984, 82041) and the Gomez
  row plain faces of theirs.
- The 33 rows on which the readers agree on a name outside the main cast
  (`store_bully` 6, `chad` 5, `beard_glasses` 4, `suit_man` 3, and ten
  names with one or two).
- The 112 rows both readers could not tell, which the file still calls `?`:
  where the pass puts one of them on a character, the bench counts a
  stranger on that character. 64 of them are from the raid (34233–36099):
  faces behind gas masks in 43 by the readers' notes, the rest helmets from
  behind, a face in a mirror and drivers behind glass. The second reviewer
  sees Walter in two of the 64 (34317, 34320) and Emilio in one
  (35463–35503), none to be told by the face. By the rule at the top such
  rows are `x`. This read gave out no `x`.
- Rows that have a name were not read (but for the 60 controls), so a wrong
  name like the passer-by of the first correction can still be in the file.

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
one figure parted wrongly. The share has little room: the selves are 0.54 to
0.68 of their own cohesion alike on the tuning episodes, the least alike
halves of one figure that the grouping joins 0.73. And the rule needs a
fitted space, which needs
200 pairs of clear faces seen in one frame: two of fifteen episodes alone and
all nine other 3D films tried have fewer, so the rule is not on for them (the
one episode among them that joins the selves keeps them joined).
`bench/3d/README.md` has the tables and what was laid down before the series
was grouped.

`identity_bench` takes `TIGHT=<share>` (the rule from that share on, in every
space; 0: nowhere) and `TIGHT_TRACKS=<n>`. Unit tests in `as-identity` cover
it; twelve ways of breaking the rule each fail one.

## A merge by hand holds

Merging character B into character A pins B's faces to A (`as-db`
`merge_characters`). Until 2026-11-14 that was all, and A's own faces stayed
as the grouping found them. When the cast was grouped again (an episode
added, a newer version of the grouping), B's pinned faces were one group and
held A, and A's own faces were mostly too unlike them to join that group:
had the two been alike, the grouping would have made one character of them,
and nobody would have merged them by hand. A's own faces then went on as a
new character, and A's name stayed with the faces that had been merged in.
Seen in the running app (three episodes of the 3D series, footage type
animation; the code is the same for every kind of footage): a character
named after one girl, a boy's character merged into it, the cast grouped
again: the name was on the boy's 119 faces and four of hers, and her other
112 faces were a new, unnamed character.

Three things hold a merge now.

**The cast remembers it by pins on both sides.** A merge also pins a few
typical faces of the character that stays (`NAME_ANCHORS`, as naming a
character pins them): A's own faces are then held by the user's word like
B's.

**Pinned faces start with their like, and end as one character.** The
grouping used to start with all faces pinned to one character as a single
group. Of two unlike people that group is like neither (forty faces of one
and five of the other look like the forty), so the unpinned faces of the
smaller side could not join it. Now a pinned face starts with the group of
its character's pinned faces it is most like, when it is like that group as
two groups must be alike to join: on average over the link bar and, where
two tight groups stay two, not that much less alike than the group is in
itself (`ahc::initial_groups`; that second part has no floor of tracks
there, so that the five faces pinned of a heroine do not start among the
sixty merged-in faces of her masked self, which they are about as like as
the bar asks). Each such group gathers its own person's faces, and after the
clustering the groups pinned to one character become one (`pins_together`
in `as_identity::identify`).

**A character without pins of its own keeps its own faces**
(`keep_own_faces`): for merges made before, and for a name whose pinned
faces are in the smaller part of its character. The unpinned group that
holds most of what a character held besides the pins (over half, by
quality), was that character more than any other and holds as many of its
earlier faces as make a character, goes on as that character together with
the group pinned to it.

What it does not do, and what stays as it was:

- **One face moved to another character moves alone.** It groups with its
  own person, and that group is then pinned to the character it was moved
  to. Before, the pin carried the whole group over. Now a pinned group that
  is not the character's own (it holds no more than half of what the
  character held besides the pins) lets go of what it swept up of another
  character, when that outweighs its pins and the character's earlier faces
  in it; what it lets go of is a group of its own again. So does a new
  character the user made of a few faces of another.
- **Characters that turn out to be one person still become one.** A group of
  pinned faces that fewer faces of another character join keeps them, and a
  named character's own group keeps a bigger unnamed one that joins it.
- **A part of a named character that is not most of it can leave.** A named
  character (a few of its typical faces are pinned) whose faces a better
  grouping parts in two keeps the part its pinned faces are in; the other
  part goes on as a new character. So a wrong join inside a named character
  can still come apart, and so do a hero and the everyday self that an
  older grouping had put on one named character. If the pinned faces are in
  the smaller part, the bigger one is most of the character and the two
  stay one.
- **A group that was mostly another character does not follow** a pin, and a
  handful of stray faces is not a character's own faces.

On the real tracks of the two tuning episodes (3D, the rule on; scratch
`E:\as-ownmodels\3d\glue`, `real5.rs` and `real5b.log`):

| What was done | Holds |
|---|---|
| S01E11 grouped; one of its seven biggest characters merged into another (42 ways); S05E21 added; grouped three times: both sides keep 0.8 of their faces on the character | 42 of 42 with the pins a merge makes now; 41 of 42 for a merge made before (the bigger self merged into the smaller: its own 55 faces are a new character from the second grouping on) |
| Both episodes; one of the nine biggest merged into another (72 ways), grouped twice: both keep 0.9 | 72 of 72 (before 2026-11-14: none) |
| ... one face of one moved to another that nobody named (72 ways), grouped three times: both keep 0.9 of their own under their own characters, the moved face where it was put | 72 of 72 |
| ... the same, to a named character | 72 of 72 |

In the running app (scratch library, the same two episodes): the bigger self
merged into the smaller pins 157 faces (152 merged in, five of the other's
own) and is one character after two more groupings; a face moved to a
character nobody named, of the other self or of an unlike character, moves
alone.

Limits that stay:

- **A merge made before 2026-11-14 into a character without a name** has no
  pins on that character's own faces. It holds by `keep_own_faces` alone,
  and that lets go when later episodes bring so many faces of the merged-in
  person that the character's own group is no longer most of what it held
  (the 1 of 42 above). Naming the character, or merging something into it
  again, pins its own faces.
- Own faces that group in several parts without a pin in them, none of them
  over half: none follows.
- A merge cannot be taken apart again short of moving the faces back.
- A long series with a merge of two big characters starts its grouping with
  more pinned groups than before (one per look of each side, not one in
  all); how that weighs on a cast of many thousand pinned faces was not
  timed.

Unit tests in `as-identity`: a merge of two unlike characters in three sizes,
with new episodes and a second grouping after it; a merge whose merged-in
side outgrows the other over three episodes, of two unlike people and of the
two selves; faces of two people all pinned to one character; characters that
turn out to be one person; a new character made of a few faces of another; a
named character that lets go; a moved face in three sizes over three
groupings, and one to ten faces moved to a named character; a stray face;
what a character held, weighed by its clear faces; how pinned faces start.
In `as-project`: the bigger of two characters merged into the smaller, then
two episodes with far more of the merged-in one (it fails on the code
before). Twenty ways of breaking the three parts each fail a test.

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
half the time. Those stay suggestions for the person. (With the labels as
corrected on 2026-11-15 the 17 are 12: five were slips of the labels, see
"Label corrections" above; 9 since the `?` rows were read again.)

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
of 106 in Everyone else gets a guess, and none a clear answer. (Of 100 since
2026-11-15: six of the 106 were faces behind gas masks, which the labels
now leave out. Of 99 since the `?` rows were read again: one more is a
person of the cast.)

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
