# {{name}}

An AutoScene plugin, `{{id}}`, made with `asp init --wasm rust`. It is a
WebAssembly component, written in Rust, and runs in AutoScene's sandbox:
it sees only what its permissions give it.

## What is in this folder

- `src/lib.rs` is the plugin. AutoScene calls its `run` for each run of
  **Say hello**, in the palette.
- `wit/plugin.wit` is the world `autoscene:plugin@1.0.0`: what AutoScene
  hands a run, what it offers it, and what the plugin returns. Keep it as
  asp wrote it.
- `plugin/` is the plugin as AutoScene loads and packs it: `plugin.json`,
  and `plugin.wasm` once you built it. Your editor checks `plugin.json` as
  you type, against `.autoscene/plugin.schema.json`.
- `Cargo.toml`, `Cargo.lock` and `rust-toolchain.toml` build it, with the
  Rust and the `wasm32-wasip2` target that `rust-toolchain.toml` names.
- `.autoscene/` and `.vscode/` are for your editor.

## Next steps

1. In `plugin/plugin.json`, write who you are in `author` and what the
   plugin does in `description`.
2. Install [rustup](https://rustup.rs). Build the component in this folder
   (the first build fetches the Rust it needs):
   `cargo build --release --target wasm32-wasip2`
3. Copy it into `plugin/`. In PowerShell:
   `Copy-Item target/wasm32-wasip2/release/{{wasm}} plugin/plugin.wasm`.
   In a shell on Linux or macOS:
   `cp target/wasm32-wasip2/release/{{wasm}} plugin/plugin.wasm`
4. Check it: `asp check plugin` lists what the component imports, and says
   what AutoScene would refuse.
5. In AutoScene, turn on **Settings → Plugins → Allow plugins that
   AutoScene has not reviewed**. Then choose **Load a folder…** and pick
   the `plugin` folder. AutoScene runs the component that is there at each
   run, so a new build counts at the next one.
6. Run it: **Say hello** is in the palette (Ctrl+K, or ⌘K on a Mac). Its
   answer shows as a toast and in Activity.
7. To share it, pack it in this folder: `asp pack plugin {{package}}`. Sign
   the package with your key (`asp keygen`, then `asp sign`), so its
   updates can be told from someone else's.

The plugin guide, `docs/PLUGINS.md` in AutoScene's repository, describes
`plugin.json`, the sandbox and what a WebAssembly plugin may use.
