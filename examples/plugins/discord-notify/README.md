# Discord Notify

An example AutoScene plugin that talks to the internet: it posts an
export's name and clips to a Discord channel through the channel's webhook.
It keeps the webhook's URL as a secret setting and posts through AutoScene,
so it needs no package of its own. [docs/PLUGINS.md](../../../docs/PLUGINS.md)
explains each part.

- **Post to Discord** (each export's menu, or after every export): posts
  the export's name, preset and how many clips it made, and, with **List
  the clips** on, each clip's name and episode. It never posts a file, a
  path or a frame.
- **Send a test message to Discord** (the command palette): posts "AutoScene
  can post here." to check the webhook.

## The webhook URL is a secret

Anyone who has a webhook's URL can post to its channel. So **Webhook URL**
is a setting of the type `secret`: AutoScene keeps it in your system's
keychain (the Windows Credential Manager, the macOS Keychain, or the Secret
Service on Linux), never in its `library.db`, a log or the run line. The
script asks for it with the call `secrets.get` each time it posts, and never
writes it to its log or its messages. AutoScene's Settings never show it
again: **Set…** replaces it, **Remove…** takes it out of the keychain.

If the URL gets out, delete the webhook in Discord (the channel's settings,
Integrations, Webhooks) and make a new one.

## Where it connects

The script posts with the call `net.fetch`: AutoScene sends the request
for it, over `https://` on port 443, only to the hosts `plugin.json` names
(Discord's) and the person agreed to, and checks the certificate itself.
The script also refuses any URL that isn't an `https://` webhook on one of
them, to say so in plain words.

`net.fetch` holds what the script asks AutoScene for. The script runs with
your user rights, as every program of a plugin does: AutoScene doesn't
stop it from opening a connection of its own. This one doesn't.

## From version 1

Version 1.0.0 kept the URL as a plain text setting in `library.db`. After
the update to 2.0.0, set **Webhook URL** again in the plugin's settings.
The old value is no longer read, and setting the secret removes it from
`library.db`.

## Try it

You need [uv](https://docs.astral.sh/uv/) and the `asp` tool:

```bash
cargo install --path crates/as-plugin --features cli --bin asp
asp pack examples/plugins/discord-notify discord-notify.asp
```

Turn on **Plugins → Allow plugins that AutoScene has not reviewed**,
then **Install a plugin…** and pick `discord-notify.asp`. In its
settings, choose **Set…** beside **Webhook URL** and paste the URL, then run
**Send a test message to Discord** from the palette. Its settings also have
**After every export**, to post each export once it finishes.

The first run can take a while: uv may need to fetch a Python.

`autoscene.py` speaks AutoScene's protocol for `main.py`; it is the helper
`asp init` writes. `main.py.lock` is made by `uv lock --script main.py`.
