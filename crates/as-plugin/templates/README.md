# {{name}}

An AutoScene plugin, `{{id}}`, made with `asp init`.

## What is in this folder

- `plugin.json` says what the plugin is and what it adds to AutoScene. Your
  editor checks it as you type, against `.autoscene/plugin.schema.json`.
<!-- {{#page}} -->
- `ui/index.html` and `ui/app.js` are its page. AutoScene shows it full
  screen, from a tile in the rail, and serves it `/_as/theme.css` (its look)
  and `/_as/sdk.js` (the `AutoScene` that talks to it).
  `.autoscene/types/` describes that SDK for your editor.
<!-- {{/page}} -->
<!-- {{#service}} -->
- `main.py` is its service. AutoScene starts it when the page opens and
  stops it a minute after the page closed. It answers the page's `count`
  requests.
<!-- {{/service}} -->
<!-- {{#action}} -->
- `main.py` is its program. AutoScene starts it for each run of **Count the
  clips**, in an export's menu.
<!-- {{/action}} -->
<!-- {{#program}} -->
- `autoscene.py` speaks AutoScene's protocol for `main.py`. `main.py.lock`
  is the lock `uv run --locked` needs.
<!-- {{/program}} -->
- `.autoscene/` and `.vscode/` are for your editor. `asp pack` leaves them
  out of the package.

## Next steps

1. In `plugin.json`, write who you are in `author` and what the plugin does
   in `description`.
<!-- {{#program}} -->
2. Install [uv](https://docs.astral.sh/uv/). After a change to the
   `# /// script` block at the top of `main.py` (a dependency, say), lock it
   again: `uv lock --script main.py`.
<!-- {{/program}} -->
<!-- {{^program}} -->
2. Change the page as you like. It may load its own files from `ui/`.
<!-- {{/program}} -->
3. In AutoScene, open **Plugins** in the rail and turn on **Allow plugins
   that AutoScene has not reviewed**. Then choose **Load a folder…** and pick
   this folder. AutoScene runs the plugin from here, as its files are now.
<!-- {{#page}} -->
4. Open its page from the rail.
<!-- {{/page}} -->
<!-- {{#action}} -->
4. Run it: **Count the clips** is in each export's menu. Right-click the
   plugin in Plugins and choose **Show its log** to see what
   else it wrote.
<!-- {{/action}} -->
5. To share it, pack it from the folder above this one:
   `asp pack {{dir}} {{package}}`. Sign the package with your key
   (`asp keygen`, then `asp sign`), so its updates can be told from someone
   else's.

The plugin guide, `docs/PLUGINS.md` in AutoScene's repository, describes
`plugin.json`, pages, services and the protocol.
