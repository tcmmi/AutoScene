# slot-bench

Dev-only tools for the two slots that are not about identity
(`bench/models/DECISIONS.md`, rows 5 to 7): they run ready-made models over
frames of the bench media, so that what exists is tested before anything is
trained. Nothing here ships.

Python is managed by [uv](https://docs.astral.sh/uv/); dependencies are the
newest releases (pre-releases included), pinned exactly in `pyproject.toml`.
On the development PC the environment and uv's cache live on `F:`
(`UV_PROJECT_ENVIRONMENT`, `UV_CACHE_DIR`), since `C:` is short of room.

The model files are not in the repository; `bench/models/data.tsv` names each
with its size, SHA-256, source and terms. A file that is a pickle (`.pth`) is
loaded weights-only: plain tensors, no code.

## Scenes: `shot_embed.py`

Picture embeddings of three frames per shot (at 1/6, 1/2 and 5/6 of it), for
step B of the scenes bench (`bench/scenes/README.md`):

```sh
uv run shot_embed.py <ffmpeg> <video> <project_copy_dir> <media> <dinov2-small model.onnx> <resnet50_places365.pth.tar | -> <out_prefix>
```

The shots come from a COPY of a project; the video is decoded once, every
frame from frame 0, and the run stops when the number of frames is not the
project's. It writes `<out_prefix>.dino_cls.emb`, `.dino_patch.emb` (the mean
of DINOv2's patch tokens) and `.places.emb`, which `scenes_bench score` reads
with `EMB=<weight>:<file>`.

## Scenes: `shot_sheets.py`

Contact sheets of an episode's shots in time order, for labelling scenes
where no labels exist:

```sh
uv run shot_sheets.py <ffmpeg> <video> <project_copy_dir> <media> <out_dir>
```

One picture per shot (two for a shot of 8 s or more), thirty per sheet, each
with the shot's number, its start and its length, and a mark where black, a
fade or a dissolve lies before it. `shots.tsv` beside the sheets turns shot
numbers back into frames.

## Scenes: `scene_merge.py`, `scene_titles.py`

`scene_merge.py join <a.tsv> <b.tsv> <out_prefix>` joins two labellers' scene
starts into the starts both name and the starts only one names;
`scene_merge.py final <out_prefix> <shots.tsv> <scenes.tsv>` writes the label
file of `bench/scenes` (frames in place of shot numbers).

`scene_titles.py <scenes_bench.exe> <titles.tsv> [NAME=value …]` runs the
scenes bench over several episodes and says what one threshold for all of
them gives, and what each would get with the threshold of the others.

## Expressions: `sound_events.py`, `crop_sheets.py`

`sound_events.py <ffmpeg> <video> <yamnet.onnx> <yamnet_class_map.csv> <out.tsv>`
listens to a video's soundtrack with YAMNet: laughing, crying and shouting
every 0.48 s, the second vote of the expressions slot.

`crop_sheets.py <list.tsv> <out_dir>` makes contact sheets of face crops,
thirty per sheet with a number under each, for labelling what the faces show.
