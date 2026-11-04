# AutoScene

Frame-exact character scenepacks from series and movies. Add a season, let
AutoScene cut it into shots and find who is on screen, then export every
scene of the characters you choose as editing-ready clips.

## Download

Get the newest version from
[Releases](https://github.com/tcmmi/AutoScene/releases/latest).

| System | File |
| --- | --- |
| Windows 10 and 11 (x64) | `AutoScene_<version>_x64-setup.exe` |
| macOS 13 or later on Apple Silicon | `AutoScene_<version>_aarch64.dmg` |
| Linux (x64) | `.AppImage`, `.deb` or `.rpm` |

The builds aren't signed by a publisher yet, so the first start needs your OK
once:

- **Windows:** SmartScreen may say it protected your PC. Choose **More info →
  Run anyway**. (With Smart App Control on, Windows 11 offers no such choice:
  it blocks unsigned apps it doesn't know.) The installer fetches WebView2 if
  it's missing.
- **macOS:** drag AutoScene from the disk image to Applications and open it.
  macOS says it can't verify the app: close the message. Then open **System
  Settings → Privacy & Security**, scroll down to the note about AutoScene,
  choose **Open Anyway** and confirm. (Since macOS 15, right-click → Open no
  longer does this.) If macOS calls the app damaged (versions up to 1.0.1
  aren't signed) or later refuses a part of it, such as
  `libonnxruntime.dylib`, run
  `xattr -dr com.apple.quarantine /Applications/AutoScene.app` once.
- **Linux:** make the AppImage executable (`chmod +x AutoScene_*.AppImage`).
  WebKitGTK 4.1 is required for the `.deb` and `.rpm` packages.

FFmpeg and ONNX Runtime ship with the app. The AI models (about 310 MB)
download the first time an analysis needs them.

### Graphics

Analysis runs on the graphics card through DirectML on Windows (any
DirectX 12 card) and on the Apple chip through Core ML on macOS; on Linux
it runs on the processor. **Settings → General → Hardware acceleration**
shows what it runs on. With two or more cards on Windows you can choose one
there, or the processor only, which is much slower; the choice applies from
the next analysis. A Mac keeps what Core ML compiled for the next start
(a compile unused for 30 days goes), so on a Mac a model waits for its compile once, and again after an
update that changes the model or the runtime.

**NVIDIA acceleration** is optional, for Windows (x64) with a GeForce RTX 30
series card or newer. Settings offers NVIDIA's engine, when analysis runs on that card, once you
agree to NVIDIA's licence there: about 111 MB from the Python Package Index
(which sees your IP address), 286 MB on disk, plus what it compiles as you use it. It speeds up the analysis of
anime. Live-action analysis (Fast Scan included) and shots don't use it. AutoScene neither
bundles nor hosts NVIDIA's files, and checks each against a SHA-256 pinned
in the release before it is used. If the engine doesn't load or a job ends
badly under it, analysis goes on without it, on DirectML. A switch turns it
off, and **Remove NVIDIA's files** deletes them and what they compiled.
Nothing changes for anyone who doesn't download it.

### Check a download

Releases after 1.0.1 come with a signed record for every file (a build
attestation): the workflow of this repository that built it, and the tag and
commit it was built from. To check a file, use the
[GitHub CLI](https://cli.github.com), signed in with any GitHub account
(`gh auth login`):

```bash
gh attestation verify AutoScene_<version>_x64-setup.exe -R tcmmi/AutoScene
```

It shows the workflow and the tag. To insist on the release workflow, add
`--signer-workflow tcmmi/AutoScene/.github/workflows/release.yml`.

They also have a `SHA256SUMS` file with the SHA-256 of every other file. Put
it next to your downloads and run
`sha256sum --check --ignore-missing SHA256SUMS` (Linux) or
`shasum -a 256 --check --ignore-missing SHA256SUMS` (macOS). In PowerShell
on Windows, this prints `True` for an intact download:

```powershell
(Get-FileHash .\AutoScene_<version>_x64-setup.exe).Hash -eq (-split (Select-String -Path SHA256SUMS -Pattern x64-setup.exe -SimpleMatch).Line)[0]
```

## What it does

- **A library of series and movies**, live action and anime. Episodes are
  numbered from their file names, and moved or renamed files can be found
  again.
- **Frame-exact everywhere.** Every frame is indexed from the decoder itself,
  and each export is checked against the source frames.
- **Shots.** A shot detector finds every cut. Transition frames belong to no
  shot, so clips never start mid-dissolve.
- **Characters.** Faces are detected, tracked and grouped into characters
  automatically. You name them, and you review the faces the analysis wasn't
  sure about with one key each. Your decisions survive every later analysis:
  they go back onto the faces it finds again. One whose face it doesn't find
  waits for the next analysis, and the cast says how many wait. Projects made
  as anime use their own pipeline (anime head detectors and a character
  embedder) and, with the AniList setting on, get their names looked up on
  AniList once the characters are found: you give them with one click.
- **Scenepacks.** For each character you get their shots, filters and your own
  picks, then one export: a single video with a chapter for each shot, or a
  file per shot, named from a template.
- **A player for editors.** It has J/K/L shuttle, frame stepping, in and out
  points, and a zoomable timeline with shots and character lanes.

## Build from source

You need:

- **Rust:** the nightly toolchain pinned in `rust-toolchain.toml`. `rustup`
  installs it.
- **Node:** version 26.
- **Windows:** Visual Studio with the C++ tools.
- **Linux:** the WebKitGTK development packages and `patchelf`. See `.github/workflows/ci.yml`.

```bash
cargo xtask fetch-sidecars        # pinned FFmpeg + ONNX Runtime into app/src-tauri
cd app
npm ci
npm run tauri:dev                 # the app with hot reload
npm run tauri:build               # installers in target/release/bundle
```

Tests:

```bash
cargo xtask fixtures              # synthetic test media
cargo xtask wasm-guests           # the WebAssembly plugins the sandbox tests run
cargo test --workspace --features as-plugin/cli
cd app && npm test
```

## Layout

| Path | What |
| --- | --- |
| `crates/` | The Rust core: media and frame index (`as-media`), projects and database (`as-project`, `as-db`), analysis engine and inference (`as-engine`, `as-infer`, `as-analysis`, `as-identity`), exports (`as-export`), plugins and the `asp` tool (`as-plugin`) |
| `app/` | The desktop app: Tauri shell (`app/src-tauri`) and React UI (`app/src`) |
| `app/redteam/` | A test app that tries every known way out of a plugin page |
| `xtask/` | Dev tasks: sidecars, fixtures, test models |
| `models/manifest.toml` | The AI models, pinned by URL and SHA-256 |
| `tools/model-prep/` | Dev-only Python that prepares models (never shipped) |
| `docs/DESIGN.md` | The design system |
| `docs/PLUGINS.md` | How to write, package and sign a plugin |
| `docs/CATALOG.md` | AutoScene's plugin catalog: its signed index, its keys, `asp catalog` and the runbook |
| `SECURITY.md` | How to report a security hole privately, and what counts |
| `examples/plugins/` | Seven example plugins: export actions (`hello-export`, and `discord-notify` with a secret, `net.fetch` and an event), a page (`hello-page`), a service (`hello-service`), a shot filter with a pack action (`bright-shots`), and two that run in the sandbox as WebAssembly (`discord-notify-wasm` in Rust, `clip-list-py` in Python) |

`src/autoscene/` and `ui/` hold AutoScene 0.1, the Python-based version. They
stay until this version also has its dialogue search, smart tags and frame
interpolation.

## Models and licences

The full face pass's models (SCRFD and ArcFace, with the people models)
come from InsightFace and may only be used non-commercially; Fast Scan's
(YuNet, SFace) are MIT and Apache-2.0. Anime's head and person detectors
(deepghs) are MIT; its character embedder, CCIP, is under OpenRAIL, whose
use restrictions apply. The shot detector, TransNetV2, is MIT-licensed. **Settings → About** shows each model's licence. `models/manifest.toml` lists
the source and licence of each model.
