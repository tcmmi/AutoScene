# Training AutoScene's own identity embedders (dev only)

Nothing here ships. The rules — slots, test sets, data, gates, caps — are in
`bench/models/README.md`; what was decided and measured is in
`bench/models/DECISIONS.md`. This folder holds the tools.

Run everything with the environment on a drive with room (the CUDA build of
torch alone is about 3 GB):

```bash
export UV_CACHE_DIR=F:/as-ownmodels/uv-cache UV_PROJECT_ENVIRONMENT=F:/as-ownmodels/venv/train
export HF_HOME=E:/as-ownmodels/hf HF_HUB_DISABLE_XET=1   # DINOv2's weights, fetched once by timm
uv run <script> …
```

Work files (crops, runs, scores) go to `E:/as-ownmodels/train` (`TRAIN_WORK`).

## Scoring an embedder without a pass of its own

An embedder is judged on the tracks the Standard anime pass found on the
tuning episodes, with its vectors in place of the pass's (`EMB_OVERRIDE` of
the identity bench). That needs the heads the Best scan would read:

1. `anime_crops` (an example of `as-analysis`) decodes an episode again and
   writes each track's clearest heads as the app crops them:

   ```bash
   cargo run --release -p as-analysis --example anime_crops -- <ffmpeg_dir> <bench inputs of the episode> <episode>.std3.tracks $TRAIN_WORK/crops/<episode>.h4
   ```

2. `embed.py <model.onnx | checkpoint.pt> <out dir>` embeds them, project by
   project in each project's own order of episodes, and makes each track's
   vector as the Best scan does (its heads' unit vectors, weighted by their
   quality): `<out dir>/<project>.emb`.
3. `score.py <name> <out dir>` groups and scores the three tuning projects
   as `bench/anime/tuning.md` does (figure level, main cast from 15 s) and
   prints precision, recall and F per project and their mean; `--against
   <name>` compares two results, paired. The scorer sees no setting of the
   shell but the ones given on the command line.

The check that this path is faithful (the dry-run gate): Berkeley's graph run
through it must score as the real Best pass does. It does — see row 13.

## Recipe A, first stage: ArcFace on iCartoonFace's identities

After Bamman et al. 2026 (code MIT, DINOv2 Apache-2.0).

```bash
uv run heads.py <personai_icartoonface_rectrain.zip> <anime head detector.onnx> $TRAIN_WORK/heads.tsv
B=../../bench/models
uv run train.py --zip <personai_icartoonface_rectrain.zip> --out <run dir> --arch vits14 --heads $TRAIN_WORK/heads.tsv \
  --exclude $B/icartoonface.exclude.tsv,$B/icartoonface.mixed.tsv --drop $B/icartoonface.drop.tsv
uv run export.py <run dir>/best.pt <run dir>/model.onnx $TRAIN_WORK/crops/<episode>.h4
```

- **Crops as the app's.** iCartoonFace boxes the face; the app crops the
  head its detector finds, which is about 1.6 times as tall and 1.8 times as
  wide. `heads.py` runs the app's head detector on every picture, and with
  `--heads` a training crop is that head box padded by a quarter and
  squeezed to 224 × 224, as `as_vision::berkeley::crop` does (where no head
  is found: the face box widened as heads are on average). On top of the
  reference's changes (a part of the crop, mirrored, other colours) a share
  is cut to a band, as a close-up's head is by the frame (`--cut`), and a
  share is shrunk to 20–112 pixels, half of those through a JPEG, as a small
  head in a video is (`--shrink`).
- `train.py`: about a tenth of the identities is held out to pick the
  checkpoint (Rank@1 among the held-out faces); which side an identity is on
  follows from its name and the seed alone. A run saves every
  `--ckpt-minutes`, stops by itself `--hours` after each start, and
  `--resume` goes on with the same order of pictures and only with the
  settings it began with (the random changes to a picture are not replayed).
  `--limit-identities 100` is the dry run (2 % of the data, minutes on the
  development PC). `manifest.json` says what ran, `identities.txt` on whom.
- The embedding's size must be one no embedder of the app's tracks has (128,
  512, 768, 1024): the small backbone's own 384, `--dim 640` for the base
  one.
