# Discord Notify (sandboxed)

The WebAssembly twin of [discord-notify](../discord-notify): an example
AutoScene plugin, written in Rust, that tells a Discord channel an export is
done, or that a project's episodes are analysed. It runs in AutoScene's sandbox, so what `plugin/plugin.json` asks for
is all it can reach: Discord, over https, and nothing on the computer but
its own folder. [docs/PLUGINS.md](../../../docs/PLUGINS.md) describes the
sandbox.

- **Post to Discord** (each export's menu, or after every export): posts
  "Export <name> is done: <n> clips." through the channel's webhook. It
  never posts a file, a path or a frame, and its posts never ping anyone.
- **Post when an analysis finishes** (an event, under "Runs by itself" in
  its settings, off until you turn it on): once a project's new episodes
  are all analysed, posts "<project>: <n> episodes analysed." (with
  ", <m> failed" when some failed).

## What is in this folder

- `src/lib.rs` is the plugin. AutoScene calls its `run` for each run of
  **Post to Discord**, and its `on_event` for each run of its event.
- `wit/plugin.wit` is the world `autoscene:plugin@1.1.0`, a copy of
  `crates/as-plugin/wit/plugin.wit`; a test keeps the two the same.
- `plugin/` is the plugin as AutoScene loads and packs it: `plugin.json`,
  and `plugin.wasm` once you built it.
- `Cargo.toml` and `Cargo.lock` build it. It is a workspace of its own,
  and pins `wit-bindgen` and `wasip2` exactly.

## The webhook URL is a secret

Anyone who has a webhook's URL can post to its channel. So **Webhook URL**
is a setting of the type `secret`: AutoScene keeps it in your system's
keychain and hands it to the plugin only when it asks (`host::secret`). The
plugin never writes it to its log or its messages. It refuses any URL that
doesn't start with `https://discord.com/api/webhooks/`.

The post goes over wasi:http. AutoScene sends it, and only to
`discord.com`, the one host `plugin.json` names and you agreed to. A
webhook copied as `discordapp.com`, `ptb.discord.com` or
`canary.discord.com`, or with a version such as `/api/v10/webhooks/`, is
refused, and the plugin says so: change it to
`https://discord.com/api/webhooks/…`.

## Build it

You need the Rust of this repository (its `rust-toolchain.toml` names the
`wasm32-wasip2` target; rustup fetches both on the first build) and the
`asp` tool:

```bash
cargo install --path crates/as-plugin --features cli --bin asp
cd examples/plugins/discord-notify-wasm
cargo build --release --locked --target wasm32-wasip2
```

Copy the component into `plugin/`. In PowerShell:

```powershell
Copy-Item target/wasm32-wasip2/release/discord_notify_wasm.wasm plugin/plugin.wasm
```

In a shell on Linux or macOS:

```bash
cp target/wasm32-wasip2/release/discord_notify_wasm.wasm plugin/plugin.wasm
```

Check it, then pack it:

```bash
asp check plugin
asp pack plugin discord-notify-wasm.asp
```

`asp check` lists what the component imports: AutoScene's world, and
wasi:http, which needs the `network` permission.

## Try it

Turn on **Plugins → Allow plugins that AutoScene has not reviewed** (the
switch sits under the cards), then **Install a plugin…** and pick
`discord-notify-wasm.asp` (or **Load a folder…** and pick `plugin`). In its
settings, choose
**Set…** beside **Webhook URL** and paste the URL. Then run **Post to
Discord** from an export's menu.
