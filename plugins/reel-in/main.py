# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "yt-dlp[default,curl-cffi]==2026.10.26",
#     "deno==2.9.8; sys_platform != 'win32' or platform_machine != 'ARM64'",
# ]
# ///
"""Reel In: a source of AutoScene that downloads YouTube and TikTok videos.

AutoScene runs this once per link the person adds (docs/PLUGINS.md,
"Sources"). The run line names the link, the folder the finished videos go
to (outDir) and a folder for what isn't finished yet (partDir), which
AutoScene removes when the run ends. Each finished video is named with a
file line, and AutoScene adds it to the project at once.

yt-dlp does the downloading. It is pinned by the lock beside this script,
never updated by itself, and fetches no code of its own: YouTube's player
script runs in Deno, which yt-dlp starts without any permission (no files,
no network). AutoScene's FFmpeg joins video and sound.
"""

import re

#: The qualities the setting offers: the most a download takes, by height.
QUALITIES = {"best": None, "2160": 2160, "1440": 1440, "1080": 1080, "720": 720, "480": 480}

#: The most videos one playlist (or profile) link gives.
MAX_PLAYLIST = 100

#: How a finished video is named: its title, cut to 80 bytes so the path
#: stays within what Windows takes, and its id, which tells two videos of
#: one title apart and makes a second download of it find the first.
#: A playlist's videos start with E and their place in it, so AutoScene
#: orders them as episodes.
NAME = "%(playlist_index&E{:03d} - |)s%(title).80B [%(id)s].%(ext)s"

#: What a download skips: a stream that is live or yet to come. A video that
#: doesn't say (TikTok's) passes: `?` lets a missing field through.
LIVE_FILTER = "!is_live & live_status!=?is_upcoming"


def format_for(quality: str) -> str:
    """yt-dlp's format for `quality`: the best video up to its height with the
    best sound, else the best single file up to it, else (a video that is
    smaller than any of those) the best there is."""
    height = QUALITIES.get(quality)
    if height is None:
        return "bv*+ba/b"
    return f"bv*[height<={height}]+ba/b[height<={height}]/bv*+ba/b"


#: What a failure says, by what yt-dlp's message holds (in lower case), in turn.
ERRORS = [
    (("private video", "this video is private"), "This video is private."),
    (("confirm your age", "age-restricted", "age restricted", "inappropriate for some users"),
     "YouTube wants you signed in to watch this video. Reel In can't download it."),
    (("not a bot",), "YouTube asked to check that you're not a bot. Try again later."),
    (("members-only", "join this channel", "members only"), "This video is for the channel's members only."),
    (("not available in your country", "geo restricted", "geo-restricted", "blocked it in your country"),
     "This video isn't available in your country."),
    (("is_live", "live event", "is live", "premieres in", "is_upcoming"),
     "Live streams can't be downloaded. Try again once it has ended."),
    (("unsupported url",), "Reel In can't download from that link."),
    (("video unavailable", "has been removed", "no longer available", "account associated", "terminated",
      "does not exist", "not found", "404"), "This video isn't there any more."),
    (("no space left", "errno 28", "disk full", "not enough space"), "The drive is full."),
    (("429", "too many requests", "rate-limit", "rate limit"), "The site asked to slow down. Try again later."),
    (("timed out", "getaddrinfo", "connection", "unable to download webpage", "network is unreachable",
      "name or service not known", "ssl"), "Couldn't reach the site. Check the internet connection."),
]


def plain_error(message: str) -> str:
    """What the person reads for yt-dlp's `message`."""
    low = message.lower()
    for marks, said in ERRORS:
        if any(m in low for m in marks):
            return said
    return "The download failed."


#: The id at the end of a name made by NAME: "… [dQw4w9WgXcQ].mkv".
ID_IN_NAME = re.compile(r"\[([^\]\[]+)\]\.[A-Za-z0-9]+$")


def id_of(path: str) -> str | None:
    """The video id in the name of a file NAME made, if it has one."""
    m = ID_IN_NAME.search(path)
    return m.group(1) if m else None


def fraction(done: float | None, total: float | None, part: int, parts: int, index: int, count: int) -> float | None:
    """How far a run is: `done` of `total` bytes of the `part`-th of `parts`
    downloads of a video (its picture weighs 9 times its sound), the
    `index`-th of `count` videos of the link, all counted from 1."""
    if not total or done is None:
        return None
    within = min(1.0, done / total)
    if parts > 1:
        weights = [0.9] + [0.1 / (parts - 1)] * (parts - 1)
        within = sum(weights[: part - 1]) + weights[part - 1] * within
    count = max(1, count)
    return max(0.0, min(1.0, ((max(1, index) - 1) + within) / count))