- `export.py`: opset 17, any batch, 32-bit floats; refuses a graph whose
  vectors are less than 0.99999 (processor) or 0.999 (DirectML) like the
  network's. The app has no loader for it yet.

Not built yet: the student for Fast Scan, the real-life adapter.

## Recipe A, second stage: triplets from tracks of real video

No names are needed: two heads of one track are one character, a head of a
track seen beside it is another. The footage is run through the Standard
anime pass (`bench_inputs`, `shot_signals`, `fuse_signals --anime`,
`anime_run`), then `anime_crops` with `HEADS=6 APART=<a second in frames>`
writes each track's heads and the tracks seen together. Per title, in one
folder: `in/<title>/bb.*`, `crops/<title>.h6.{crops,crops.tsv,pairs.tsv}`.

```bash
B=../../bench/models
uv run triplets.py build <tracks dir> <pairs dir> --titles <order.tsv> --first 400 --fuse <fuse_signals.exe> --drop $B/stage2.drop.tsv
uv run triplets.py look <tracks dir> <pairs dir> <sheets dir>
uv run triplets.py leaks <tracks dir> <pairs dir> <first stage>/last.pt <leaks.tsv> jjk=<emb dir>/jjk.emb,<names.tsv> …
uv run triplets.py sheets <tracks dir> <pairs dir> <sheets dir> <key.tsv>
uv run triplets.py audit <key.tsv> <reader A's answers> <reader B's answers> [<a third reader's>]
uv run train_tracks.py --from <first stage>/last.pt --tracks <tracks dir> --pairs <pairs dir> --out <run dir>
```

- `build` takes the titles a list names (`n id seconds sha256_of_id channel
  title`, the first `--first` of them) and applies the rules of
  `bench/models/README.md` and row 19 of `DECISIONS.md` (which also says why
  each is there): a positive is at least a second apart, not one held
  drawing, with no shot change between its two heads and no slip onto
  another drawing; a negative was seen beside its anchor in at least two
  samples, is no second box on the same head and not alike to the pass's
  own embedder. A tenth of the shows (a key made from a title's name) is
  held out. `--drop` names the titles that never train.
- `look` lays eight heads of every title on a row: the titles that are not
  drawn animation go on the drop list. `leaks` looks for bench characters
  in the footage by their looks; a title that shows one goes there too.
  After either, `build` again: the tables a run draws from are named by
  their hashes in its `manifest.json`, and a run refuses to go on with
  others.
- `sheets` and `audit` are the README's check of 300 pairs, drawn as
  training draws them; the key stays where the readers don't look.
- `train_tracks.py` is one pass over a fixed draw of triplets from the first
  stage's weights. `--identity 0.02` keeps the first stage's task beside
  them (the weight is small for a reason: see the script). `dev.tsv` holds
  the held-out triplets ranked right and clear of the margin; `last.pt` goes
  to `embed.py`, `sweep.py` and `export.py` as a first-stage checkpoint
  does.

## Before the data trains anything

- **Bench franchises** (`audit.py names`, `leaks.py`): their characters
  inside iCartoonFace, by name and by how they look (CCIP's nearest
  neighbours of the bench's labelled tracks; it separates poorly, the names
  carry the list). The result is `bench/models/icartoonface.exclude.tsv`.
- **Wrong labels** (`audit.py sheets`): 300 pictures drawn at random, each
  beside six others of its identity; the answers are
  `bench/models/icartoonface.audit.tsv`.
- **Identities that hold several characters** (`mixed.py`): two short runs
  with `train.py --fold 0/2` and `--fold 1/2` each train on half of the
  identities; `mixed.py embed` gives every picture the vector of the run
  that never saw its identity, `mixed.py find` groups each identity's
  pictures, ranks the identities by how alike their pictures are and cuts
  each in two halves, and `mixed.py sheets` lays ten pictures of each out to
  be read. The vectors alone do not settle it (look-alikes stay alike to a
  three-epoch model); the readers' answers are
  `bench/models/icartoonface.reading.tsv`, the identities at least two of
  them call mixed `icartoonface.mixed.tsv`, and the single pictures far
  from the rest of their identity `icartoonface.drop.tsv`.
