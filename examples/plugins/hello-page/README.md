# Hello page

An example AutoScene plugin that runs no program: two pages, in plain HTML
and classic scripts, which AutoScene shows full screen in a sandboxed
frame. [docs/PLUGINS.md](../../../docs/PLUGINS.md) explains each part.

- **Hello page** (a tile in the rail): keeps a note through `state.set` and
  `state.get`, so it is still there next time, and says hello in a toast
  (`ui.toast`), which AutoScene shows as `Plugin “Hello page”: …`.
- **Clips** (each project's **More** menu): lists the project's episodes
  (`project.get`) and the clips of its latest export (`exports.list`,
  `exports.get`), and plays the one you choose. `files.resolve` gives the
  page an address of its own for the file, never its path.

`plugin.json` says `"runtime": { "type": "none" }` and asks for
`project:read` and `exports:read`. A page asks AutoScene for each thing it
reads, and AutoScene checks each call against what you agreed to at
install: without `exports:read`, the Clips page gets an error for
`exports.list`, not the list.

Each page loads `/_as/theme.css` (AutoScene's look and its `as-` classes)
and `/_as/sdk.js` (the `AutoScene` it talks to AutoScene with), which
AutoScene serves to every page, and its own files from `ui/`.

## Try it

You need the `asp` tool:

```bash
cargo install --path crates/as-plugin --features cli --bin asp
asp pack examples/plugins/hello-page hello-page.asp
```

AutoScene hasn't reviewed this package, so turn on **Settings → Plugins →
Allow plugins that AutoScene has not reviewed** first. Then **Install a
plugin…** and pick `hello-page.asp`, or **Load a folder…** and pick this
folder to change the pages as they run: **Reload the page** in the page's
⋯ menu loads your change.
