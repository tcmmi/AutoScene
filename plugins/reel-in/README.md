# Reel In

Add YouTube and TikTok videos to an AutoScene project by their links,
instead of video files.

In **New project**, choose **Add links…**, paste one link or many (one per
line), and pick a quality: Best, 4K, 1440p, 1080p, 720p or 480p. Create the
project: it opens at once, each link downloads in Activity, and each video
joins the project, and is analysed, as soon as it is done. A project's
Overview and Episodes have **Add links…** too.

- **Links it takes:** youtube.com (also Shorts and `m.` and `music.`
  links), youtu.be, tiktok.com (also `vm.` and `vt.` short links). A
  playlist or profile link gives up to 100 videos, numbered in their order
  (`E001 - …`). A video link with a playlist in it gives that one video.
- **Quality:** Best takes the highest the video has; the others are the
  most it takes. Standard dynamic range comes first; a video in HDR only
  still comes. Video and sound are joined into an MKV file with
  AutoScene's FFmpeg, as they are: nothing is re-encoded.
- **Where videos go:** the Downloads folder of your AutoScene library,
  in a folder named after the project. Removing the project leaves them.
- **What it can't do:** live streams, videos that need you signed in
  (age-restricted, members only), and videos the site blocks for this
  computer. Each says why when it fails.

Download only videos you have the right to use.

## How it works

`main.py` is a [uv](https://docs.astral.sh/uv/) script. AutoScene runs it
once per link (docs/PLUGINS.md, "Sources"); it downloads with
[yt-dlp](https://github.com/yt-dlp/yt-dlp), pinned by `main.py.lock`, and
names each finished video with a `file` line. yt-dlp never updates itself
here and fetches no code: YouTube's player script runs in
[Deno](https://deno.com), which yt-dlp starts with no permissions (no
files, no network). Deno comes from PyPI with the lock; it has no build
for Windows on ARM, where YouTube works only partly.

The first run downloads Python (when the computer has none), yt-dlp, Deno
and their packages: about 50 MB.

## Developing

```bash
python -m unittest test_main.py         # its own logic, no download
uv lock --script main.py                 # after changing the # /// script block (uv 0.13.7 or older)
asp pack . reel-in-1.0.0.asp && asp verify reel-in-1.0.0.asp
```

YouTube changes often; a new yt-dlp is a new version of this plugin: raise
the pin in `main.py`, lock again, raise `version` in `plugin.json`.
