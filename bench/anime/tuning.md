# Tuning franchises: track labels, and Best against Standard

Made on 2026-11-11 and 12 for the own-models plan (`bench/models/README.md`,
"Before any training", check 3). The decision is row 9 of `DECISIONS.md`.

**Provisional.** The labels were made by language-model labellers, not by
people, and the human audit of 150 items that freezes labels (`README.md`,
"Labels") has not been done. Three tuning franchises are labelled, not six.

## Files

| File | What |
|---|---|
| `jjk.labels.tsv`, `rezero.labels.tsv`, `onepiece.labels.tsv` | a name for every track of the anime pass: 1,515 + 1,147 + 2,038 = 4,700 tracks (`FORMATS.md`, "Track labels") |
| `<project>.labelling.tsv` | per label, in the same order: the first and the second labeller's answer and what reviewed it afterwards |
| `<project>.gallery.tsv` | the look codes: the galleries of 2026-10-30, plus `GARFIEL_BEAST` (Re:Zero) and `LUFFY_KID` (One Piece), added from the labellers' notes |
| `<project>.spans.tsv` | opening, ending, eyecatch, recap and preview of each episode, found by eye and set on shot cuts |

Each project is one timeline: a media's frames are offset by the frames of the
media before it.

| Project | Media (`media.tsv`) | Frames | Offset |
|---|---|---|---|
| `jjk` | `jjk_s2e03` | 34,407 | 0 |
| | `jjk_s2e04` | 34,402 | 34,407 |
| `rezero` | `rz_31` | 37,641 | 0 |
| | `rz_33` | 34,766 | 37,641 |
| `onepiece` | `op_6` | 34,304 | 0 |
| | `op_7` (25 fps; the others 23.976) | 35,769 | 34,304 |
| | `op_a` | 34,302 | 70,073 |

## How the labels were made

- **Tracks.** The anime pass as the app runs it (`anime_run` at `0ec1076e`
  with the app's models and inputs: `HEAD=anime_head_detect_v2m`,
  `PERSON=anime_person_detect_v1m`, `CCIP=anime_ccip_b36`,
  `HEAD_INPUT=640x640 PERSON_INPUT=960x544 CCIP_BATCH=4`; shots from
  `shot_signals` and `fuse_signals --anime`). These are not the tracks the
  README plans for track ground truth (the E1 winner at score ≥ 0.10, stride 1,
  linked by box only): the labels name the tracks the shipped pass makes and
  say nothing about heads it missed. The Best pass makes the same tracks, box
  for box; only their embeddings differ.
- **Sheets.** `ORDER=time identity_bench sheets`: 64 tracks a sheet, in the
  order they come on screen, never by a grouping. Per track the labeller saw
  its thumbnail, three moments (first, middle, last head) and the frame of the
  thumbnail with the head boxed. No labeller, adjudicator or reviewer saw a
  grouping or a score.
- **Labellers.** Language-model agents without any other context, each with
  the instructions, the gallery and its sheets. The first labeller named all
  78 sheets; the second, a random 23 of them (whole sheets, drawn per episode
  with a fixed seed until they held a quarter of its tracks). Part-way, the
  account's limit for the stronger model was reached: of the first labeller's
  tracks 2,494 are from the stronger model (Claude Fable 5.2) and 2,206 from
  Claude Opus 5.5; of the second labeller's, 320 and 1,108.
- **A rule corrected while labelling.** The first instructions made a box on
  less than half a head `n` (the keyframe class N). That turned eye close-ups
  and half heads of known characters into `n`, which the track codes do not
  say and which would punish a grouping that puts them on the right character.
  Corrected to: any part of a real character's head is that character, or `x`;
  `n` only for no head at all and for heads that are not characters in the
  scene. The labellers then at work went back over their `n` answers; the
  saved answers hold no such `n`.
- **Adjudication.** Where the two differed, a third agent decided from larger
  pages (five moments, the frame) with both answers shown in a random order.
  On five sheets of `op_7` a further labeller (the stronger model, 320 tracks)
  was compared with the names so far: 35 differed and were adjudicated the
  same way, 7 names changed.
