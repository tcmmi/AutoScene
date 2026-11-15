# slot-bench

Dev-only tools for the two slots that are not about identity
(`bench/models/DECISIONS.md`, rows 5 to 11): they run ready-made models over
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

## Conversations: `speak_sheets.py`, `speak_cast.py`, `speak_merge.py`

Labels of who is seen and who speaks in each shot, for an episode that has
neither identity nor speaker labels (`bench/scenes/README.md`,
"Conversations on a second title"; row 29):

```sh
uv run sound_events.py <ffmpeg> <video> <yamnet.onnx> <yamnet_class_map.csv> <sound.tsv>
uv run speak_sheets.py <ffmpeg> <video> <project_copy_dir> <media> <sound.tsv> <sheets_dir> [<scenes.tsv> odd]
uv run speak_cast.py project <ffmpeg> <video> <project_copy_dir> <media> <cast_dir>
uv run speak_cast.py sheet <cast_dir> <cast.tsv> <cast_prefix>
uv run speak_merge.py <sheets_dir>/rows.tsv <reader_a_dir> <reader_b_dir> <cast.tsv> <out.shots.tsv> [<a.tsv> <b.tsv>]
```

`speak_sheets.py` draws every shot as a row of six pictures with the seconds
of speech heard in it. `speak_cast.py project` cuts face pictures of the
stored characters with three face tracks or more (`labels` does the same for
the names of an identity label file); from them a table `label keys note` is
made by eye, and `sheet` draws the pages the readers get. Two readers, blind
to each other, follow `speak_reader.md`; `speak_merge.py` checks their files
and writes the shot label file that `conversation_bench shots` and
`conversation_bench agree` read.

## Expressions: `sound_events.py`, `crop_sheets.py`

`sound_events.py <ffmpeg> <video> <yamnet.onnx> <yamnet_class_map.csv> <out.tsv>`
listens to a video's soundtrack with YAMNet: laughing, crying and shouting
every 0.48 s, the second vote of the expressions slot.

`crop_sheets.py <list.tsv> <out_dir>` makes contact sheets of face crops,
thirty per sheet with a number under each, for labelling what the faces show.

## Expressions: the ready-made models against a check set

The steps of `bench/expressions/README.md`, in their order:

1. `face_crops.py <ffmpeg> <video> <project_copy_dir> <media> <out_dir>`
   cuts up to three face crops out of every face track of an episode (a
   copy of a project, never the library itself) and lists them in
   `crops.tsv`.
2. `expr_models.py [--top <dir>] [--emb <dir>] [--only <table>] <models_root> <crops_dir> <out_dir> [model …]`
   runs the ready-made models over the crops, one table of raw outputs per
   model; `--top` also draws the thirty highest crops of every output.
   `expr_scores.py` (no program of its own) turns those tables into one
   number per crop and output, and names the outputs that could stand for
   each label.
3. `expr_select.py <crops_dir> <scores_dir> <out_dir> [random_tracks] [seed]`
   draws the tracks to label: the highest of every candidate output, and a
   random draw of the rest with a weight, so that the labelled tracks stand
   for the whole episode. `crop_sheets.py` makes the sheets of its `list.tsv`.
4. `expr_check.py join <check_dir>` compares two labellers and lists what a
   third has to decide; `expr_check.py final <check_dir> <project_copy_dir>
   <out.tsv>` writes the label file of `bench/expressions`;
   `expr_check.py score <crops_dir> <scores_dir> <check.tsv> [<sound.tsv>
   <fps_num> <fps_den>]` rates every output against every label, per crop
   and per track, with the gate of `DECISIONS.md` row 5.
5. `expr_probe.py <name>=<crops_dir>,<scores_dir>,<check.tsv>[,<emb_dir>] …`
   trains a small head per label on the labelled crops and scores it on
   tracks it never saw: over the models' outputs, or (`EMB=<model>`) over
   the picture embeddings that `expr_models.py --emb <emb_dir> --only
   <check.tsv>` writes. A look at whether training is worth a try, not a
   model.
6. `expr_audit.py page <out_dir> <name>=<crops_dir>,<check.tsv> …` draws 100
   labelled crops and writes one page (`audit.html`, the crops inside it)
   on which a person says what each face shows, without seeing the labels;
   `expr_audit.py compare <answers.tsv>` sets the answers against the
   labels. The page holds faces: it stays on this computer.

`tokenizers` reads SigLIP 2's tokenizer (its newest pre-release cannot, so
the last release is pinned); `onnx` changes one attribute of Kagami's graph
in memory, which DirectML refuses otherwise.
