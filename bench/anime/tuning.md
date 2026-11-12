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