- **Second look.** Every track the first labeller could not name (`x`, `?u`)
  outside the second labeller's sample went to another agent with the larger
  pages. The Opus labellers gave up more often: `x` on 6–10 % of their
  Jujutsu Kaisen tracks and 8–37 % of their One Piece tracks, against 0–2 %
  and 2–14 % for the stronger model.
- **Tracks that change character.** 40 named tracks flagged as jumping to
  another head were looked at over ten moments and given the character most of
  the track shows: 13 kept their name, 8 took another character's, 2 became
  `?e`, and 17 that were about half and half became `?u`. No track was split
  (`mix@`).
- **Added codes.** `?e` tracks whose notes say "young Luffy" or "Garfiel's
  tiger" became `LUFFY_KID` (9) and `GARFIEL_BEAST` (12). Other characters
  outside the galleries who come back (Tama, Franky, Brook, Megumi as a child,
  Beatrice, villagers, Geto's curses) stay `?e`: each track someone of their
  own.

| Episode | Tracks | Second labeller | Same answer | Both gave a code: the same | Adjudicated: first's answer / second's / neither | Second look at `x`, `?u`: named / `?e`, `n` / still open | Final: named / `?e` / `n` / `x`, `?u` |
|---|---|---|---|---|---|---|---|
| `jjk_s2e03` | 772 | 256 (33 %) | 225 (87.9 %) | 176 of 177 | 32: 21 / 9 / 2 | 19: 4 / 1 / 14 | 563 / 114 / 64 / 31 |
| `jjk_s2e04` | 743 | 192 (26 %) | 161 (83.9 %) | 147 of 149 | 31: 6 / 21 / 4 | 32: 14 / 4 / 14 | 552 / 124 / 45 / 22 |
| `rz_31` | 591 | 192 (32 %) | 180 (93.8 %) | 169 of 170 | 12: 7 / 3 / 2 | 13: 0 / 2 / 11 | 536 / 9 / 35 / 11 |
| `rz_33` | 556 | 192 (35 %) | 165 (85.9 %) | 120 of 121 | 27: 21 / 3 / 3 | 17: 6 / 1 / 10 | 448 / 72 / 20 / 16 |
| `op_6` | 663 | 192 (29 %) | 157 (81.8 %) | 90 of 90 | 35: 27 / 5 / 3 | 81: 18 / 12 / 51 | 412 / 123 / 75 / 53 |
| `op_7` | 788 | 212 (27 %) | 188 (88.7 %) | 88 of 89 | 24: 9 / 13 / 2 | 79: 12 / 17 / 50 | 300 / 350 / 80 / 58 |
| `op_a` | 587 | 192 (33 %) | 170 (88.5 %) | 123 of 124 | 22: 12 / 8 / 2 | 144: 31 / 35 / 78 | 393 / 85 / 31 / 78 |
| all | 4,700 | 1,428 (30 %) | 1,246 (87.3 %) | 913 of 920 (99.2 %) | 183: 103 / 62 / 18 | 385: 85 / 72 / 228 | 3,204 / 877 / 350 / 269 |

What the numbers say: two labellers who both name a character name the same
one 99.2 % of the time (7 of 920 differ). The 12.7 % of differing answers are
mostly about whether a head can be named at all and where `?e`, `n` and `x`
meet. The adjudicator left the first labeller's answer on 103 of 183 tracks:
about 5.6 % of the sampled tracks changed, and that share is the best guess for
the 70 % of tracks no second labeller saw. In all, 287 of the 4,700 final names
differ from the first labeller's answer.

Cross-check against the keyframe trial (`E:/as-anime/gt_trial`, two other
labellers on whole frames, 2026-10-30): 234 of its heads lie on a track; 223
carry the same code, none a different gallery code, 11 a code against `?e`,
`?u`, `n` or `x`.

Not done: the labeller calibration on AOT Characters for these labellers (the
gate was missed on 2026-10-30, `DECISIONS.md` row 3); shot presence (the main
cast comes from the labels' spans); the 150-item human audit.

## Best against Standard

Both passes on every tuning episode (`anime_run` without and with
`BERKELEY=`), each franchise scored as one project:

```
ANIME=1 GRID=6,10 MAIN_SECS=15 BOOT=2000 FPS=24000/1001 LEVEL=figure \
GALLERY=bench/anime/<p>.gallery.tsv SPANS=bench/anime/<p>.spans.tsv \
SEGMENTS=<p>.segments.tsv EPISODES=<offsets> ITEMS=<p>.<arm>.items.tsv \
identity_bench score <p>.<arm>.tracks bench/anime/<p>.labels.tsv
identity_bench paired <p>.std.items.tsv <p>.best.items.tsv
```

`GRID=6,10` is the anime pass's sample grid; both arms group with
`Params::anime()`, as the app does. The plan was written down before any label
or score existed: the number that decides is the main cast's per-track BCubed
F (figures with at least 15 s), Best − Standard, as the macro mean over the
franchises, with a paired bootstrap that redraws the main figures inside each
franchise (2,000 resamples).

Main cast, figure level, spans left out:

| Franchise | Scan | Tracks scored | Main figures | Characters | P | R | **F** | Screen-time F | Everyone named F | On their character | Merges to clean | Clip P / R |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Jujutsu Kaisen | Standard | 1,012 | 9 | 70 | 0.843 | 0.166 | **0.277** | 0.343 | 0.283 | 0.799 | 36 | 0.972 / 0.390 |
| | Best | | | 55 | 0.976 | 0.121 | **0.216** | 0.291 | 0.223 | 0.652 | 22 | 0.937 / 0.333 |
| Re:Zero | Standard | 1,046 | 14 | 47 | 0.899 | 0.321 | **0.473** | 0.537 | 0.471 | 0.883 | 26 | 0.985 / 0.554 |
| | Best | | | 44 | 0.993 | 0.423 | **0.593** | 0.668 | 0.583 | 0.868 | 15 | 0.993 / 0.712 |
| One Piece | Standard | 1,237 | 20 | 124 | 0.932 | 0.161 | **0.274** | 0.410 | 0.279 | 0.741 | 45 | 0.966 / 0.379 |
| | Best | | | 88 | 0.967 | 0.242 | **0.387** | 0.497 | 0.395 | 0.763 | 26 | 0.972 / 0.524 |
| Macro mean | Standard | | | | | | **0.341** | 0.430 | 0.344 | 0.808 | | |
| | Best | | | | | | **0.399** | 0.485 | 0.400 | 0.761 | | |

Best − Standard, with 95 % paired-bootstrap intervals:

| | Per-track F | 95 % | Screen-time F | 95 % | Clip precision | 95 % |
|---|---|---|---|---|---|---|
| Jujutsu Kaisen (9 figures) | −0.061 | [−0.149, +0.113] | −0.051 | [−0.160, +0.107] | −0.035 | [−0.100, +0.016] |
| Re:Zero (14) | +0.120 | [+0.005, +0.174] | +0.131 | [−0.031, +0.205] | +0.009 | [−0.005, +0.031] |
| One Piece (20) | +0.113 | [+0.080, +0.140] | +0.087 | [+0.048, +0.122] | +0.006 | [−0.026, +0.043] |
| **Macro mean** | **+0.057** | **[+0.009, +0.126]** | +0.056 | [−0.013, +0.120] | −0.007 | [−0.030, +0.017] |

How firm the macro number is: five other seeds put its lower bound between
+0.009 and +0.012, 20,000 resamples at +0.010. At the look level it is +0.059
[+0.011, +0.127], at the person level +0.050 [+0.000, +0.115], with the main
cast from 30 s +0.054 [+0.006, +0.115], with every named figure +0.057
[+0.005, +0.117]. The interval redraws figures, not franchises: it says nothing
about a fourth franchise, and one of the three goes the other way.

What stands out beside it:

- Both scans break a character into many: Standard makes 70, 47 and 124
  characters for 9, 14 and 20 main figures, Best 55, 44 and 88. Recall is
  0.16–0.32 with Standard and 0.12–0.42 with Best. What they do group is
  nearly pure (precision 0.84–0.93 Standard, 0.97–0.99 Best). The targets
  (Track-F ≥ 0.90, at most one merge to clean per main figure and episode) are
  far off for both.
- On Jujutsu Kaisen Best leaves 28–38 % of the four biggest figures' time in
  no character (Standard 4–23 %).

Looked at afterwards, not part of the plan:

- The embedders alone (`identity_bench separation`, named tracks, figure
  level): the share of same-figure pairs that pass where 1 % (0.1 %) of
  different-figure pairs do is 0.25 (0.04) for CCIP b36 and 0.38 (0.16) for
  Berkeley on Jujutsu Kaisen, 0.41 (0.14) and 0.69 (0.50) on Re:Zero, 0.25
  (0.11) and 0.38 (0.22) on One Piece. AUC: 0.869 / 0.842, 0.929 / 0.948,
  0.820 / 0.827.
- `ASSIGN` from 0.25 to 0.50 moves Best's F by at most 0.02 and Standard's not
  at all: the bar that decides is the link in the fitted space.

Not verified: pass times (other jobs ran beside the passes, so the two scans'
times cannot be compared); the three franchises as an app project with one
media per episode (the bench puts every track on one timeline with one media
id); the One Piece timeline's seconds (one of its three files is 25 fps,
scored at 23.976).

## Label oracle: what a perfect embedder would reach

The third rung of the oracle ladder (`README.md`), run on 2026-11-12 after the
comparison above; the plan was written down before its first score. Every
named track gets the unit vector of its figure, every other track (`?e`, `n`,
`x`, `?u`) one of its own, so the cosine is 1 inside a figure and 0 everywhere
else. The vectors go in as `EMB_OVERRIDE`; tracks, labels, bars and scoring are
those of the two passes. Passing a scan's own embeddings the same way gives its
score back to the last digit.

Main cast, figure level, per-track:

| | Jujutsu Kaisen | Re:Zero | One Piece | Macro F |
|---|---|---|---|---|
| Standard | P 0.843 R 0.166 F 0.277 | P 0.899 R 0.321 F 0.473 | P 0.932 R 0.161 F 0.274 | 0.341 |
| Best | P 0.976 R 0.121 F 0.216 | P 0.993 R 0.423 F 0.593 | P 0.967 R 0.242 F 0.387 | 0.399 |
| Perfect embeddings | P 1.000 R 0.687 F 0.815 | P 1.000 R 0.813 F 0.897 | P 1.000 R 0.685 F 0.813 | **0.842** |
| Characters (Standard / Best / perfect) | 70 / 55 / 17 | 47 / 44 / 17 | 124 / 88 / 31 | |
| Share of the gap to 1 closed, against Standard | 0.744 | 0.804 | 0.742 | **0.759**, 95 % [0.719, 0.812] |
| ... against Best | 0.764 | 0.747 | 0.695 | **0.736**, 95 % [0.697, 0.786] |

By the README's rule (two thirds or more) the pass is embedder-bound on these
franchises, against either scan. One Piece against Best is the weakest cell
(0.695, its own interval [0.591, 0.831]). The fitted space changes nothing here
(`ADAPT=0`: the same numbers); a vector per gallery code instead of per figure
gives 0.815, 0.897 and 0.794.

A perfect embedder still stops at 0.81–0.90, below the target of 0.90 on all
three franchises. Precision is 1.000; what is missing is recall, and the cause is in
the tracks: 263 of 1,115 named tracks on Jujutsu Kaisen, 209 of 984 on Re:Zero
and 255 of 1,105 on One Piece were found in a frame with another track of the
same figure. The grouping reads two tracks in one frame as two people, so the
figure gets a second character, and tracks that fit both end as unsure or
unassigned. Looked at on pages of twelve pairs per franchise: most are a second,
wide box of the pass on the same character beside its head box (the head a
person box implies); the rest are real doubles (the Ryuzu copies, the rabbits,
artwork in an opening).

Added after those scores were seen, not part of the plan:

- Without the doubles (of every such pair the track with fewer heads leaves;
  112–134 tracks a franchise, 7–10 %; the tracks left are scored against their
  labels): perfect embeddings reach 0.985, 0.996 and 0.988. Standard goes from
  0.277 / 0.473 / 0.274 to 0.368 / 0.601 / 0.316 (macro 0.341 → 0.428), Best
  from 0.216 / 0.593 / 0.387 to 0.339 / 0.751 / 0.521 (0.399 → 0.537); Best −
  Standard is then +0.108 [+0.049, +0.184], Jujutsu Kaisen still −0.029. These
  runs score fewer tracks than the passes, so they show the size of the effect,
  not a result to hold a scan to.
- Dropping the shared frames of alike tracks (`CL_CAP=0.9`) lifts the perfect
  oracle to 0.867, 0.907 and 0.884: the tracks' spans still overlap.
- With every figure's tracks moved onto their mean real embedding (no spread
  inside a figure, the embedder's own distances between figures): CCIP b36
  0.650 / 0.824 / 0.536 with precision 0.83 / 0.95 / 0.56, Berkeley 0.735 /
  0.832 / 0.657 with precision 0.99 / 0.98 / 0.91. CCIP's figures of One Piece
  lie too close to each other even then.
- A cosine mixed from the real one and the truth, (1 − a) · real + a · truth:
  macro F 0.575 / 0.656 / 0.660 for CCIP b36 at a = 0.25 / 0.5 / 0.75, 0.611 /
  0.727 / 0.705 for Berkeley. It does not rise evenly towards 0.842: the doubles
  count as strangers who look alike, and the bars move up with them.

So the gap of the shipped scans to a perfect score splits about three to one:
0.50 F (Standard) or 0.44 (Best) is the embedder, 0.16 is one figure found
twice in a frame. Not run: the first two rungs (ground-truth boxes and tracks);
the labels sit on the pass's own tracks. Like everything above, this rests on
labels a person has not audited yet.

## One figure, one head: the doubles taken out of the pass

The oracle above stops short because one figure is found twice in a frame.
The second box is nearly always the head a person box implies (its top 35 %)
beside the head the head detector found on the same figure: in a close-up the
head is far bigger than that top strip, and a figure reaching out or leaning
has its head at one side of its box, so neither of the two overlap rules
(`union_iou`, `union_cover`) took the boxes for one head.

How the rule was found. `anime_dets` writes what the two detectors find in
every sample before anything is joined; replaying the pass's own rule on that
gives the pass's heads back (99.97–100 % of the heads in tracks), so rules can
be tried without a pass. By the labels, the implied heads the pass kept were,
per frame: 1,436 a second box on a figure with another track in the frame,
2,015 the figure's only track there (what the person detector is for), 653
`n`, 1,190 other unnamed, 126 in no track. An own head's top lies within about
half its height of the person box's top; the head of someone in front of the
figure lies 1.2 heights or more below. The rule (`union_heads`,
`Params::union_reach` 1.0): a person on which a found head sits adds no implied
head. A head sits on a person when its centre lies in the person box, its top
at most one head height below the box's top, and it is at most 1.25 person
widths wide; a head about to be dropped as too small counts for no one; the
implied head of such a person still stands where a head is for the people
after it, so the rule only takes heads away. Replayed, it drops 1,340 of the
1,436 doubles (93 %; 94 / 95 / 91 % a franchise) and 94 of the 2,015 sole
heads (4.7 %), and adds none. The labels chose this rule, so they are tuning
data for it; no other title has seen it.

The check was written down before a pass with the rule had finished: both
scans run again on the seven episodes and scored against the same labels (the
scorer puts labels on tracks by frame and box). Main cast, figure level,
per-track; before → after:

| | Jujutsu Kaisen | Re:Zero | One Piece | Macro F | After − before, 95 % |
|---|---|---|---|---|---|
| Standard | P 0.843 R 0.166 F 0.277 → P 0.871 R 0.197 F 0.322 | P 0.899 R 0.321 F 0.473 → P 0.914 R 0.470 F 0.620 | P 0.932 R 0.161 F 0.274 → P 0.930 R 0.212 F 0.345 | 0.341 → 0.429 | +0.088 [+0.049, +0.118] |
| Best | P 0.976 R 0.121 F 0.216 → P 0.974 R 0.185 F 0.311 | P 0.993 R 0.423 F 0.593 → P 0.988 R 0.492 F 0.657 | P 0.967 R 0.242 F 0.387 → P 0.951 R 0.369 F 0.532 | 0.399 → 0.500 | +0.101 [+0.070, +0.130] |
| Perfect embeddings | 0.815 → 0.908 | 0.897 → 0.974 | 0.813 → 0.882 | 0.842 → 0.921 | |
| Tracks | 1,515 → 1,405 | 1,147 → 1,053 | 2,038 → 1,913 | | |
| Named tracks in a frame with another of their figure | 263 → 82 | 209 → 52 | 255 → 95 | | |
| Frames in which a figure had a track and has none now | 0.43 % | 0.15 % | 0.66 % | | |
| Characters, Standard / Best | 70 → 57 / 55 → 48 | 47 → 39 / 44 → 40 | 124 → 100 / 88 → 84 | | |
| Merges to clean, Standard / Best | 36 → 26 / 22 → 15 | 26 → 17 / 15 → 12 | 45 → 32 / 26 → 20 | | |
| Clips, Standard | P 0.972 → 0.938, R 0.390 → 0.417 | P 0.985 → 0.982, R 0.554 → 0.673 | P 0.966 → 0.948, R 0.379 → 0.427 | | |
| Clips, Best | P 0.937 → 0.954, R 0.333 → 0.403 | P 0.993 → 0.991, R 0.712 → 0.713 | P 0.972 → 0.953, R 0.524 → 0.605 | | |

The five conditions set beforehand:

1. Fewer doubles on every franchise: held.
2. Macro F of neither scan falls: held (both rise, intervals above 0).
3. Precision of neither scan falls by more than 0.01 on any franchise: **not
   held**. Best on One Piece falls from 0.967 to 0.951. Every other cell is
   within 0.005; Standard rises on two franchises.
4. At most 1 % of a figure's frames lose their track: held.
5. The oracle rises on every franchise: held.

What the lost precision is. It is not noise: leaving five unnamed tracks out
at random, twelve times, moves that precision by 0.005 at most. With the fix
Best puts Kozuki Toki together with eight tracks of her daughter as the oiran
(drawn alike; labelled `?e`) and one of another woman, and Nami's character
takes three unnamed tracks. The boxes are not at fault: the doubles put
cannot-links between a figure's own tracks, which kept every group small, the
wrong joins along with the right ones. Without them Best's bars (row 7: set on
one title, `copy_link` and the table never tuned for Berkeley) join more. The
same shows in `stress` over its 27 runs: with Standard both fall (people split
952 / 645 / 1,145 → 767 / 420 / 897, characters holding two people 204 / 207 /
189 → 118 / 156 / 127); with Best fewer people are split (1,017 / 638 / 949 →
843 / 564 / 597) but more characters hold two (7 / 10 / 15 → 28 / 13 / 27).

