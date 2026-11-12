# Own models (pre-registration)

How a recognition model AutoScene trains itself is chosen, judged and
released. Everything in this file is fixed **before** the first training run;
a change after that is a new row in `DECISIONS.md` (this folder) with its
reason. Media and training data are not in the repository; labels, rules and
result tables are.

The owner's decisions (2026-11-11): six identity slots — the normal scan and
Fast Scan for real life, anime and 3D animation; the models keep getting
better through retrain cycles and gated releases (nothing new learns on a
user's PC, nothing is uploaded); real life in the normal scan keeps
`glintr100` and gets one adapter experiment; a rented graphics card may cost
up to $300 a month.

## Slots

| Slot | Runs today | What training would make | Trained only if |
|---|---|---|---|
| Real life, normal | ArcFace R100 (`glintr100`) | a low-resolution adapter merged into `glintr100` | the adapter gate below holds |
| Real life, Fast | SFace | a small student of `glintr100` | the Fast Scan oracle shows a gain, and no ready-made model reaches it |
| Anime, normal | CCIP b36, Berkeley ViT-L as Best | DINOv2-S/14 or -B/14 in Berkeley's place | Best beats Standard on the tuning set (then a replacement for size and licence is worth it); to aim higher than Best, the anime pass must also be embedder-bound |
| Anime, Fast | CCIP b36 on Fast Scan's frames | a small student in CCIP's place | the anime normal model shipped |
| 3D, normal | `glintr100` + the per-title fitted space | the anime recipe on cartoon and CGI data | a labelled 3D set exists and the pass is embedder-bound |
| 3D, Fast | SFace | a small student of the 3D model | the Fast Scan oracle shows a gain on 3D |

Not trained: a body model (the bodies bench holds 84 queries, too few to tell
models apart; ready-made ones come first, on at least 3 titles and 300
queries).

Two slots that are not about identity were added on 2026-11-12; rows 5 and 6
of `DECISIONS.md` hold their labels, checks and gates:

- **Expressions**: what a face shows (smiling, laughing, crying and so on),
  one score per label, for real life, anime and 3D animation.
- **Scenes**: which shots belong together, so that a character's scenes
  include the shots in which the character is not to be seen.

Both test what exists before anything is trained. The rules of this file
apply to them where they fit: the titles of a check set never train, sources
are listed before a download, and the caps hold.

## Before any training: is the embedder the limit?

Free, on the development PC. A slot whose check fails is closed with a
`DECISIONS.md` row.

1. **Fast Scan oracle.** `fast_run` with `EMBEDDER=<file>`: `glintr100`
   first (the strongest embedder there is here), then ready-made small ones
   (`w600k_mbf`, `w600k_r50`). Scored with `GRID=6,8` against the labels,
   each embedder with its own norms (`identity_bench norms`). The slot stays
   open when "on their character" rises by at least 0.02 with 0 merged in
   `stress`; a ready-made model that reaches it within +15 % of the pass's
   time on a processor ships without any training.
2. **Oracle ladder** per footage type (`bench/anime/README.md`): ground-truth
   boxes, then ground-truth tracks, then label-oracle embeddings. A pass is
   embedder-bound when the embedder oracle closes at least two thirds of the
   gap to a perfect score.
3. **Anime, Best against Standard**: `anime_run` with and without
   `BERKELEY=`, then `identity_bench score` and `paired` on the tuning
   franchises. Best must lead by at least 0.02 F with a bootstrap lower bound
   above 0; otherwise there is nothing worth replacing.

## Test sets

- Per footage type: 6 tuning and 6 sealed franchises, 2 episodes each, at
  least 6 main figures. That resolves about 0.03 F; 0.02 takes about 15
  franchises. Until a type has them, its results are reported as provisional.
- A franchise has one role for good: training, tuning or sealed. All its
  episodes share it.
- Half of the sealed franchises first aired after the public training sets
  were collected (iCartoonFace 2019, `character_similarity` 2023-11).
- Sealed media: nothing runs on them before their one test — no detection, no
  error analysis, no app checks. Two labellers and an adjudicator, then an
  audit of 150 items. Every file is named by its SHA-256 in `media.tsv`.
- Label sheets are ordered by time (`ORDER=time`), never by a clustering of
  the embedder under test.
- Labels live in `bench/`, never in a scratch folder.

## Training data

- Sources and their terms are listed in `DECISIONS.md` before they are
  downloaded; the owner approves every source, and every online title.
- Every tuning and sealed franchise is removed from the training data, by
  name and by nearest neighbour against the bench galleries. Gate: 0 leaked
  identities, and at most 2 % wrong labels among 300 audited crops.
- Track triplets: the positive is the same track at least 1 s apart (held
  frames removed) or a pin of the owner's; the negative a face seen in the
  same frame, never a second box on the same head, a mirror image, a clone, an
  opening or ending, or a pair CCIP calls alike (cosine above 0.5). Gate: at
  most 2 % wrong among 300 audited pairs.
- No output of a model without a licence is a training target.
- Crops of real people's faces stay on the development PC or a data-centre
  host, never on a peer-hosted machine. No token is put on a rented machine.

## Gates

| Step | Rule |
|---|---|
| Dry run | 2 % of the data on the development PC through export, parity and `EMB_OVERRIDE`; a known embedder's vectors scored offline equal its real pass within 0.005 F |
| Kill gate (first paid run, at most $10) | on tuning: at least the model in the slot + 0.02 F with a bootstrap lower bound above 0, or — a replacement for size or licence — no more than 0.03 below it; otherwise a "not adopted" row |
| Triplets | at least +0.01 F on held-out franchises over 3 seeds, the seeds' spread below the gain; otherwise the model before them ships |
| Export | fp32; cosine at least 0.99999 to PyTorch on the processor, at least 0.999 on DirectML; a vector size no other embedder uses; a goldens script in `tools/model-prep` |
| Refit | at most 200 configurations, listed before the run; the centre of a plateau; `stress` 0 merged; the pass within its time and memory budget |
| Sealed test | once, with the build that ships; a better model needs + 0.02 F with a lower bound above 0 and no title worse by 0.02; a replacement no more than 0.01 below, with precision at least 0.97 |
| Adapter (real life, normal) | sealed F + 0.01 with a lower bound above 0; clean crops' vectors at least 0.98 like the original's; `stress` 0 merged |

A gate that fails ends the run it guards; the result is written down either
way.

## Release cycles

- A slot is retrained when about 25 new titles have come in, or once a
  quarter, on all its data from the last checkpoint.
- Each cycle gets 2 fresh sealed franchises per footage type; after the
  decision they become tuning franchises.
- A release must pass the sealed-test gate. At most about 3 releases a year
  per slot: a new embedder makes every project find its faces again.
- Two cycles in a row without a release pause the slot.

## Caps

The first build-out costs at most $100 of rented time, a slot's first model at
most one week of work; a month never more than $300. A run that would pass a
cap is not started.
