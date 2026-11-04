# ArcFace goldens

Four faces from the Tears of Steel stills the app ships
(`app/public/footage/live-*.webp`), aligned to ArcFace's 112×112 template,
and the vectors ONNX Runtime's CPU gives them. A GPU session of ArcFace
embeds these first and must match them (`as_vision::golden`).

- `tos_<i>.rgb`: the aligned face, RGB24, 112×112.
- `tos_<i>.vec`: `arcface-glintr100`'s raw output, 512 little-endian f32.
- `golden.json`: the still, box, score and points of each face, and the
  hashes of the models that made them.

Made with `tools/model-prep/goldens_arcface.py` (see its docstring):

```sh
cd tools/model-prep
uv run goldens_arcface.py ../../app/src-tauri/ffmpeg ../../app/public/footage <models>/scrfd_10g.onnx <models>/arcface_glintr100.onnx ../../crates/as-vision/fixtures/arcface
```

Tears of Steel: (CC) Blender Foundation | mango.blender.org, licensed under
[CC BY 3.0](https://creativecommons.org/licenses/by/3.0/).
