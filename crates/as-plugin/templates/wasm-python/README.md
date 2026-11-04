# {{name}}

An AutoScene plugin, `{{id}}`, made with `asp init --wasm python`. It is a
WebAssembly component, written in Python, and runs in AutoScene's sandbox:
it sees only what its permissions give it.

## What is in this folder

- `app.py` is the plugin. AutoScene calls its `run` for each run of
  **Say hello**, in the palette.
- `wit/plugin.wit` is the world `autoscene:plugin@1.0.0`: what AutoScene
  hands a run, what it offers it, and what the plugin returns. Keep it as
  asp wrote it.
- `plugin/` is the plugin as AutoScene loads and packs it: `plugin.json`,
  and `plugin.wasm` once you built it. Your editor checks `plugin.json` as
  you type, against `.autoscene/plugin.schema.json`.
- `.autoscene/` and `.vscode/` are for your editor.

A Python plugin can't use the network in this version of AutoScene.

## Next steps

1. In `plugin/plugin.json`, write who you are in `author` and what the
   plugin does in `description`.
2. Install [uv](https://docs.astral.sh/uv/). Build the component in this
   folder, with componentize-py 0.25.1:
   `uv tool run --from componentize-py==0.25.1 componentize-py -d wit -w plugin componentize app -o plugin/plugin.wasm`
3. Check it: `asp check plugin` lists what the component imports, and says
   what AutoScene would refuse.
4. In AutoScene, turn on **Settings → Plugins → Allow plugins that
   AutoScene has not reviewed**. Then choose **Load a folder…** and pick
   the `plugin` folder. AutoScene runs the component that is there at each
   run, so a new build counts at the next one.
5. Run it: **Say hello** is in the palette (Ctrl+K, or ⌘K on a Mac). Its
   answer shows as a toast and in Activity.
6. To share it, pack it in this folder: `asp pack plugin {{package}}`. Sign
   the package with your key (`asp keygen`, then `asp sign`), so its
   updates can be told from someone else's.

The plugin guide, `docs/PLUGINS.md` in AutoScene's repository, describes
`plugin.json`, the sandbox and what a WebAssembly plugin may use.
