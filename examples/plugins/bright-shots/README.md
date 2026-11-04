# Bright Shots

An example AutoScene plugin for the scenepack builder: a Python script that
needs no packages. [docs/PLUGINS.md](../../../docs/PLUGINS.md) explains each
part.

- **Bright enough** (a chip in a character's scenepack): a shot filter. It
  measures how bright the middle of each shot is with AutoScene's FFmpeg
  (`signalstats`), and leaves out the shots darker than the setting
  **Darkest to keep**. Each one shows dimmed, with "Too dark" on its card.
- **Save a cut sheet (CSV)…** (a scenepack's ⋯ menu): a pack action. It saves
  the pack, as the grid shows it, as a CSV file with a row per clip: its name,
  its episode, where it starts and ends, and how many frames it has. Nothing
  is rendered.

## How it works

`plugin.json` has `"engines": { "protocol": 2 }` and the `project:read`
permission, which a shot filter and a pack action both need.

The filter names its setting: `"settings": ["darkest"]`. So moving **Darkest
to keep** asks for new verdicts, and AutoScene measures the shots again. A
later version that measures differently raises the filter's `revision`.

AutoScene runs the filter one episode at a time. The script writes a verdict
for each shot it was sent (`p.verdict`). A shot FFmpeg can't measure stays in,
with a line in the plugin's log: a shot left without a verdict would mark the
check as failed.

The pack action writes to the run line's `output`. AutoScene gives that file
the name the person chose once the run ends well, and removes it otherwise.
A name that starts with `=`, `+`, `-` or `@` gets a `'` in front, so a
spreadsheet shows it and doesn't run it as a formula.

Both turn frames into times with the episode's `fps`, which holds only for a
file of a constant rate. In a phone recording, or a recording with a hole, the
filter measures another frame, so a bright shot can leave the pack, and the
cut sheet's times are off. Its **Frames** column stays right. The guide's
[Frames and time](../../../docs/PLUGINS.md#frames-and-time) says why.

## Try it

You need [uv](https://docs.astral.sh/uv/) and the `asp` tool:

```bash
cargo install --path crates/as-plugin --features cli --bin asp
asp pack examples/plugins/bright-shots bright-shots.asp
```

Turn on **Plugins → Allow plugins that AutoScene has not reviewed**, then
**Install a plugin…** and pick `bright-shots.asp`. Open a character's
scenepack and turn on the chip **Bright enough**, or save the pack from the ⋯
beside Export.

The first run can take a while: uv may need to fetch a Python.

`autoscene.py` speaks AutoScene's protocol for `main.py`; it is the helper
`asp init` writes. `main.py.lock` is made by `uv lock --script main.py`.
