# Writing AutoScene plugins

A plugin adds to AutoScene: a program that AutoScene runs for the person (on
an export, from the command palette, by itself after every export, or from
one of the plugin's pages), pages of its own that AutoScene shows full
screen, or both. It describes itself in a `plugin.json`, and AutoScene draws
what it adds (menu entries, palette commands, its settings, a tile for its
page). It ships as a package, an `.asp` file, which the `asp` tool makes and
signs.

This guide is for plugin authors. It describes AutoScene's plugins as they
are now: versions 1 and 2 of the manifest and of the protocol. Version 2
adds pages, services, secret settings, and calls from a plugin into
AutoScene. A plugin of version 1 works as it always did.

The design started from moongetsu's prototype in
[PR #93](https://github.com/tcmmi/AutoScene/pull/93): the review before an
install, the contributions AutoScene draws, a folder per plugin, a run whose
processes end together, and plugins that run after an export all come from
it, and so do the EDL and CSV exports.

## What a plugin can do

How a plugin runs is its `runtime.type`:

- **`process`**: AutoScene starts its program once per run and talks to it
  over stdin and stdout. The program is either a program in the plugin's
  own folder (an `.exe` on Windows), or a Python script, which
  [uv](https://docs.astral.sh/uv/) runs with the packages its lock pins.
- **`service`** (version 2): a process plugin whose program also runs while
  one of its pages is open, and answers the page's requests (see
  [Services](#services)).
- **`none`** (version 2): no program at all, only pages.

It can add:

- **Export actions**: entries in each export's menu. The person can also
  turn each one on to run after every export.
- **Commands**: entries in the command palette, with or without a project.
- **Settings**: listed under the plugin's name in Settings, which AutoScene
  draws and saves. Each run gets the values. A **secret** setting (version
  2), such as a token, is kept in the system's keychain instead (see
  [Secret settings](#secret-settings)).
- **Pages** (version 2): screens of its own, in HTML, which AutoScene shows
  full screen in a sandboxed frame: from a tile in the rail, a project's
  **More** menu, or the palette (see [Pages](#pages)).

Each run is a job in Activity, with its progress, a Stop button, and the
plugin's message when it is done. A program of protocol 2, a service and a
page may also **call** AutoScene: ask for the project, an export and its
files, the plugin's settings and secrets, a place to keep data, and the
hosts it names on the internet (see [Host methods](#host-methods)).
AutoScene checks each call against what the person agreed to.

**A plugin's program runs with the person's user rights.** It can read,
change and send anything they can. AutoScene checks what the program asks
it for, but not what the program does itself: its own files, its own
connections. For a program, the permissions a plugin names are a promise
shown before it is installed, not a wall. **A page runs in a sandbox**: it
gets nothing but what it asks AutoScene for, and AutoScene checks each ask
(see [What AutoScene enforces](#what-autoscene-enforces)). Write a plugin so
that what it does matches what it says.

## Quick start

1. Install [uv](https://docs.astral.sh/uv/) and `asp` (from a checkout of
   this repository):

   ```bash
   cargo install --path crates/as-plugin --features cli --bin asp
   ```

2. Copy `examples/plugins/hello-export` to a folder of your own. In its
   `plugin.json`, change `id` to one of yours (`dev.yourname.my-plugin`),
   and `name`, `author` and `description` with it.
3. Lock the script: `uv lock --script main.py`. Do it again each time you
   change the `# /// script` block at the top of `main.py`.
4. In AutoScene, open **Plugins** in the rail and turn on **Allow plugins
   that AutoScene has not reviewed**. Then **Load a folder…** and pick your
   folder. AutoScene runs it from there, as its files are, so a change to
   `main.py` counts at the next run.
5. Run it: **Write a clip list** is in each export's menu, and **Say hello**
   is in the palette (Ctrl+K, or ⌘K on a Mac). The result shows as a toast
   and in Activity. **Show its log** (right-click the plugin in Settings →
   Plugins) shows what else it wrote.
6. Make a package to share: `asp pack my-plugin my-plugin-1.0.0.asp`. Sign
   it with your key so its updates can be told from someone else's (see
   [Signing](#signing-with-asp)).

Or let `asp init` make a plugin to start from: `asp init my-plugin` makes
one with a page, `--service` one whose page talks to its service, and
`--action` one with an export action (see
[Developing a plugin](#developing-a-plugin)).

The other examples, in `examples/plugins`:

- `discord-notify`: an export action that posts to the internet through
  AutoScene (`net.fetch`), with its webhook as a secret setting.
- `hello-page`: two pages and no program: a note in the rail, and a
  project's episodes and clips.
- `hello-service`: a page whose service counts the words it is sent.

## How a run goes

1. The person starts an action, an export finishes and an action runs
   after it, or a page of the plugin starts one (`actions.run`). AutoScene
   queues a job, titled with the action's title, with the plugin's name
   under it.
2. When the job starts, AutoScene checks again that the plugin may run. It
   starts the program, in the plugin's data folder, and sends it the run
   line on stdin.
3. The plugin works, and writes progress and log lines to stdout. Activity
   shows "Starting <name>", then each stage the plugin names.
4. The plugin writes one result line, or one error line, and exits.
5. AutoScene shows the result's message in a toast and in Activity, or the
   error's message in a toast and as a failed job. The toast's title starts
   with "Plugin", then names the plugin and the action ("Plugin “Discord
   Notify”: Post to Discord", or "… failed"). A plugin chooses its name and
   its words, but not that first word, so its toast says it is a plugin's
   whatever the plugin calls itself.

### Where it runs

- **The program** is started by its full path in the plugin's folder, with
  the arguments `plugin.json` gives. No shell reads the command. A uv
  script runs as `uv run --script --locked <the script's full path>`.
  AutoScene looks for uv on the PATH (but not in the Microsoft Store's
  `WindowsApps` folder), then in `~/.local/bin` and `~/.cargo/bin`; then in
  `%LOCALAPPDATA%\Microsoft\WinGet\Links` on Windows, and in
  `/opt/homebrew/bin` and `/usr/local/bin` on macOS and Linux. Apps started
  from the macOS Dock or Finder don't get the shell's PATH, which is why the
  folders count. Without uv, the run fails with "This plugin needs uv,
  which isn't installed. Install it from docs.astral.sh/uv, then try
  again."
- **The current folder** is the plugin's data folder (`plugin-data/<id>`,
  see [where AutoScene keeps plugins](#where-autoscene-keeps-plugins)). It
  is the plugin's to keep files in between runs. It stays through updates,
  and an uninstall removes it only when the person asks.
- **The environment** holds only what programs need to work:
  - On every system: `PATH`, `LANG`, `TZ`, the proxy variables
    (`http_proxy`, `https_proxy`, `no_proxy`, `all_proxy`, also in
    capitals), `CUDA_VISIBLE_DEVICES` and those that start with
    `CUDA_PATH`.
  - On Windows: `SystemRoot`, `windir`, `SystemDrive`, `PATHEXT`,
    `COMSPEC`, `TEMP`, `TMP`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`,
    `ProgramData`, `ProgramFiles`, `ProgramFiles(x86)`, `ProgramW6432`,
    `CommonProgramFiles`, `NUMBER_OF_PROCESSORS`, `PROCESSOR_ARCHITECTURE`,
    `PROCESSOR_ARCHITEW6432`, `OS`, `USERNAME`, `HOMEDRIVE` and `HOMEPATH`.
  - On macOS and Linux: `HOME`, `USER`, `LOGNAME`, `TMPDIR`, `DISPLAY`,
    `WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS`, and those that start
    with `LC_` or `XDG_`.

  Every other variable is left out, also uv's own (`UV_CACHE_DIR`,
  `UV_INDEX_URL`); uv still reads its config files. AutoScene adds:

  | Variable | Value |
  |---|---|
  | `AUTOSCENE_PLUGIN_ID` | The plugin's id |
  | `AUTOSCENE_PLUGIN_DATA` | The data folder (also the current folder) |
  | `AUTOSCENE_FFMPEG`, `AUTOSCENE_FFPROBE` | AutoScene's FFmpeg and ffprobe |
  | `AUTOSCENE_PROTOCOL` | The protocol the plugin declared: `1` or `2` |
  | `AUTOSCENE_MODE` | `service` for a service; left out for a run |
  | `PYTHONUNBUFFERED`, `PYTHONUTF8` | `1` |
  | `PYTHONIOENCODING` | `utf-8` |

  A secret setting is never in the environment, nor in the run line.

- **On Windows** it starts without a console window.

A service starts the same way: the same program, folder and environment.

A plugin may run AutoScene's FFmpeg itself. The run line names it too.

### One at a time

Plugin runs wait for each other: one runs at a time, whatever plugin it
belongs to. A plugin with `"gpu": true` waits for the analysis of episodes
instead, and an analysis waits for it, so the two never share the graphics
card. Two runs of one plugin never run at once (Activity says "Waiting for
its other run"), and a run that starts while its plugin is updated waits
for the update, then runs the new version.

The engine keeps its models on the graphics card for up to 2 minutes after
its last analysis. A GPU plugin that starts right after an analysis may
find less free video memory than it expects.

A run that waits or runs already isn't queued again: the same action of the
same plugin, on the same export or in the same project, with the same input
from a page. AutoScene then says so, and a page's `actions.run` gives
`{ "jobId": null }`. A page's `actions.run` is refused while 16 runs of its
plugin wait or run, with "<name> is running. Wait for it to finish, or
stop it in Activity."

There is no time limit. A plugin may run as long as it needs, and the
person can stop it in Activity.

### Cancel and the end of a run

- **Stop** in Activity sends the cancel line. A plugin that still runs 5
  seconds later is ended, with the processes it started. On Windows they
  all end at once; a process a system service started for it (through WMI,
  the Task Scheduler or a COM server) isn't the plugin's and keeps running.
  On macOS and Linux the plugin's process group gets SIGTERM, then SIGKILL
  2 seconds later; a process that left the group (`setsid`, `setpgid`)
  keeps running. The run is cancelled, unless the plugin gave its result
  or error line first: Activity then shows that.
- After its result or error line, the plugin's stdin closes. A plugin that
  still runs 10 seconds later is ended the same way, and its line counts.
- AutoScene never tries a run again: what the plugin did, a post to a
  webhook say, would happen twice.
- Turning a plugin off stops its runs, running or still waiting. So does
  turning off "Allow plugins that AutoScene has not reviewed", for each
  plugin AutoScene hasn't reviewed. A plugin can't be uninstalled while it
  runs; its runs still waiting go with it.
- Quitting AutoScene stops the runs as Stop does. A run still waiting then
  is lost.
- None of this undoes what a plugin set up outside AutoScene: a process
  that slipped out as above, a scheduled task, a program it set to start
  with the computer. A plugin you no longer trust: uninstall it, and look
  at what starts with your computer.

### After every export

Each export action can be turned on in the plugin's settings, under
**After every export**. It is off until the person turns it on. It
then runs once an export finishes with every clip made: never after an
export that failed, was cancelled, or made only some of its clips. It runs
as a job of its own, so a plugin never fails an export or holds it up.

## The protocol (versions 1 and 2)

The plugin and AutoScene talk in JSON lines: UTF-8, one JSON object per
line, each with a `"type"`. Fields are camelCase. The examples below are
spread over several lines to be read; each is sent as one line.

A plugin declares the protocol its program speaks in `engines.protocol`,
and AutoScene speaks that one to it. Version 2 adds [calls](#calls-version-2)
into AutoScene, the `input` a page hands a run, and
[service mode](#service-mode-version-2). A program of version 1 gets the run
line it always got, byte for byte.

### The run line

The first line on the plugin's stdin is the run:

```json
{
  "type": "run",
  "protocol": 1,
  "runId": "01a19efa-56d7-7715-aa4f-e8fd5d5320d2",
  "plugin": {
    "id": "dev.autoscene.hello-export",
    "version": "1.0.0",
    "dataDir": "C:\\Users\\you\\AppData\\Local\\AutoScene\\plugin-data\\dev.autoscene.hello-export"
  },
  "app": { "version": "1.0.3-BETA", "platform": "windows-x86_64" },
  "tools": {
    "ffmpeg": "C:\\…\\ffmpeg\\ffmpeg.exe",
    "ffprobe": "C:\\…\\ffmpeg\\ffprobe.exe",
    "ffmpegVersion": "9.0"
  },
  "settings": { "format": "text", "greeting": "Hello", "withFrames": true },
  "action": { "kind": "exportAction", "id": "clip-list", "trigger": "menu" },
  "project": {
    "id": "019a2b3c-4d5e-7f60-8a9b-0c1d2e3f4a5b",
    "title": "Breaking Bad",
    "folder": "D:\\AutoScene Library\\Breaking Bad"
  },
  "export": {
    "id": "019a2b3c-9f8e-7d6c-b5a4-938271605f4e",
    "label": "Walter · scenepack",
    "preset": "prores-hq",
    "outDir": "D:\\Exports\\Walter",
    "createdAt": 1762150000000,
    "mergedFile": null,
    "clips": [
      {
        "name": "Walter_001",
        "path": "D:\\Exports\\Walter\\Walter_001.mov",
        "status": "done",
        "mediaId": "019a2b3d-0a1b-7c2d-8e3f-4a5b6c7d8e9f",
        "sourcePath": "D:\\Breaking Bad\\S01E01.mkv",
        "episode": "S01E01",
        "start": 1200,
        "end": 1450,
        "fps": "24000/1001"
      }
    ]
  }
}
```

| Field | What it holds |
|---|---|
| `protocol` | The protocol the plugin declared (`engines.protocol`), which AutoScene speaks to it: `1` or `2`. |
| `runId` | A new id for each run. It also heads the run's part of the log. |
| `plugin` | The plugin's `id` and installed `version`, and `dataDir`: its data folder, where the run starts. |
| `app` | AutoScene's `version`, and the `platform` it runs on: `windows-x86_64`, `windows-aarch64`, `macos-aarch64`, `macos-x86_64`, `linux-x86_64` or `linux-aarch64`. |
| `tools` | AutoScene's `ffmpeg` and `ffprobe` (full paths) and FFmpeg's version. |
| `settings` | Each of the plugin's settings: the person's value, else its default. A setting with neither is left out. Numbers are JSON numbers and may come as `3.0`, also for whole-number settings. |
| `action` | What to do: `kind` is `exportAction` or `command`, `id` is the contribution's id in `plugin.json`, and `trigger` says what started it: `menu` (an export's menu), `afterExport`, `palette` or `page` (one of its pages, protocol 2 only). |
| `project` | The project it runs in: its `id`, `title` and `folder`. `null` for a command run without a project open. |
| `export` | The export an export action runs on; `null` for a command. |
| `input` | What the page that started the run handed it (`actions.run`), as JSON; protocol 2, and only with `trigger` `page`. Left out when the page handed none, and for every other trigger. |

An export holds its `id`, its `label` (`"Walter · scenepack"`, or `null`
for single clips), its `preset` (such as `prores-hq`, `h264-edit` or
`bit-perfect`), `outDir` (the folder it was written to), `createdAt` (Unix
milliseconds), `mergedFile` (the one file its clips were joined into, or
`null` for a file per clip) and its `clips`. Each clip has:

| Field | What it holds |
|---|---|
| `name` | The clip's file name, without its extension. |
| `path` | Where the clip was written; `null` until it is. In a joined export, the joined file. |
| `status` | `queued`, `running`, `done`, `failed` or `cancelled`. After every export, each is `done`. |
| `mediaId` | The episode's id in the project. |
| `sourcePath` | The episode's file. Empty when the episode was removed from the project since. |
| `episode` | The episode's name. Empty when it was removed. |
| `start`, `end` | The clip's first frame in the episode, and the frame after its last. |
| `fps` | The episode's frame rate as a fraction (`24000/1001`), or `null`. |

A scenepack export is joined by default: one file, with a chapter per clip.
(The run line above is from one made with a file per clip.) Then
`mergedFile` names that file, and each clip in it has that file as its
`path`. A clip's `start` and `end` are still frames of its episode, not
times in the joined file. Check `mergedFile` before you act on each clip's `path`,
or a plugin that uploads each clip uploads the whole file once per clip.

A later AutoScene may add fields: ignore the ones you don't know.

### The cancel line

The person pressed Stop:

```json
{"type":"cancel"}
```

Stop soon. Exit as you like: the run is cancelled whatever the exit.
AutoScene sends nothing else. Read stdin on a thread of its own (as the
examples do), or a cancel line waits until the plugin reads it.

### What the plugin writes

```json
{"type":"progress","fraction":0.25,"stage":"Posting clip 3 of 12"}
{"type":"log","level":"warn","message":"Clip 4 has no file; skipped."}
{"type":"result","message":"Posted 12 clips."}
{"type":"error","message":"Discord doesn't know that webhook.","detail":"HTTP Error 404: Not Found"}
```

- **`progress`**: `fraction` is how far the run is, from 0 to 1 (left out
  while it isn't known); `stage` is what it does now, shown in Activity.
  Both are optional; without a stage Activity shows the plugin's name. A
  fraction outside 0 to 1 is taken as the nearest end. A stage is cut to 80
  characters.
- **`log`**: a line for the plugin's log. `level` is `debug`, `info` (the
  default), `warn` or `error`; another reads as `info`.
- **`result`**: the run did its work. The optional `message` is shown to the
  person, in a toast and in Activity. Keep it to one short sentence; it is
  cut to 300 characters.
- **`error`**: the run failed. `message` is required and shown to the person
  (cut to 300 characters; an empty one shows as "<name> failed."). The
  optional `detail` is technical: it goes to the plugin's log, and to
  AutoScene's own log cut to 2000 characters.

Texts lose control characters and the invisible characters that hide or turn
text, and their line breaks and runs of spaces become single spaces.

### The rules

- The first `result` or `error` line counts. Later ones only go to the log.
- A plugin that exits without either line succeeded when its exit code is
  0, with no message. With another exit code it failed with "<name> stopped
  unexpectedly."; the last 20 lines of its stderr go into AutoScene's log
  beside its exit status.
- Every line that isn't a message goes to the plugin's log: text that isn't
  a JSON object, an object with a `type` this AutoScene doesn't know, or a
  message whose fields don't read. So is all of stderr. A plugin may print
  what it likes; only the messages above are taken as messages.
- A line may be at most 64 KB (a call line more, see
  [Calls](#calls-version-2)). A longer one is cut there, marked `[cut]` in
  the log, and never read as a message.
- A byte order mark before a line is skipped, and `\r\n` line ends are fine.
- Each run writes at most 1 MB to the plugin's log. After that, the log
  says "more output dropped", and AutoScene still reads the pipes to their
  end.

### Calls (version 2)

A program of protocol 2 may ask AutoScene for something while it runs: it
writes a call line, and reads the reply with the same id on its stdin.

```json
{"type":"call","id":1,"method":"exports.get","params":{"exportId":"019a2b3c-9f8e-7d6c-b5a4-938271605f4e"}}
```

```json
{"type":"reply","id":1,"result":{"id":"019a2b3c-9f8e-7d6c-b5a4-938271605f4e","label":"Walter · scenepack","…":"…"}}
{"type":"reply","id":2,"error":{"code":"denied","message":"project.get needs the project:read permission."}}
```

- **`id`** is a whole number of 0 or more, or a text of at most 64
  characters. The reply carries it. A line with any other id is no call,
  and goes to the log.
- **`method`** is one of the [host methods](#host-methods). One AutoScene
  doesn't know gets `invalid`: "AutoScene doesn't know the method "<name>"."
- **`params`** are the method's; left out, they read as `{}`.
- A reply has a `result` (`null` for an empty one) or an `error`, with its
  [code](#error-codes) and a sentence for the log or the person.

The rules:

- Replies come as their calls are answered, not in the order they were
  made: match them by id. Read stdin on a thread of its own, or a reply
  waits until the plugin reads it.
- AutoScene answers 4 calls of a run at once, and keeps 64 more waiting
  their turn. One more is answered `busy` ("Too many calls at once.").
- It holds at most 16 MB of a run's calls and of the replies the plugin
  hasn't read. Past that, a call is answered `busy`, and the calls waiting
  start once the plugin reads. A plugin that leaves 1024 replies unread is
  ended.
- A call line may be at most 5 MB and 4 KB, room for the largest params
  (`net.fetch`'s); a service's at most 1 MB. A longer one is answered
  `invalid` ("A call is at most 5 MB."), when its id came before the cut.
- A reply goes only while the plugin's stdin is open: a call after its
  result or error line is dropped. Once the run is cancelled, each call
  still waiting, and each one after, is answered `cancelled`. The calls
  being answered then stop where they are: a `net.fetch` sends nothing
  more, and a `state.set` writes nothing.
- The log names each call and how it was answered (`[call] exports.get`,
  `[reply] exports.get ok`), never its params or its result.
- A program of protocol 1 is never answered: its call lines go to its log,
  as any line that isn't a message.

A call waits for its reply. `autoscene.py` (below) gives up after 120
seconds.

### Service mode (version 2)

A plugin of runtime `service` speaks protocol 2. Its program starts in
service mode while one of its pages is open (see [Services](#services)),
with `AUTOSCENE_MODE=service`, and its first line is the serve line: the
run line's context, without a task.

```json
{
  "type": "serve",
  "protocol": 2,
  "serviceId": "01a19efb-0c2d-7e3f-8a4b-5c6d7e8f9a0b",
  "plugin": { "id": "dev.autoscene.hello-service", "version": "1.0.0", "dataDir": "…" },
  "app": { "version": "1.1.0", "platform": "windows-x86_64" },
  "tools": { "ffmpeg": "…", "ffprobe": "…", "ffmpegVersion": "9.0" },
  "settings": {}
}
```

The service says it is ready, then answers each request of the plugin's
pages with a response of the request's id:

```json
{"type":"ready"}
```

```json
{"type":"request","id":"r1","method":"count","params":{"text":"one two"},"context":{"page":"main","projectId":null}}
```

```json
{"type":"response","id":"r1","result":{"words":2}}
{"type":"response","id":"r2","error":{"message":"Give it some text to count.","detail":"KeyError: 'text'"}}
```

- **`request`**: `method` is the service's own name for what it does (the
  page's `service.request`), `params` what the page sent (`null` when it
  sent none), and `context` the page that asks (its id in `plugin.json`)
  and the project it is open in.
- **`response`**: its `result` goes to the page (`null` for an empty one).
  With `error`, the page's call fails with `message` (cut to 300
  characters; an empty one reads "<name> couldn't do that."), and `detail`
  goes to the service's log. The ids are `r1`, `r2`, … in the order the
  service is asked: answer them in any order.

It may also write:

```json
{"type":"emit","event":"counted","data":{"words":2,"total":9}}
```

an event for every open page of the plugin (`service.on`). Its name is 1 to
64 of `a-z`, `0-9`, `.`, `_` and `-`; its data at most 256 KB of JSON. A
service emits at most 50 a second; the rest are dropped, and its log says
how many.

AutoScene also sends:

```json
{"type":"settings","settings":{"loud":false}}
{"type":"stop"}
```

- **`settings`**: the person changed one of the plugin's settings, or set
  or removed a secret. It carries every setting as it is now, as the serve
  line did; never a secret, which the service asks for again.
- **`stop`**: end soon. A service still running 3 seconds later is ended,
  with the processes it started.

A service writes `progress` and `log` lines as a run does. Before it is
ready, its progress shows on its pages ("starting", with the stage); after,
it goes to its log. It calls AutoScene as a run does, but names the
project it reads (`projectId`): a request's `context` says which project
the page is open in.

### A plugin in a few lines

```python
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
import json
import sys

run = json.loads(sys.stdin.readline())
clips = run["export"]["clips"] if run["export"] else []
for i, clip in enumerate(clips):
    print(json.dumps({"type": "progress", "fraction": i / len(clips), "stage": clip["name"]}), flush=True)
print(json.dumps({"type": "result", "message": f"Saw {len(clips)} clips."}), flush=True)
```

Flush after each line. Python's output is unbuffered under AutoScene
(`PYTHONUNBUFFERED`), but another language's may wait for a full buffer, and
Activity then shows nothing until the end.

To try a plugin without AutoScene, give it a run line yourself:

```bash
echo '{"type":"run","protocol":1,"runId":"test","plugin":{"id":"dev.me.test","version":"0.1.0","dataDir":"."},"app":{"version":"1.0.3","platform":"linux-x86_64"},"tools":{"ffmpeg":"ffmpeg","ffprobe":"ffprobe","ffmpegVersion":"9.0"},"settings":{},"action":{"kind":"command","id":"hello","trigger":"palette"},"project":null,"export":null}' | uv run --script --locked main.py
```

For protocol 2, `asp init` writes `autoscene.py` beside the program: a
helper in the standard library that reads the first line, hands each call
its reply, and serves a service's requests. The same plugin, which also
asks for the project (with `"engines": { "protocol": 2 }` and the
`project:read` permission):

```python
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
import autoscene

p = autoscene.start()  # reads the run line, then listens for the rest
clips = p.first["export"]["clips"] if p.first["export"] else []
for i, clip in enumerate(clips):
    p.progress(i / len(clips), clip["name"])
try:
    project = p.call("project.get")  # its result, or autoscene.CallError
    p.result(f"Saw {len(clips)} clips of {project['title']}.")
except autoscene.CallError as e:
    p.error(str(e), e.code)
```

`p.cancelled` is set once AutoScene asks the run to stop. For a service,
`@p.handle("<method>")` makes a function answer a request, `p.emit` sends
an event, and `p.serve()` answers until AutoScene says stop:
`examples/plugins/hello-service` shows it.

## Host methods

What a plugin may ask AutoScene for: from a page (through the SDK), from a
run of a program of protocol 2, and from a service (with call lines). Each
call is checked before anything is read: its params' size, then whether
its caller may call the method, then whether the plugin holds what the
method needs, then the project and the export it names.

| Method | Called by | Needs | Params (at most) | Gives |
|---|---|---|---|---|
| `settings.get` | page, run, service | | `{}` (1 KB) | Each setting: the person's value, else its default. Never a secret. |
| `secrets.get` | page, run, service | a secret setting | `{ key }` (1 KB) | `{ value }`: the secret, or `null` while the person hasn't set it. Only the plugin's own secret settings. |
| `project.get` | page, run, service | `project:read` | `{ projectId? }` (1 KB) | The project (below). |
| `exports.list` | page, run, service | `exports:read` | `{ projectId? }` (1 KB) | Its exports, newest first (below). |
| `exports.get` | page, run, service | `exports:read` | `{ projectId?, exportId }` (1 KB) | The export (below). |
| `files.resolve` | page, run, service | `exports:read` for a clip or a joined file, `project:read` for an episode | `{ kind: "clip", projectId?, exportId, index }`, `{ kind: "merged", projectId?, exportId }` or `{ kind: "episode", projectId?, mediaId }` (1 KB) | `{ url, path, mime, size }` (below). |
| `net.fetch` | page, run, service | `network`, and a host it names | a request (5 MB) | The answer (see [Through AutoScene to the internet](#through-autoscene-to-the-internet)). |
| `state.get` | page, run, service | | `{ key }` (1 KB) | `{ value }`, `null` for a key it doesn't hold. |
| `state.set` | page, run, service | | `{ key, value }` (300 KB) | `null` |
| `state.delete` | page, run, service | | `{ key }` (1 KB) | `null`; a key it doesn't hold is no error. |
| `state.keys` | page, run, service | | `{}` (1 KB) | The keys, in order. |
| `service.request` | page | runtime `service` | `{ method, params?, timeoutMs? }` (1 MB) | The service's result (see [Services](#services)). |
| `actions.run` | page | a program of protocol 2 | `{ action, exportId?, input? }` (64 KB) | `{ jobId }`; `null` when the same run waits or runs already. |
| `ui.toast` | page | | `{ text, description?, tone? }` (2 KB) | `{ shown }` |
| `ui.navigate` | page | | `{ path, replace? }` (1 KB) | `null` |
| `ui.copyText` | page | | `{ text }` (64 KB) | `null` |
| `ui.openExternal` | page | | `{ url }` (4 KB) | `{ opened }`, once the person said yes or no. |
| `files.save` | page | | `{ name, text?, dataBase64?, filters? }` (48 MB) | `{ saved }` |
| `files.open` | page | | `{ filters?, maxBytes? }` (1 KB) | `{ name, size, dataBase64 }`, or `null` when the person picked none. |

The page-only methods are described under [Pages](#what-a-page-may-ask).
What a method needs:

- **A permission**: one the person agreed to at install that the plugin's
  `plugin.json` still asks for. A loaded folder whose `plugin.json` asks
  for more gets nothing more until it is loaded again. Without it:
  "project.get needs the project:read permission."
- **A secret setting**: `secrets.get` reads only keys of the plugin's own
  secret settings ("secrets.get reads only the plugin's secret settings,
  and <key> isn't one.").
- **A service**: `service.request` needs a plugin of runtime `service`
  ("service.request needs a plugin that runs a service.").
- **A program of protocol 2**: `actions.run` needs a plugin whose program
  reads a page's run ("actions.run needs a plugin whose program speaks
  protocol 2.").

Params past their cap are refused before AutoScene reads them: "The params
of state.set are larger than 300 KB." Params that don't fit their method
get `invalid`: "The params of exports.get don't fit: missing field
`exportId`."

### What a call reads

- **A page** reads the project it is open in (a project page), and no
  other: "A page reads only the project it is open in." A page of its own
  is in no project: "This page isn't open in a project."
- **A run** reads the project it runs in ("A run reads only the project it
  runs in."), and a run of an export action reads only its export ("A run
  reads only the export it runs on."). A command run outside a project:
  "This run isn't in a project."
- **A service** is in no project: it names the one it reads with
  `projectId` ("A service names the project it reads: projectId."). Its
  program runs with the person's rights anyway.
- A page and a run may leave `projectId` out, or name their own project.

### Projects and exports

A page gets no paths. `project.get` for a page:

```json
{
  "id": "019a2b3c-4d5e-7f60-8a9b-0c1d2e3f4a5b",
  "title": "Breaking Bad",
  "episodes": [
    { "id": "019a2b3d-0a1b-7c2d-8e3f-4a5b6c7d8e9f", "name": "S01E01", "frames": 83112, "fps": "24000/1001", "missing": false }
  ]
}
```

A run and a service also get the project's `folder` and each episode's
`path`. An episode's `name` is its number (`S01E03`), else its file's name;
`frames` and `fps` are `null` while not known; `missing` says its file
isn't where it was.

`exports.list`, for every caller:

```json
[
  { "id": "019a2b3c-9f8e-7d6c-b5a4-938271605f4e", "label": "Walter · scenepack", "preset": "prores-hq", "createdAt": 1762150000000, "merged": false, "clips": 12, "done": 12, "status": "done" }
]
```

`status` is `queued`, `running`, `done`, `failed` or `cancelled`; `clips`
is how many it has, `done` how many are written, and `merged` whether they
are joined into one file.

`exports.get` for a page:

```json
{
  "id": "019a2b3c-9f8e-7d6c-b5a4-938271605f4e",
  "label": "Walter · scenepack",
  "preset": "prores-hq",
  "createdAt": 1762150000000,
  "merged": false,
  "clips": [
    { "index": 0, "name": "Walter_001", "status": "done", "mediaId": "019a2b3d-0a1b-7c2d-8e3f-4a5b6c7d8e9f", "episode": "S01E01", "start": 1200, "end": 1450, "fps": "24000/1001" }
  ]
}
```

A clip's `index` is what `files.resolve` takes. A run and a service get the
export as an export action's run line has it, with `outDir`, `mergedFile`
and each clip's `path` and `sourcePath` (see [the run line](#the-run-line)).

`files.resolve` gives a page an address of its own page session for the
file, `{ "url": "http://asp.localhost/<session>/~f/<token>", "path": null,
"mime": "video/quicktime", "size": 734003200 }` (`asp://localhost` on
macOS and Linux): for a picture of at most 32 MB, a video or a sound only
("A page can show only pictures, video and sound."), never through a link.
Video and sound come in parts of 1 MB, as a `<video>` or `<audio>` asks for
them. A run and a service get the file's `path` and no `url`; `mime` is
`application/octet-stream` for a kind AutoScene doesn't know. A file that
isn't there: "That clip isn't written yet.", "That file isn't where it
was."

### Error codes

A call that fails gets one of these codes, and a sentence that says why
(the SDK's `CallFailure` has both, as `code` and `message`):

| Code | When |
|---|---|
| `denied` | The plugin has no grant for it: a permission, a host or a secret it doesn't hold, a method its caller can't call, a project or an export outside its caller's. |
| `invalid` | The call itself is wrong: a method AutoScene doesn't know, params that don't fit or are too large, a path or a name it refuses. |
| `not_found` | What it names isn't there. |
| `busy` | Too many calls at once, or what it asks for is busy: a dialog already open, a plugin being updated. |
| `failed` | AutoScene couldn't do it: the keychain, the network, the service's own error. |
| `closed` | The page that called was closed. |
| `cancelled` | Its run was cancelled or ended, its service stopped, or the request was cancelled. |

## Pages

A page is an HTML file of the plugin, under `ui/`, which AutoScene shows
full screen under its own top bar. The bar shows the page's title, the
plugin's icon and a "Plugin" badge the plugin can't word, and a ⋯ menu:
**Reload the page**, **Its settings**, **Show its log**, **Show its service
log** for a service, and **Close**. Everything under the bar is the
plugin's.

```json
"contributes": {
  "pages": [
    { "id": "main", "title": "Hello page", "description": "Keeps a note.", "entry": "ui/index.html", "rail": true },
    { "id": "clips", "title": "Clips", "entry": "ui/clips.html", "scope": "project" }
  ]
}
```

- **`id`** (1 to 32 lowercase letters, digits and dashes) and **`title`**
  (at most 40 characters) are required; **`description`** has at most 200.
- **`entry`** is the page's file: a package path under `ui/` that ends in
  `.html`, which the package must hold ("The plugin package is missing its
  page ui/gone.html.").
- **`scope`**: `global` (the default) is a page of its own; `project` is a
  page in each project, which may read that project. A scope this AutoScene
  doesn't know skips the page, with a warning.
- **`rail`**: a global page with `"rail": true` gets a tile in the rail,
  under Library, with the tooltip `Plugin “<name>”: <title>`. The rail shows
  at most four plugin pages; the others are in the palette. A project page
  can't have one (a warning).

A plugin has at most 4 pages. They show:

- a global page with `rail`: in the rail, and in the palette ("Open
  <title>");
- any other global page: in the palette;
- a project page: in each project's top bar, under **More**, and in the
  palette while a project is open.

They show only while the plugin may run: installed, on, and allowed (see
[Trust](#trust)). AutoScene shows one page at a time: opening another, or
any other screen, closes it.

### What a page is served

The frame loads the page from an address of its own session:
`http://asp.localhost/<session>/ui/index.html` on Windows,
`asp://localhost/<session>/ui/index.html` on macOS and Linux. A page links
its own files by relative paths (`hello.js`, `page.css`), and AutoScene's
two by absolute ones:

```html
<link rel="stylesheet" href="/_as/theme.css" />
<script src="/_as/sdk.js"></script>
<script src="app.js" defer></script>
```

- A page's own files are those under `ui/`, as its package lists them: a
  file whose size or SHA-256 on disk isn't its lock's is not served. A
  loaded folder's are served as they are now, never through a link.
- A file is served only by a type its extension names, from a fixed list:
  `.html`, `.js`, `.mjs`, `.css`, `.json`, `.map`, `.svg`, `.png`, `.jpg`,
  `.jpeg`, `.gif`, `.webp`, `.avif`, `.ico`, `.woff2`, `.woff`, `.ttf`,
  `.otf`, `.txt`, `.md`, `.mp4`, `.m4v`, `.webm`, `.mov`, `.mp3`, `.m4a`,
  `.aac`, `.wav`, `.ogg`, `.opus` and `.flac`. Any other isn't served. A
  file has at most 32 MB.
- Use classic scripts (`<script src>`), as the examples do. Modules load on
  Windows, but whether they load on macOS and Linux isn't checked yet.

### The sandbox

A page runs in a frame sandboxed to scripts and forms
(`sandbox="allow-scripts allow-forms"`, never its own origin), with no
referrer and no feature it may ask for. Every answer for it carries a CSP
that says the same sandbox, whatever the frame says:

```text
default-src 'none'; script-src <session>/ <kit>/; style-src <session>/ <kit>/ 'unsafe-inline';
img-src <session>/ data: blob:; font-src <session>/ <kit>/ data:; media-src <session>/ blob:;
connect-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; manifest-src 'none';
object-src 'none'; form-action 'none'; base-uri 'none'; sandbox allow-scripts allow-forms;
frame-ancestors <AutoScene's window>
```

So a page:

- loads scripts only from its own files and the kit's: inline scripts,
  `eval` and `new Function` don't run. Inline styles do.
- reaches nothing on the network: no `fetch`, XHR, WebSocket, picture,
  font, form or navigation away. Its way out is `net.fetch`.
- has no storage, cookies or caches of its own (each throws): it keeps data
  with `state.*`.
- opens no window, no frame, no worker, no plugin object, and no dialog of
  the browser's own; nor the camera, the microphone, the location or the
  clipboard of its own accord.
- can't call AutoScene's commands, draw outside its frame, or cover
  AutoScene's top bar and rail.
- reads no file but those it asks for with `files.resolve` (with a
  permission) and `files.open` (the person's pick), and no other plugin's
  data or secrets.

A page keeps the person's keys working. The SDK stops what the browser in
the frame would do on its own with a key the page didn't take (reload,
print, find, save the page; on macOS there is none), and with a link's
middle or modified click; copy, paste and undo stay with fields. It hands
the keys and right-clicks the page didn't take to AutoScene, so its
shortcuts (Ctrl+K, ⌘K on a Mac) and its menu work over a page too: a
right-click outside a field shows the page's ⋯ menu. A right-click in a
field shows no menu on Windows, where AutoScene turns the browser's menus
off; its keys (copy, paste, undo) still work there. A file dropped on a
plugin page adds nothing.

### What a page may ask

Besides the [host methods](#host-methods) any caller has, a page has its
own. Each reaches outside AutoScene only through a step of the person's:

- **`ui.toast`**: a toast AutoScene shows as `Plugin “<name>”: <text>`,
  with an optional `description` and `tone` (`neutral` or `error`), and no
  button. `text` has 1 to 200 characters, `description` at most 300. A page
  shows at most two a minute, at least 2 seconds apart; past that it gets
  `{ "shown": false }` and nothing shows.
- **`ui.navigate`**: moves inside the page, so Back and Forward work. The
  path is relative, at most 512 characters, like `clips/3` (no `.` or `..`
  part). It becomes part of AutoScene's route
  (`/x/<plugin>/<page>/<path>`), and `page.path` and `onRoute` give it back.
- **`ui.copyText`**: copies at most 64 KB of text, without NUL, while the
  frame has the focus and AutoScene is in front ("A page copies only while
  it has the focus.", "A page copies only while AutoScene is in front."),
  on Windows right after the person clicked or typed in it ("A page copies
  only right after the person clicks or types in it."), never within 5
  seconds of a click or a key elsewhere in AutoScene, which Windows counts
  for the page too ("A page copies only 5 seconds after the person clicks
  or types elsewhere in AutoScene."), and at most 10 times a minute
  ("A page copies at most 10 times a minute.").
- **`ui.openExternal`**: a plain `https://` link (at most 2000 characters,
  no `user@` before its host) opens in the person's browser once they say
  yes in a dialog of the system's own: "Open this link in your browser?",
  with `Plugin “<name>” asks to open:` and the link, and **Open**. It gives
  `{ "opened": false }` when they don't.
- **`files.save`**: the person saves a file where they choose, in the
  system's dialog. `name` is a plain file name of at most 100 characters,
  like `clips.txt`; the contents are `text` or `dataBase64`, at most 32 MB.
  A page never saves a program, a script, an installer or a shortcut
  (`.exe`, `.bat`, `.ps1`, `.sh`, `.py`, `.app`, `.dmg`, `.lnk`, `.desktop`
  and the like, also by the name the person typed): "A page can't save a
  program, a script or a shortcut." Nor a remote desktop's or a Windows
  Sandbox's settings (`.rdp`, `.wsb`). On Windows and macOS the file is
  marked as one from the internet, as a browser's download is, and what
  comes out of an archive with it. The dialog's title names the plugin:
  `Plugin “<name>” saves a file`. It gives `{ "saved": false }` when the
  person cancels.
- **`files.open`**: the person picks a file in the system's dialog, titled
  `Plugin “<name>” asks for a file`; the page gets its name and contents
  (`dataBase64`), never its path. At most
  `maxBytes`, and never more than 16 MB: "That file is larger than 16 MB."
  `null` when they cancel.

`filters` (both dialogs) lists at most 16 kinds of file, each with a name
and at most 16 plain extensions: `[{ "name": "Text", "extensions": ["txt"] }]`.

A page opens one dialog at a time ("This page has a dialog open already."),
only while its frame has the focus and AutoScene is in front ("A page opens
a dialog only while it has the focus.", "A page opens a dialog only while
AutoScene is in front."), on Windows right after the person clicked or
typed in it ("A page opens a dialog only right after the person clicks or
types in it.") and never within 5 seconds of a click or a key elsewhere in
AutoScene ("A page opens a dialog only 5 seconds after the person clicks or
types elsewhere in AutoScene."), and after the person says no to one, 3 seconds later at the soonest
("A page asks again only 3 seconds after the person says no."). A page may
have 64 calls waiting, of which AutoScene answers 16 at once; one more gets
`busy` ("This page has 64 calls waiting already."). The params of each
method have the cap the table above gives ("The params of ui.toast are
larger than 2 KB."), and AutoScene reads none of params past it; a page's
waiting calls hold at most 64 MB of params together ("This page has too
much waiting already."). A method AutoScene doesn't have gets `invalid`
("AutoScene has no such method.").

### A page's life

A page's session starts as it opens, with what the plugin may use then.
It closes when the person leaves the page or reloads it, and when the
plugin changes: an update or an uninstall ("It is being updated.", "It is
being uninstalled."), turning it off ("It was turned off."), or turning
off "Allow plugins that AutoScene has not reviewed" for a plugin AutoScene
hasn't reviewed. The page hears why (`onClose`), each call it still waits
for fails with `closed`, and AutoScene covers it with "This page was
closed." and **Reload**.

A page that stops answering for 10 seconds is covered with "This page
isn't responding.", with **Reload** and **Close**. On macOS and Linux a
page that hangs holds AutoScene's window too: after 5 seconds, while the
window is in front, AutoScene asks in a dialog of the system's own
("Plugin “<name>”'s page stopped responding."), and **Close it** closes
the page and goes back to the library.

### What a page keeps

A page's frame has no storage, so a page keeps what it needs between
visits with `state.get`, `state.set`, `state.delete` and `state.keys`. The
plugin's pages, runs and service share it: `page-state.json`, in the
plugin's data folder.

- A key is 1 to 128 letters, digits, dots, colons, dashes or underscores;
  a value is at most 256 KB of JSON, and the whole file at most 1 MB.
- A write replaces the file whole, so a crash leaves it as it was or as it
  is now. A file that isn't JSON, or is too large, reads as empty, and is
  kept as `page-state.json.bad`.
- **The file is the pages'.** A program may read it in its data folder
  (its current folder), and must never write it: AutoScene writes it in
  turns, and a program's write would be lost, or break it. Nothing stops a
  program from writing it; a broken one reads as empty.
- It goes with the plugin's data, when the person uninstalls the plugin
  with **Delete what it kept too**.

## The SDK

AutoScene serves two files to every page, at `/_as/`:

- **`sdk.js`**: a classic script that defines the global `AutoScene`.
- **`theme.css`**: AutoScene's colours, fonts and page rules, and plain
  classes that look like its controls.

```js
AutoScene.connect().then(async (page) => {
  const note = await page.state.get("note")
  page.onClose((reason) => show(`AutoScene closed this page. ${reason}`))
})
```

`AutoScene.connect(options)` says `ready` to AutoScene and gives the page
once AutoScene answers, or fails with "AutoScene didn't answer this page."
after `timeoutMs` (10 seconds). A second `connect()` gives the same page.
Its options: `guards` (stop the browser's own keys and menu in the page;
on), `forwardKeys` (hand the keys and right-clicks the page didn't take to
AutoScene; on), `timeoutMs`, and `mock` (see
[Developing a plugin](#developing-a-plugin)).

The page it gives:

| What | Is |
|---|---|
| `plugin`, `page` | `{ id, name, version }` and `{ id, title, scope }`. |
| `projectId` | The project it is open in; `null` for a page of its own. |
| `path`, `onRoute(cb)` | The path inside the page, and each change Back or Forward makes. |
| `permissions` | What its calls may use: what the person agreed to and its `plugin.json` still asks for. |
| `call(method, params)` | Any host method, as it is. The helpers below are made of it. |
| `project.get()`, `exports.list()`, `exports.get(exportId)` | The project and its exports. |
| `files.resolve(ref)`, `files.save(f)`, `files.open(f?)` | `save` gives whether it saved, `open` the file or `null`. |
| `net.fetch(request)` | The answer. |
| `secrets.get(key)` | The secret, or `null`. |
| `settings.get()`, `settings.onChange(cb)` | The settings, and each time the person changes them. |
| `state.get(key)`, `state.set(key, value)`, `state.delete(key)`, `state.keys()` | What the plugin keeps. |
| `service.request(method, params?, { timeoutMs? })`, `service.on(event, cb)`, `service.onState(cb)` | Its service: an answer, its events, and its state (`starting`, `ready` or `stopped`, with a message). |
| `actions.run(action, { exportId?, input? })` | The run's job id, or `null` when it waits already. |
| `ui.toast(text, { description?, tone? })`, `ui.navigate(path, { replace? })`, `ui.copyText(text)`, `ui.openExternal(url)` | `openExternal` gives whether it opened. |
| `onClose(cb)` | The page was closed, and why. |

Each `on…` gives a function that stops it. A call that fails rejects with a
`CallFailure`: an `Error` with the call's `code` (see
[Error codes](#error-codes)).

**Types.** `asp init` writes the SDK's TypeScript declarations to
`.autoscene/types/`, and the page's script starts with:

```js
// @ts-check
/// <reference path="../.autoscene/types/global.d.ts" />
```

so the editor checks the script against the SDK, in plain JavaScript.

**`theme.css`** holds AutoScene's dark theme (`color-scheme: dark`; a page
gets no light one), its colour tokens as CSS variables (`--bg-canvas`,
`--bg-panel`, `--fg-1` to `--fg-3`, `--accent`, `--danger`, `--ok`, …), its
fonts (inlined: a page loads no font from anywhere else), and these
classes:

| Class | What |
|---|---|
| `as-btn` | A button. `data-variant`: `primary` (the one main action of a screen), `secondary` (the default), `ghost`, `accent` (selected or running), `danger`; `data-size="sm"` for the smaller one. |
| `as-input` | A text field or text area. `aria-invalid="true"` turns it red, for a value that won't do. |
| `as-card` | A group of settings or facts. |
| `as-badge` | A chip for a short fact. `data-tone`: `neutral` (the default), `accent`, `danger`, `outline`. |
| `as-display`, `as-eyebrow` | A big title, and a small label over it. |
| `as-timecode` | Timecodes and frame numbers. |
| `as-glass` | A surface that floats. |
| `as-skeleton` | Something still loading. |

The page's background stays clear over AutoScene's, and its body scrolls.
A `<select>`, a checkbox or a range in a page is the browser's own.

## Services

A plugin of runtime `service` keeps its program running while one of its
pages is open, to answer the pages' requests (`service.request`): a model
held in memory, say, or a connection kept open. Its program speaks
protocol 2. Its export actions and commands still run as runs, each a
program of its own, beside the service.

- **When it runs.** It starts when a page of its plugin opens, or when a
  page asks it something while it doesn't run. It stops a minute after the
  plugin's last page closed, when the plugin is updated, uninstalled or
  turned off, when "Allow plugins that AutoScene has not reviewed" goes off
  for a plugin AutoScene hasn't reviewed, and when AutoScene quits. Never at
  AutoScene's start, and never for a run. There is one service per plugin,
  for all its pages.
- **Its start.** It has 2 minutes to say it is ready (uv may fetch Python
  and the script's packages first). Until then its pages show it starting,
  with the stage its progress lines name; a request that comes meanwhile
  waits for it. One that isn't ready in time is ended: "<name> didn't
  start in time."
- **Requests.** At most 32 of them wait for their answer at once; one more
  gets `busy` ("<name> has too many requests waiting."). Each waits 60
  seconds, or the `timeoutMs` the page gives, up to 10 minutes: "<name>
  didn't answer in time." A request of a page that closes is cancelled. An
  answer may be at most 1 MB: "<name>'s answer is larger than 1 MB."
- **Settings.** When the person changes a setting, or sets or removes a
  secret, the service gets a `settings` line, and its pages
  `settings.onChange`.
- **Crashes.** A service that ends unasked has crashed: each request still
  waiting fails, its pages hear "<name> stopped unexpectedly.", and its log
  notes the end of its stderr. The next request starts it again. After 3
  crashes in 5 minutes it isn't started again ("<name> keeps stopping. Its
  log says why.") until a page of its plugin opens again.
- **Its log** is its own, beside the plugin's run log:
  `logs/plugins/<id>.service.log`. It names each request, call, response
  and event, never what they carry. A service runs for hours, so its log
  drops nothing: past 1 MB it moves to `<id>.service.log.1` and goes on in
  a new file. **Show its service log** is in the page's ⋯ menu, and the
  plugin's settings name the file.
- **Stop.** A stop line, then 3 seconds to end, then it is ended with the
  processes it started, as a run is.

**Keep GPU work in actions.** A service runs beside the analysis of
episodes, whatever the plugin's `gpu` says: only its runs wait for the
analysis. Keep a service light, and do the heavy work in an action, which
the page starts with `actions.run`.

## Secret settings

A setting of the type `secret` (version 2) holds a token, a password or a
webhook's address:

```json
{ "key": "webhookUrl", "type": "secret", "title": "Webhook URL", "placeholder": "https://discord.com/api/webhooks/…" }
```

- **Where it is kept**: in the system's keychain, never in AutoScene's
  `library.db`, a log, the run line, `settings.get` or a run's environment.
  On Windows that is the Credential Manager, on macOS the Keychain, and on
  Linux the Secret Service (GNOME Keyring or KWallet), which must be
  running: without one, a call gets "Your system's keychain can't be used:
  this computer has none running. Once one runs, restart AutoScene." Each
  secret is kept under `AutoScene plugin <id> (<library>)` and its key;
  `<library>` keeps apart the secrets of two AutoScenes with data folders
  of their own.
- **Who sets it**: only the person, in the plugin's settings. **Set…** asks
  for it, hidden as it is typed ("AutoScene never shows it again"), and
  **Remove…** takes it out of the keychain. A plugin can read its secrets,
  never set them.
- **Who reads it**: the plugin, with `secrets.get`, from its pages, its
  runs and its service. Each call reads the keychain anew; a value is
  never kept. `null` while it isn't set. A page reads one at a time: the
  keychain may ask the person in a dialog of its own (on macOS after an
  update of AutoScene, on Linux while it is locked). Once a page's read
  failed, its next ones get the same error without asking again, until
  the page is opened again.
- **Its limits**: 1 to 1024 characters (an emoji counts as two), without
  NUL. A plugin has at most 8 secret settings, with no `default` ("A secret
  setting has no default."), and no two keys that differ only in capitals,
  which a keychain doesn't tell apart. `placeholder` has at most 200
  characters.
- **When it goes**: with **Remove…**, and with the plugin, when it is
  uninstalled. A secret set under a key that held a plain value before
  (an update made the setting a secret) removes that value from
  `library.db`; the plain value is never handed to the plugin.
- **On macOS**, each update of AutoScene is signed anew, so the Keychain
  may ask to allow AutoScene again after one. The setting's row says so.

A secret needs something that can ask for it: a program of protocol 2, a
page, or runtime `service` or `none`. A process plugin of protocol 1
without a page has its secret settings skipped, with a warning.

## Through AutoScene to the internet

`net.fetch` sends a request for a plugin that has the `network` permission,
to the hosts its `network.hosts` names and the person agreed to:

```json
{
  "url": "https://discord.com/api/webhooks/123/abc",
  "method": "POST",
  "headers": [["content-type", "application/json"]],
  "body": "{\"content\":\"Posted 12 clips.\"}",
  "timeoutMs": 20000
}
```

```json
{ "status": 204, "url": "https://discord.com/api/webhooks/123/abc", "headers": [["date", "…"]], "body": null, "bodyBase64": null }
```

- **Where**: `https://` addresses on port 443 only, whose host is exactly
  one of the plugin's (in any case, without a trailing dot): no subdomain,
  no address such as 127.0.0.1, no user name or password in it ("net.fetch
  reaches only the hosts the plugin names, and <host> isn't one.").
- **Which addresses**: AutoScene looks the name up, and connects only to
  public addresses, never to this computer or its network ("<host>
  resolves to an address on this computer or its network."), and only to
  the address it checked. In a version 2 `plugin.json`, a host that can't
  be on the internet gets the warning "net.fetch can't reach <host>.": an
  address, a single name, a name whose last part is `localhost`, `local`,
  `lan`, `internal`, `localdomain`, `test`, `invalid` or `example`, or one
  that ends in `home.arpa`.
- **No proxy**: it never goes through one, which would look the name up
  itself. A plugin behind a company proxy can't reach the internet this
  way.
- **The request**: `GET` (the default), `HEAD`, `POST`, `PUT`, `PATCH` or
  `DELETE`. At most 32 headers, 8 KB in all, as `[name, value]` pairs;
  never `host`, `user-agent`, `connection`, `content-length`,
  `transfer-encoding`, `te`, `upgrade`, `expect`, `keep-alive`, `trailer`
  or a `proxy-` one. AutoScene sends `user-agent: AutoScene/<version>
  (plugin <id>)`, so a server sees which plugin asks. A body is `body`
  (text) or `bodyBase64`, never both, at most 4 MB; a `GET` or `HEAD` has
  none. The address has at most 8 KB.
- **Time**: 30 seconds, or `timeoutMs` from 1 to 120000, redirects
  included: "<host> didn't answer in time."
- **Redirects**: at most 5, each checked as the first one was. A 303, and
  a 301 or 302 after a `POST`, go on as a `GET` without the body.
  `authorization` and `cookie` don't go on to another host.
- **The answer**: its `status`, the `url` it came from after redirects,
  its `headers` (names in lowercase), and its body: `body` when it is text,
  JSON or XML in UTF-8 without control characters, else `bodyBase64`. At
  most 8 MB ("The answer is larger than 8 MB."). A status such as 404 is an
  answer, not an error.
- **At once**: at most 8 `net.fetch` calls of one plugin wait for their
  answer; one more gets `busy` ("Too many net.fetch calls at once.").
- A run that is cancelled, or a page that closes, stops its requests where
  they are: nothing more is sent. What was sent may have arrived.

`net.fetch` holds what a plugin asks AutoScene to send. A program's own
connections aren't checked: it runs with the person's rights. A page has
no other way out (see [What AutoScene enforces](#what-autoscene-enforces)).

## plugin.json (manifest versions 1 and 2)

The `plugin.json` of `examples/plugins/hello-export`, which stays a
version 1 sample:

```json
{
  "manifestVersion": 1,
  "id": "dev.autoscene.hello-export",
  "version": "1.0.0",
  "name": "Hello Export",
  "description": "Writes a list of an export's clips beside them, and says hello from the command palette. An example plugin.",
  "author": { "name": "AutoScene", "url": "https://github.com/tcmmi/AutoScene" },
  "repository": "https://github.com/tcmmi/AutoScene",
  "engines": { "autoscene": ">=1.0.0, <2", "protocol": 1 },
  "runtime": {
    "type": "process",
    "command": { "*": ["uv", "run", "--script", "--locked", "main.py"] }
  },
  "permissions": ["exports:read", "exports:write"],
  "contributes": {
    "exportActions": [{ "id": "clip-list", "title": "Write a clip list", "description": "…" }],
    "commands": [{ "id": "hello", "title": "Say hello", "description": "…" }],
    "settings": [{ "key": "greeting", "type": "string", "title": "Greeting", "default": "Hello", "maxLength": 40 }]
  },
  "icon": "icon.svg"
}
```

And of `examples/plugins/hello-page`, a version 2 plugin with two pages and
no program:

```json
{
  "manifestVersion": 2,
  "id": "dev.autoscene.hello-page",
  "version": "1.0.0",
  "name": "Hello page",
  "description": "A page in the rail that keeps a note, and one in each project that shows its episodes and clips. An example plugin.",
  "author": { "name": "AutoScene", "url": "https://github.com/tcmmi/AutoScene" },
  "engines": { "autoscene": ">=1.0.0, <2", "protocol": 2 },
  "runtime": { "type": "none" },
  "permissions": ["project:read", "exports:read"],
  "contributes": {
    "pages": [
      { "id": "main", "title": "Hello page", "entry": "ui/index.html", "rail": true },
      { "id": "clips", "title": "Clips", "entry": "ui/clips.html", "scope": "project" }
    ]
  },
  "icon": "icon.svg"
}
```

### Versions 1 and 2

Version 2 only adds to version 1: a version 1 manifest reads as it always
did, and with `"manifestVersion": 2` it reads the same, but for one warning
(the last row below). What each version does with what version 2 adds:

| In plugin.json | Version 1 | Version 2 |
|---|---|---|
| `"engines": { "protocol": 2 }` | Read. | Read. |
| `contributes.pages` | Skipped, with the warning `Skipped contributes.pages: pages need "manifestVersion": 2.` | Read (see [Pages](#pages)). |
| A setting of the type `secret` | Skipped, with the warning `Skipped the setting "<key>": secret settings need "manifestVersion": 2.` | Read, when something can ask for it (see [Secret settings](#secret-settings)). |
| `"runtime": { "type": "service" }` | Refused: `The plugin's runtime.type "service" needs "manifestVersion": 2.` | Read; it needs protocol 2. |
| `"runtime": { "type": "none" }` | Refused, as `service` is. | Read. |
| A host in `network.hosts` that can't be on the internet | Read. | Read, with the warning `net.fetch can't reach <host>.` |

AutoScene 1.0 reads only version 1 and protocol 1: it refuses a version 2
manifest, and protocol 2, with "This plugin needs a newer AutoScene." So
does this AutoScene with a `manifestVersion` above 2, a protocol above 2,
or a `runtime.type` it doesn't know (`wasm`, say).

### Fields

| Field | Required | What it is |
|---|---|---|
| `manifestVersion` | yes | `1` or `2`. |
| `id` | yes | The plugin's id (see below). It never changes: an update is a package with the same id. |
| `version` | yes | A [semver](https://semver.org) version such as `1.2.0`, at most 64 characters. |
| `name` | yes | At most 40 characters. |
| `description` | yes | At most 300 characters. |
| `author` | yes | `{ "name": …, "url": … }`: a name of at most 60 characters, and an optional `https://` link. |
| `license` | no | Such as `MIT`, at most 60 characters. |
| `repository` | no | An `https://` link. |
| `engines` | yes | `autoscene`: the AutoScene versions it works with. `protocol`: the protocol its program speaks, `1` or `2`; `2` for a service, and for pages that start runs. |
| `platforms` | no | The platforms it runs on. Left out or empty: everywhere it has a command for. |
| `runtime` | yes | `type`: `"process"`, or in version 2 `"service"` or `"none"`. `command`: the command per platform. `gpu`: `true` when it uses the graphics card (default `false`). A plugin of `none` has neither. |
| `permissions` | no | What it says it does (see below). |
| `network` | no | `{ "hosts": [...] }`: where it connects, with the `network` permission. |
| `contributes` | no | Its `exportActions`, `commands`, `settings` and, in version 2, `pages`. |
| `icon` | no | A `.png` or `.svg` file in the plugin. |
| `banner` | no | With `"manifestVersion": 2`: wide art, a `.png`, `.jpg` or `.webp` file in the plugin. |
| `$schema` | no | Allowed, and not read. |

Texts lose control characters and invisible ones. A name or title longer
than its limit is cut, with a warning. A link that isn't a plain `https://`
address (with no `user@` before its host) is left out, with a warning.

**The id** is a reverse domain name such as `dev.example.my-plugin`: 2 to 5
parts joined by dots, each 1 to 39 lowercase letters, digits and dashes,
not starting or ending with a dash. The first part can't be a name Windows
keeps for a device (`con`, `prn`, `aux`, `nul`, `com0` to `com9`, `lpt0` to
`lpt9`). The id names the plugin's folders, so choose it once.

**`engines.autoscene`** is a version requirement as Cargo writes it, with
commas between its parts: `">=1.0.0, <2"`. `">=1.0.0 <2"`, without the
comma, is refused. A pre-release of AutoScene counts as its release:
1.0.3-BETA meets `>=1.0.0` and `>=1.0.3`. A plugin made for a later
AutoScene is refused with "This plugin needs a newer AutoScene.", and one
made only for earlier versions with "This plugin only works with an older
AutoScene."

### runtime.command

A plugin of runtime `none` runs no program: it has no `command`, `gpu` or
export actions and commands ("A plugin whose runtime.type is none runs no
program: remove runtime.command."), works on every platform (its
`platforms` are ignored, with a warning), and needs a page. The other
runtimes need a `command`; a `service` without a page gets the warning "Its
service never starts: a service runs while one of its pages is open, and it
has none."

`command` maps a platform to the command that starts the plugin there. Its
keys are the platforms (`windows-x86_64`, `windows-aarch64`,
`macos-aarch64`, `macos-x86_64`, `linux-x86_64`, `linux-aarch64`) and `*`,
the command for every platform without one of its own. A command is one of
two forms:

- **A uv script**: exactly `["uv", "run", "--script", "--locked",
  "<script>.py"]`. The package must hold the script and the lock `uv lock
  --script <script>.py` makes beside it, `<script>.py.lock`. `--locked`
  makes uv refuse a lock that doesn't match the script's `# /// script`
  block, so the lock decides what uv installs. On the first run uv fetches
  the packages, and a Python when the computer has none it can use; it
  keeps them in its cache.
- **A program in the plugin's folder**: `["<path>", "<argument>", …]`. The
  path is a package path (see [paths](#paths)) that doesn't start with a
  dash. On Windows the program must be an `.exe`: Windows runs a `.bat` or
  `.cmd` through `cmd.exe`. On macOS and Linux it may be a script with a
  `#!` line (but not named `.sh` or `.py`), marked to run.

A command has at most 16 parts, of at most 256 characters each, with no
control or invisible characters. AutoScene refuses:

- the computer's shells, interpreters and script hosts as the program, in
  any case and with or without `.exe`: `python`, `python3`, `pythonw` and
  the like, `uv` in any other form, `uvx`, `py`, `pyw`, `node`, `deno`,
  `bun`, `perl`, `ruby`, `php`, `osascript`, `sh`, `bash`, `zsh`, `dash`,
  `ksh`, `csh`, `tcsh`, `fish`, `env`, `wsl`, `cmd`, `powershell`, `pwsh`,
  `cscript`, `wscript`, `mshta`, `rundll32`, `regsvr32` and `msiexec`. A
  command only ever starts a file in the plugin's folder, so `python3`
  would start a file called `python3` there, not the computer's Python.
- programs whose name ends in `.bat`, `.cmd`, `.ps1`, `.sh` or `.py`, which
  a shell or an interpreter would run. Use the uv form for Python.
- absolute paths, and paths that leave the folder.

A run picks its platform's own command, else `*`. With `platforms` set, a
platform it doesn't name can't install it ("This plugin isn't made for this
computer."), and neither can one with no command. Keep in mind that `*`
covers Windows too: a `*` program that isn't an `.exe` makes the plugin
unusable there, which `asp pack` warns of.

```json
"platforms": ["windows-x86_64", "macos-aarch64", "linux-x86_64"],
"runtime": {
  "type": "process",
  "command": {
    "windows-x86_64": ["bin/frame-guard.exe", "--quiet"],
    "*": ["bin/frame-guard", "--quiet"]
  },
  "gpu": true
}
```

### permissions and network

| Permission | What the review says |
|---|---|
| `exports:read` | Reads the exported clips |
| `exports:write` | Changes or adds files beside the exported clips |
| `project:read` | Reads the project: its episodes and characters |
| `network` | Talks to the internet |

Name what the plugin does. The person sees the list before they install it.
For a plugin that runs a program, it shows under "It runs with your user
rights: it can read, change and send anything you can." and the heading
"What it says it does (not enforced)": AutoScene checks the program's calls
against it, but not what the program does itself. For a plugin of runtime
`none`, it shows under "What its pages may use", with "AutoScene checks
these each time its pages ask." An update that names a permission or a host
the version before didn't marks it "New" in its review. The review also
says how the plugin runs (a uv script fetches Python and its packages on
its first run; a page-only plugin "Runs no program. Its pages run in a
sandbox."; a service "Keeps its program running while its page is open.")
and whether uv is missing, lists its pages and where they show, and shows
the plugin's `description` under "In its own words".

What each permission lets a plugin ask AutoScene for:

- `exports:read`: `exports.list`, `exports.get`, and `files.resolve` of a
  clip or a joined file.
- `project:read`: `project.get`, and `files.resolve` of an episode.
- `network`: `net.fetch`, to the hosts of `network.hosts`.
- `exports:write`: nothing AutoScene answers yet; a program says with it
  that it writes beside the exported clips.

A permission this AutoScene doesn't know refuses the plugin: it may stand
for more than the person agreed to.

`network.hosts` lists at most 16 lowercase host names (`discord.com`; no
scheme, no port), shown under "Talks to the internet". It is ignored, with
a warning, without the `network` permission. In version 2, a host
`net.fetch` can never reach gets a warning (see
[Through AutoScene to the internet](#through-autoscene-to-the-internet)).

### contributes

```json
"contributes": {
  "exportActions": [
    { "id": "post", "title": "Post to Discord", "description": "Posts the export's clips to the channel." }
  ],
  "commands": [
    { "id": "test", "title": "Send a test message", "needsProject": false }
  ],
  "settings": [
    { "key": "webhookUrl", "type": "secret", "title": "Webhook URL" }
  ]
}
```

- **`exportActions`** (at most 8): each an entry in each export's menu, and
  a switch under "After every export" in the plugin's settings. An entry is
  off while its export runs.
- **`commands`** (at most 16): each an entry in the palette's Plugins group.
  With `"needsProject": true` it is offered only inside a project, and runs
  with it; otherwise it runs with the open project, if any.
- **`settings`** (at most 32): see below.
- **`pages`** (version 2, at most 4): see [Pages](#pages).

An action's or a command's `id` is 1 to 32 lowercase letters, digits and
dashes, starting with a letter or digit, and unique among its kind. Its
`title` (required) has at most 60 characters, its `description` at most
200.

### Settings

Each setting has a `key` (a lowercase letter, then letters, digits and `_`,
at most 40 in all), a `type`, a `title` (at most 60 characters) and an
optional `description` (at most 200).

| `type` | Its own fields | Drawn as |
|---|---|---|
| `string` | `default`, `placeholder` (at most 200 characters), `maxLength` (1 to 2000, default 2000) | A text field |
| `number` | `default`, `min`, `max` (at least `min`), `step` (above 0), `integer` (default `false`) | A slider when `max` is above `min`, else a number field |
| `boolean` | `default` (default `false`) | A switch |
| `select` | `options`: 1 to 32 `{ "value": …, "label": … }`, each value unique and at most 200 characters, each label at most 60; `default`, one of the values | Up to 3 choices side by side, else a menu |
| `secret` (version 2) | `placeholder` (at most 200 characters); never a `default` | Whether it is set, with **Set…** and **Remove…** |

```json
{ "key": "threshold", "type": "number", "title": "Threshold", "default": 0.4, "min": 0, "max": 1, "step": 0.05 },
{ "key": "quality", "type": "select", "title": "Quality",
  "options": [{ "value": "fast", "label": "Fast" }, { "value": "best", "label": "Best" }], "default": "best" }
```

A default must fit its setting: within `min` and `max`, whole for an
`integer`, one of the options. A slider without a default starts at its
`min` (the next whole number for an `integer`), and that is its default,
which a run gets too. AutoScene checks each value the person sets
the same way, and keeps only the values that differ from their default: a
setting left alone follows the plugin's default, also after an update. A
stored value that no longer fits after an update gives way to the default.

A token, a password or a webhook URL belongs in a `secret` setting, which
AutoScene keeps in the system's keychain and never hands a run unasked (see
[Secret settings](#secret-settings)). A `string` setting is kept in plain
text in AutoScene's `library.db`, and every run gets it. A version 1
manifest has no secret settings; there, say so where you ask for such a
value, and never write it to the log.

### icon

A `.png` of at most 256 KB, or an `.svg` of at most 64 KB, in the plugin's
folder. It shows in Plugins, in its settings, in the review and beside its
export actions. AutoScene shows it as a picture only (an SVG's scripts
never run). An `icon` that isn't a `.png` or `.svg` path is left out with
a warning. One that isn't in the package, is too large, or doesn't start
as a PNG or an SVG file does refuses the package.

### banner

Wide art for the plugin's card in Plugins and the top of its settings: a
`.png`, `.jpg` (or `.jpeg`) or `.webp` file of at most 512 KB in the
plugin's folder, in a manifest of `"manifestVersion": 2`. Make it 2:1,
1200×600: PNG for flat art, JPEG or WebP for pictures. `asp pack` and
`asp verify` warn of one smaller than 960×480 ("The banner is 800×400.
Cards show it soft below 1200×600.") and of one whose sides are more than
10% off 2:1 ("The banner is 1200×900. Cards crop it to 2:1."). AutoScene
shows it either way.

A version 1 manifest's banner is left out with the warning "Left out the
banner: it needs "manifestVersion": 2.", and one that isn't a `.png`,
`.jpg` or `.webp` path (an SVG, a GIF) with "Left out the banner: it must
be a .png, .jpg or .webp file in the plugin." One that isn't in the
package, is too large, or doesn't start as its format does (the PNG
signature, JPEG's `FF D8 FF`, WebP's `RIFF` and `WEBP`) refuses the
package.

A plugin's pictures are its own files, checked against its `files.lock`
each time they show (a loaded folder's: as they are now, never through a
link). AutoScene loads none from the internet: not its author's picture,
and no colour or art from a link.

### What is refused, and what is skipped

What makes the plugin unusable or could stand for more than the person
agreed to is refused, with a message for the person installing it: a
missing or wrong field, an unknown permission, a key named twice in one
object, a `plugin.json` over 256 KB. What a later AutoScene may add is
skipped with a warning, so a plugin made for one still installs in an
older one: an unknown field, contribution kind or setting type, a page of
a scope this AutoScene doesn't know, a platform this AutoScene doesn't
know, and in a version 1 manifest `pages`, `banner` and secret settings,
which need `"manifestVersion": 2`. A higher `manifestVersion` or
`engines.protocol`, or a `runtime.type` other than `process`, `service`
and `none`, needs a newer AutoScene.

The review lists the warnings under "Skipped in its plugin.json", and so
do the plugin's settings.

## Packages

A package (`.asp`) is a zip of the plugin's files and `files.lock`, which
lists each of them. Hello Export's, with two of its five files:

```json
{
  "lockVersion": 1,
  "files": [
    {
      "path": "main.py",
      "size": 4622,
      "sha256": "4b28e8dafc871cb41f00589e7df8455b244817a2fc3c67f3607ac3854ffc2a9d",
      "exec": false
    },
    {
      "path": "plugin.json",
      "size": 1681,
      "sha256": "43c6080eaa37f8fece80616c027b00e5a0cbf5897a7e841b27124cf8cb5aacd2",
      "exec": false
    }
  ]
}
```

A signed package also holds `files.lock.minisig`, the signature of
`files.lock`, and `author.pub`, the author's public key. The signature
covers the lock, and the lock covers every file.

AutoScene goes by the lock, never by what the zip says of itself. The zip
must hold exactly the listed files (folders only on their way to them),
each of its listed size and SHA-256. Whether a file may run comes from its
`exec`, never from the zip: on macOS and Linux a program unpacks as 0755,
everything else as 0644. Refused too: encrypted entries, links,
compression other than stored or deflate, data before the zip, local
headers that disagree with the zip's directory, and names that differ only
in case.

### Paths

A package path is relative, with `/` between its parts. Each part uses only
ASCII letters, digits and `. _ @ + -`, isn't `.` or `..`, doesn't end in a
dot, and isn't a name Windows keeps for a device (`NUL.txt`, `com1`).
That rules out spaces, `\`, `:` and `~`, so a path names one file on every
system. A path has at most 200 bytes and 12 parts. `files.lock`,
`files.lock.minisig` and `author.pub` are the package's own names at its
top.

### Limits

| What | At most |
|---|---|
| The package | 1 GB |
| Its files, unpacked | 2 GB together, 1 GB each |
| Entries in its zip | 4096 (so 4093 files) |
| Packed size | A file over 1 MB may be packed at most 100 times smaller than it unpacks, and so may all files together. |
| `files.lock` | 1 MB |
| `files.lock.minisig` | 4 KB |
| `author.pub` | 1 KB |
| `plugin.json` | 256 KB |

### Making a package with asp pack

```bash
asp pack <plugin-folder> <out.asp>
```

`asp pack` lists every file in the folder, but the version control folders
(`.git`, `.hg`, `.svn`), `.DS_Store` and `Thumbs.db`, and the editor folders
`asp init` writes (`.autoscene`, `.vscode`), at any depth, and a
`files.lock`, `files.lock.minisig` or `author.pub` at its top. It refuses
a folder that holds a link, a file a package can't name, or a secret key
(`asp.key`, or any file that is a minisign secret key), and a package
written into the plugin's own folder. It checks the manifest, the icon,
the banner (and warns of one cards would show soft or crop) and each
page's file, and checks the plugin for each platform it runs on as
AutoScene would there: it warns of each platform the plugin can't run on,
and fails when that is all of them. It warns of what its manifest's
version skips (pages or a secret setting in version 1, say). A plugin made
for another AutoScene than the one asp comes with gets a warning, not an
error. It checks the package as
AutoScene would before it writes it, so a package AutoScene would refuse
is never written.

It marks a file to run (`"exec": true`) when `plugin.json` starts it on
some platform. On macOS and Linux it also marks every other file its owner
may run there, with the warning "<file> may run on this computer, so the
package marks it to run too." **On Windows a file can't be marked to run,
so a package built there marks only the programs `plugin.json` starts.** A
plugin whose program starts helper programs or scripts of its own on macOS
or Linux needs them marked too: build its package on macOS or Linux, from
a checkout that keeps the files' modes. In WSL, build it from the Linux
file system: files under `/mnt/c` usually all show as runnable.

The same folder gives the same package, byte for byte, on one system:
files go in the lock's order, dated 1980, each deflated, but those deflate
would pack more than 100 times smaller (zero-padded model weights), which
are stored.

## Trust

Who signed a package's `files.lock` decides how far AutoScene trusts it.

| Trust | Its badge | What it means |
|---|---|---|
| Reviewed | Reviewed by AutoScene | An AutoScene Plugins key, compiled into the app, signed this version. |
| Pinned | Signed, not reviewed | Signed with the key in its `author.pub`. The first install pins that key: an update must be signed with it. That key came in the package, so it shows no one's name: the review says so, and names the author as the plugin "says it is by". |
| Unreviewed | Not reviewed | Nobody signed it, or a key AutoScene doesn't know did. |
| | Loaded from a folder | A developer's folder, run as its files are now. It always counts as unreviewed. |

**AutoScene hasn't reviewed any plugin yet**: the app holds no review keys
so far, so every plugin is Pinned or Unreviewed for now. A key a package
brings along never makes it Reviewed.

A plugin AutoScene hasn't reviewed (Pinned, Unreviewed, or loaded from a
folder) needs **Plugins → Allow plugins that AutoScene has not
reviewed**, which is off until the person turns it on, and then the person
types its name in the review before it installs. Turning the setting on,
and each such install, also ask in a dialog of the system's own, which
names the plugin's id, version and key as AutoScene checked them: a script
in AutoScene's page can't say yes for the person. While the setting is
off, such plugins don't run, their actions and pages aren't offered, their
runs stop, their pages close and their services stop. The review can turn
it on, after the same question.

Trust is about who made a plugin, not about what it may do. A page runs in
a sandbox whatever its trust: it gets only what it asks AutoScene for, and
AutoScene checks its permissions each time it asks. A program, a run's or a
service's, still runs with the person's rights, whatever its trust (see
[What AutoScene enforces](#what-autoscene-enforces)).

Signing doesn't make a plugin safe. It makes its updates come from the same
author: a Pinned plugin takes an update only when it is signed with its
pinned key (or reviewed). Anyone else's package for that id is refused with
"This update is signed by a different author than the installed version."
Sign from the first version on.

### Updates

A package with the id of an installed plugin replaces it after its review.
The review shows the version installed, and marks "New" the permissions,
hosts and pages the update adds, a program where the version before ran
none ("The installed version runs no program."), and a service where it
had none ("Now keeps its program running while its page is open.").

- **Trust:** a Reviewed plugin takes only reviewed versions. A Pinned one
  takes versions its pinned key signed, and reviewed ones. An Unreviewed
  one takes any, and a signed update pins its key.
- **Version:** an update must be newer. An older one is refused with
  "Version 1.2.0 is installed, which is newer than 1.1.0. Uninstall it
  first to go back." The same version installs again only with the same
  files, as a repair; with other files it is refused with "Version 1.2.0
  is installed already, with other files. A changed plugin needs a new
  version."
- **What the person set** stays: whether it is on, its settings and
  secrets, its data folder (with what its pages keep), and its actions that
  run after every export (those that still exist).
- **While it runs**, it can't be updated or uninstalled: "<name> is running.
  Wait for it to finish, or stop it in Activity." Its open pages close and
  its service stops for the update.

An install never leaves a plugin half done: the package is unpacked and
checked in a staging folder, checked again, and only then takes the place
of the version before. A review that waits more than 10 minutes expires:
"This plugin waited too long to be installed. Open it again."

The pin goes with an uninstall: installing the plugin again is a first
install.

## What AutoScene enforces

What holds a plugin depends on what runs: a program, or a page.

| | A program (`process`, `service`) | A page (any runtime) |
|---|---|---|
| Its calls into AutoScene | Checked, each one | Checked, each one |
| Files | Its own: whatever the person may read and write | Only `files.resolve` (with a permission), `files.open` and `files.save` (the person's step) |
| The internet | `net.fetch`: checked. Its own connections: not | Only `net.fetch`; the CSP blocks the rest, but for the side channels below |
| The rest of the computer | Whatever the person may do | Nothing: the frame's sandbox |

**What AutoScene checks**, on every call of a page, a run and a service
(see [Host methods](#host-methods)), before it reads anything:

- the params' size;
- whether the caller may call the method (a run never calls a page's own);
- the permissions the person agreed to that the plugin's `plugin.json`
  still asks for, and its hosts and secret settings;
- the runtime the person agreed to: a loaded folder whose `runtime.type`
  changed doesn't run, "Its runtime changed. Load it again to agree to
  that.";
- the project and the export the caller is in.

**What the frame and its CSP block**, for a page: scripts other than its
own files and the kit's, inline scripts and `eval`; every request but its
own files (no `fetch`, XHR, WebSocket, outside picture, font or media, form
submit or navigation away); storage and cookies; windows, frames, workers
and plugin objects; the browser's dialogs; the camera, the microphone, the
location and the clipboard; AutoScene's own commands, and its window
outside the frame. See [The sandbox](#the-sandbox).

**What a page may still send out.** A CSP can't close every channel a
browser engine has. The red-team app (`app/redteam`) tries each known way
out of a page in the real webview, and measures what gets out:

| What a page tries | WebView2 (Windows) |
|---|---|
| WebRTC (`RTCPeerConnection`) with a STUN server it names | Gets out: it gathered the computer's host candidates and sent 4 STUN datagrams to the address it named |
| `<link rel=preconnect>` to an address | Opens a connection, sends nothing |
| `<link rel=prefetch>` of an address | Sends nothing |
| A navigation of its frame that the CSP blocks | Opens a connection, sends nothing |

These are the numbers of its run on Windows 11 with WebView2 (Edge 156),
where every wall it checks held. So a page can send a little to an address
of its choosing through WebRTC: treat what a page may read as something it
could send. The numbers for macOS (WKWebView) and Linux (WebKitGTK) come
from the red team's run in CI on all three systems, and go here once it has
run.

**What nothing checks**: what a plugin's program does itself. It reads,
changes and sends what the person can, whatever its permissions say, and
what it sets up outside AutoScene may outlive it (see
[Cancel and the end of a run](#cancel-and-the-end-of-a-run)). The trust a
plugin has, and the review before it installs, are what stand between a
program and the person's files. A page's state file, which a program may
read and must not write, is a rule nothing enforces either (see
[What a page keeps](#what-a-page-keeps)).

## Signing with asp

```text
asp keygen <dir> [--force] [--unencrypted]
asp pack <plugin-folder> <out.asp>
asp sign <in.asp> --key <asp.key> [--out <out.asp>]
asp verify <in.asp> [--review-key <RW…>]…
asp init <dir> [--page | --service | --action] [--id <id>] [--name <name>]
```

`asp init` is under [Developing a plugin](#developing-a-plugin).

1. **Make a key once**, outside any plugin folder:

   ```bash
   asp keygen ~/autoscene-keys
   ```

   It asks for a password, and writes `asp.key` (the secret key, locked with
   that password; an empty one is refused) and `author.pub` (the public
   key). It prints the key's id, as AutoScene shows it in the review. It
   replaces neither file unless `--force`, and refuses a folder that holds
   a `plugin.json`. `--unencrypted` writes a key without a password, for
   CI; keep that one in a CI secret.
2. **Sign each package** after `asp pack`:

   ```bash
   asp sign my-plugin-1.0.0.asp --key ~/autoscene-keys/asp.key
   ```

   It asks for the password, and adds `files.lock.minisig` and `author.pub`
   to the package (in place, or into `--out`). The signature's trusted
   comment names the plugin, its version and the lock's SHA-256:
   `autoscene-plugin;id=<id>;version=<version>;lock=<sha256>`. A signature
   is good for that one version of that one plugin only.
3. **Check a package** as AutoScene would:

   ```bash
   asp verify my-plugin-1.0.0.asp
   dev.yourname.my-plugin 1.0.0: Pinned (key 4B2D9C1E0F3A7E51)
   ```

   It prints Reviewed, Pinned with the key's id, or Unreviewed, with the
   warnings AutoScene would show. `--review-key` adds a review key to the
   app's own, to test a review signature.

The keys and the signature are minisign's formats; asp signs the prehashed
way, which is all AutoScene takes.

**Keep `asp.key` and its password safe, and keep a copy.** Without it,
nobody can sign an update that your plugin's users will take: they would
have to uninstall the plugin and install a package signed with a new key.
`asp keygen --force` warns of this when it replaces a key. A secret key in
a plugin's folder would let anyone sign as you, so `asp pack` refuses one.

## Developing a plugin

- **Start with `asp init`**:

  ```bash
  asp init my-plugin [--page | --service | --action] [--id dev.yourname.my-plugin] [--name "My Plugin"]
  ```

  It makes a plugin in a new folder (or an empty one) that packs, loads and
  runs as it is:
  - `--page` (the default): version 2, runtime `none`, one page in the
    rail (`ui/index.html`, `ui/app.js`) that keeps a note and says so in a
    toast.
  - `--service`: runtime `service`, protocol 2: the page asks the service
    (`main.py`, with `autoscene.py` and `main.py.lock`) to count words.
  - `--action`: a process plugin of protocol 2 with the export action
    "Count the clips" (`main.py`, `autoscene.py`, `main.py.lock`).

  Each form also writes a `README.md` with the next steps,
  `.autoscene/plugin.schema.json` and `.vscode/settings.json`, so the
  editor checks `plugin.json` as it is typed. The page forms write the SDK's
  types to `.autoscene/types/`. `asp pack` leaves `.autoscene` and
  `.vscode` out. The id is `dev.example.<folder>` unless `--id` says
  otherwise, and the name the folder's.
- **Load the folder** (Plugins → Load a folder…, while "Allow
  plugins that AutoScene has not reviewed" is on). AutoScene runs the
  plugin where it is, without a lock or a package: a change to a file
  counts at the next run, and a page's files are served as they are now.
  It reads the folder's `plugin.json` again each time it lists the
  plugins, so turning the plugin off and on shows a new action, setting or
  page. A permission or a host it asks for since, or a changed
  `runtime.type`, counts only once the folder is loaded again. A uv script
  still needs its `.lock`.
- **Reload a page** from its ⋯ menu (**Reload the page**), or a right-click
  on it, to load what you changed. Its service runs on: turn the plugin off
  and on to start it again.
- **Open a page outside AutoScene**, in a plain browser, while you make it:
  `AutoScene.connect({ mock })` then answers from an in-memory host instead,
  as AutoScene would, with the same refusals for what the plugin may not
  use. `mock` names what the page sees: `permissions`, `hosts`,
  `settings`, `secrets`, `project`, `exports`, `projectId`, `service`, and
  `answer` (an answer of your own for any method). The page needs
  `/_as/sdk.js` and `/_as/theme.css` beside it there: `npm run build:kit`
  in a checkout of AutoScene's repository writes them to `app/dist/_as/`.
- **The app's own CSP applies only in a built app. The page's CSP applies
  everywhere.** AutoScene started for development (`tauri dev`) leaves its
  own page without a CSP, but every answer to a page carries the page's,
  in a built app and in development alike. Check how your page behaves in
  an installed AutoScene before you share it.
- **The log**: right-click the plugin in Plugins, then **Show its
  log**. Its settings name its folder and its log too. Each run
  starts with `--- <time> run <runId> <action>`. The plugin's lines are
  marked `[stdout]`, `[stderr]` and `[<level>]` for its log lines;
  AutoScene's own are marked `[host]`, `[result]` and `[failed]`, with the
  exit status at the end, and a call and its answer `[call]` and `[reply]`.
  A log over 1 MB is moved to `<id>.log.1` when the next run starts. A
  service has a log of its own (see [Services](#services)).
- **AutoScene's own log**, `logs/app.log` in its data folder, has the
  detail of each failed run.
- A loaded folder can't take the place of a version that AutoScene reviewed
  or that its author signed: uninstall that one first. A folder inside
  AutoScene's own `plugins` folder can't be loaded.
- Nor can a folder take the place of an installed package of the same
  version, or of a newer one. To go back to the folder after you installed
  its package to try it, uninstall the package, or raise the folder's
  version.
- Uninstalling a loaded folder makes AutoScene forget it and its settings;
  the folder stays as it is.

## Where AutoScene keeps plugins

AutoScene's data folder is `%LOCALAPPDATA%\AutoScene` on Windows,
`~/Library/Application Support/AutoScene` on macOS and
`~/.local/share/AutoScene` on Linux (`AUTOSCENE_DATA_DIR` moves it).

| Path in the data folder | What |
|---|---|
| `plugins/<id>/<version>+<8 hex>/` | An installed package's files, with its `files.lock` (and signature). |
| `plugin-data/<id>/` | The plugin's own files; each run and its service start there. |
| `plugin-data/<id>/page-state.json` | What `state.*` keeps (see [What a page keeps](#what-a-page-keeps)). |
| `logs/plugins/<id>.log` | Its log, and the one before in `<id>.log.1`. |
| `logs/plugins/<id>.service.log` | Its service's log, and the one before in `<id>.service.log.1`. |
| `library.db` | In the `settings` table: its record (`plugin.<id>`), the settings the person changed (`plugin_settings.<id>`), the runtime the person agreed to and the names of the secrets set (`plugin_extra.<id>`), and the id the keychain keeps this data folder's secrets under (`plugins_keychain`). |

The values of secret settings are in the system's keychain, not in the
data folder (see [Secret settings](#secret-settings)). An uninstall
removes the plugin's files, its rows and its secrets; its data folder,
with `page-state.json`, only when the person asks (**Delete what it kept
too**). AutoScene 1.0 doesn't know `plugin_extra.<id>` or the keychain: a
plugin uninstalled there leaves both, and a newer AutoScene removes them
when it starts.

AutoScene checks a package's files when it installs them. Afterwards it
checks `files.lock` and `plugin.json` each time it lists the plugin; one
that changed shows "Its files were changed. Install it again." Leave the
`plugins` folder to AutoScene: when it starts, it removes the plugin
folders it has no record of.

## Not in this version

- **A catalog** to browse, review, install and update plugins in the app,
  and the review keys that come with it.
- **WebAssembly plugins**: `runtime.type` `wasm`, a plugin whose
  permissions are enforced for its program too, needs a newer AutoScene.
- **Shortcuts** for plugin commands.
- **WebAssembly and workers inside pages**: a page's CSP allows neither.
- **An npm package of the SDK**: a page gets `sdk.js` and `theme.css` from
  AutoScene at `/_as/`, and their types from `asp init`.
- **A light theme for pages**: a page gets AutoScene's dark theme only.