def stage(title: str | None, height: int | None) -> str:
    """What Activity says while a video downloads: "“Title” · 1080p"."""
    name = f"“{title}”" if title else "A video"
    return f"{name} · {height}p" if height else name


def main() -> None:
    import autoscene

    p = autoscene.start()
    run = p.first
    source = run.get("source") or {}
    link, out_dir, part_dir = source.get("link"), source.get("outDir"), source.get("partDir")
    if not (link and out_dir and part_dir):
        p.error("Reel In runs on links added with Add links….", "The run line has no source.")
        return

    import yt_dlp
    from yt_dlp.utils import DownloadCancelled, DownloadError

    quality = str(p.settings.get("quality", "best"))
    titles: dict[str, str] = {}
    told: set[str] = set()
    # yt-dlp's last error: with a playlist's broken videos skipped, it says
    # why a link gave none.
    errors: list[str] = []

    class Log:
        """yt-dlp's words go to the plugin's log, never to stdout."""

        def debug(self, msg: str) -> None:
            if "does not pass filter" in msg:
                # A live stream, skipped: why the link gave no video.
                errors.append(msg)
            if not msg.startswith("[debug] "):
                p.log(msg, "debug")

        def info(self, msg: str) -> None:
            p.log(msg, "info")

        def warning(self, msg: str) -> None:
            p.log(msg, "warn")

        def error(self, msg: str) -> None:
            errors.append(msg)
            p.log(msg, "error")

    def on_progress(d: dict) -> None:
        if p.cancelled.is_set():
            raise DownloadCancelled("Stopped.")
        info = d.get("info_dict") or {}
        if info.get("id") and info.get("title"):
            titles[info["id"]] = info["title"]
        if d.get("status") != "downloading":
            return
        wanted = [f.get("format_id") for f in info.get("requested_formats") or []]
        part = wanted.index(info.get("format_id")) + 1 if info.get("format_id") in wanted else 1
        f = fraction(
            d.get("downloaded_bytes"),
            d.get("total_bytes") or d.get("total_bytes_estimate"),
            part,
            max(1, len(wanted)),
            int(info.get("playlist_index") or 1),
            int(info.get("n_entries") or 1),
        )
        p.progress(f, stage(info.get("title"), info.get("height")))

    def on_postprocess(d: dict) -> None:
        if p.cancelled.is_set():
            raise DownloadCancelled("Stopped.")
        if d.get("status") == "started" and d.get("postprocessor") == "Merger":
            title = (d.get("info_dict") or {}).get("title")
            p.progress(None, f"Joining video and sound of “{title}”" if title else "Joining video and sound")

    def finished(path: str) -> None:
        # The last step for each video, once it is whole in outDir; also for
        # one a run before already downloaded.
        if path in told:
            return
        told.add(path)
        vid = id_of(path)
        p.file(path, titles.get(vid) if vid else None)

    options = {
        "format": format_for(quality),
        # Standard dynamic range first: what editors and AutoScene's own
        # exports expect. A video in HDR only still comes.
        "format_sort": ["hdr:SDR"],
        "merge_output_format": "mkv",
        "paths": {"home": out_dir, "temp": part_dir},
        "outtmpl": {"default": NAME},
        "windowsfilenames": True,
        "noplaylist": True,
        "playlistend": MAX_PLAYLIST,
        "match_filter": yt_dlp.utils.match_filter_func(LIVE_FILTER),
        "ffmpeg_location": (run.get("tools") or {}).get("ffmpeg") or None,
        "progress_hooks": [on_progress],
        "postprocessor_hooks": [on_postprocess],
        "post_hooks": [finished],
        "logger": Log(),
        "quiet": True,
        "noprogress": True,
        "no_color": True,
        "continuedl": False,
        "overwrites": False,
        "ignoreerrors": "only_download",
        "retries": 5,
        "fragment_retries": 5,
        "socket_timeout": 30,
    }
    try:
        with yt_dlp.YoutubeDL(options) as ydl:
            code = ydl.download([link])
    except DownloadCancelled:
        p.error("Stopped.")
        return
    except DownloadError as e:
        p.error(plain_error(str(e)), str(e))
        return
    if p.cancelled.is_set():
        p.error("Stopped.")
    elif not told and errors:
        p.error(plain_error(errors[-1]), errors[-1])
    elif not told:
        p.error("Reel In found no video it could download at that link.", f"yt-dlp ended with {code}.")
    else:
        count = len(told)
        p.result(f"Downloaded {count} video." if count == 1 else f"Downloaded {count} videos.")


if __name__ == "__main__":
    main()