Beside it:

- Best − Standard after the fix: +0.071 [+0.024, +0.143] (row 9: +0.057
  [+0.009, +0.126]).
- The oracle's share of the gap after the fix: 0.862 [0.818, 0.919] against
  Standard, 0.843 [0.788, 0.905] against Best. What it still lacks to 1 is
  mostly two found heads of one figure in a frame: real doubles (copies of a
  character, mirrors, opening artwork), which no rule here should join.
- Fast Scan uses the same rule. One episode a franchise, before → after: F
  0.398 → 0.428, 0.513 → 0.579, 0.531 → 0.513 (each interval spans 0);
  precision 0.875 → 0.872, 0.872 → 0.871, 0.858 → 0.830; doubles 92 → 42, 71
  → 10, 61 → 24; 0.3 %, 1.2 % and 1.2 % of a figure's frames lose their track.
- The labels still hold a row for every box taken away, so "labelled faces in
  a track" falls from 1.000 to 0.937 / 0.970 / 0.953 by construction; of the
  faces in a track, as many are on their character as before (Standard 0.809 →
  0.805, Best 0.776 → 0.812, macro).
- Macro clip precision: Standard −0.018 [−0.040, +0.001], Best −0.001
  [−0.016, +0.014]; clip recall 0.441 → 0.506 and 0.523 → 0.574.

Not verified: any title outside these three (the labels of the title the bars
were set on are no longer on disk; test and reserve episodes were not
touched); the app itself (a project analysed again under `+anime3`); pass
times (other jobs shared the card).
