# Hello service

An example AutoScene plugin whose program is a service: a Python script
that AutoScene keeps running while the plugin's page is open, and that
answers the page. [docs/PLUGINS.md](../../../docs/PLUGINS.md) explains each
part.

- **Word count** (a tile in the rail): send it some text, and the service
  counts its words (`service.request` with the method `count`). The page
  shows the answer, and each `counted` event the service sends to the
  plugin's open pages (`emit`), with the total since the service started.

`plugin.json` says `"runtime": { "type": "service" }` and
`"engines": { "protocol": 2 }`. AutoScene starts `main.py` (through
`uv run --script --locked`) when the page opens, and stops it a minute after
the last page of the plugin closed. The page shows the service's state:
starting, ready, or stopped and why. A request that comes while it starts
waits for it.

`main.py` uses `autoscene.py`, beside it, which speaks AutoScene's protocol
in the standard library: it reads the serve line, says the service is
ready, and hands each request to the function that handles its method. It
is the helper `asp init` writes.

The service writes its own log, apart from the runs' log: **Show its
service log** in the page's ⋯ menu.

## Try it

You need [uv](https://docs.astral.sh/uv/) and the `asp` tool:

```bash
cargo install --path crates/as-plugin --features cli --bin asp
asp pack examples/plugins/hello-service hello-service.asp
```

AutoScene hasn't reviewed this package, so turn on **Plugins → Allow
plugins that AutoScene has not reviewed** first. Then **Install a
plugin…** and pick `hello-service.asp`, or **Load a folder…** and pick this
folder. Open **Word count** from the rail.

The first start can take a while: uv may need to fetch a Python. A service
has 2 minutes to say it is ready.

## Change it

`main.py.lock` is made by `uv lock --script main.py`. Make it again after
you change the `# /// script` block at the top of `main.py`, or uv refuses
to run the script.
