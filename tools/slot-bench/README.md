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
uv run shot_embed.py <ffmpeg> <video> <project_copy_dir> <dinov2-small model.onnx> <resnet50_places365.pth.tar> <out_prefix>
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
