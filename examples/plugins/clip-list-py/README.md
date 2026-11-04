# Clip List

An example AutoScene plugin, written in Python, that runs in AutoScene's
sandbox as a WebAssembly component. It saves a list of an export's clips in
the export's folder. [docs/PLUGINS.md](../../../docs/PLUGINS.md) describes
the sandbox.

- **Save a clip list** (each export's menu, or after every export): writes
  `<export's name> clips.txt` into the export's folder, with one line per
  clip: its name, its episode, and its first and last frame, separated by
  tabs. An export without a name gets its id instead.

It asks for `exports:write`, so AutoScene hands it the export's folder as
`/export`, which it may read and change. That folder and its own, `/data`,
are all it sees: no other file, no program, no network.

## What is in this folder

- `app.py` is the plugin. AutoScene calls its `run` for each run. It has
  no events, so its `on_event` only says so.
- `wit/plugin.wit` is the world `autoscene:plugin@1.1.0`, a copy of
  `crates/as-plugin/wit/plugin.wit`; a test keeps the two the same.
- `plugin/` is the plugin as AutoScene loads and packs it: `plugin.json`,
  and `plugin.wasm` once you built it.

## Build it

You need [uv](https://docs.astral.sh/uv/) and the `asp` tool. In this
folder, build the component with componentize-py 0.25.1:

```bash
uv tool run --from componentize-py==0.25.1 componentize-py -d wit -w plugin componentize app -o plugin/plugin.wasm
```

The component holds a whole Python, so it is about 18 MB. Check it, then
pack it:

```bash
asp check plugin
asp pack plugin clip-list-py.asp
```

A Python plugin can't use the network in this version of AutoScene.

## Try it

Turn on **Plugins → Allow plugins that AutoScene has not reviewed** (the
switch sits under the cards), then **Install a plugin…** and pick
`clip-list-py.asp` (or **Load a folder…** and pick `plugin`). Run **Save a clip list** from an
export's menu. AutoScene compiles the component once, which takes a few
seconds, and keeps what it compiled.
