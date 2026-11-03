# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Discord Notify: an example AutoScene plugin that talks to the internet.

It posts an export's name and clips to a Discord channel through the
channel's webhook. The webhook's URL is a secret setting: AutoScene keeps
it in the system's keychain and hands it over only when the script asks
(secrets.get). The post goes through AutoScene (net.fetch), which reaches
only the hosts plugin.json names. The script never writes the URL to its
log or its messages. autoscene.py, beside it, speaks the protocol;
docs/PLUGINS.md describes it.
"""

import json
import re
import sys
from urllib.parse import urlsplit

import autoscene

# Where a webhook may lead: the hosts plugin.json names, which net.fetch
# keeps to as well. Checked here first, to say so in plain words.
HOSTS = {"discord.com", "ptb.discord.com", "canary.discord.com", "discordapp.com"}
# A post holds at most 2000 characters.
MAX_POST = 1900


class Refused(Exception):
    """Why the run can't post: its message for the person, and the detail for the log."""

    def __init__(self, message: str, detail: str | None = None):
        super().__init__(message)
        self.message, self.detail = message, detail


def main() -> int:
    p = autoscene.start()
    action, settings = p.first["action"], p.settings
    try:
        if action["kind"] == "command" and action["id"] == "test":
            post(p, webhook(p), settings, "AutoScene can post here.")
            p.result("Posted a test message to Discord.")
        elif action["kind"] == "exportAction" and action["id"] == "post":
            content = describe(p.first["export"], settings.get("listClips", True))
            post(p, webhook(p), settings, content)
            p.result("Posted the export to Discord.")
        else:
            raise Refused(f"Discord Notify has no action {action['id']}.")
    except Refused as e:
        p.error(e.message, e.detail)
        return 1
    return 0


def webhook(p: autoscene.Plugin) -> str:
    """The webhook URL from the keychain, once it is one of Discord's."""
    try:
        url = (p.call("secrets.get", {"key": "webhookUrl"})["value"] or "").strip()
    except autoscene.CallError as e:
        # Its message says why (a locked keychain, say), never the value.
        raise Refused(str(e), e.code)
    if not url:
        raise Refused("Set the webhook URL in Discord Notify's settings first.")
    try:
        parts = urlsplit(url)
        port = parts.port
    except ValueError:
        parts, port = None, None
    if (
        parts is None
        # A space, a line break or a letter outside ASCII.
        or any(not "!" <= c <= "~" for c in url)
        or parts.scheme != "https"
        or parts.hostname not in HOSTS
        or parts.username is not None
        or port not in (None, 443)
        # /api/webhooks/, or a version's /api/v10/webhooks/.
        or not re.match(r"/api/(v\d+/)?webhooks/", parts.path)
    ):
        raise Refused("That webhook URL isn't a Discord webhook. Copy it again from the channel's settings.")
    return url


def describe(export: dict, list_clips: bool) -> str:
    """The post for an export: its name, how many clips it made, and which."""
    made = [c for c in export["clips"] if c["status"] == "done"]
    missing = len(export["clips"]) - len(made)
    count = "1 clip" if len(made) == 1 else f"{len(made)} clips"
    head = f"**{plain(export['label'] or 'An export')}** is done: {count} with {plain(export['preset'])}."
    if missing:
        head += " 1 wasn't made." if missing == 1 else f" {missing} weren't made."
    lines = [head]
    if list_clips:
        for i, clip in enumerate(made):
            line = f"- {plain(clip['name'])} · {plain(clip['episode'])}"
            if sum(len(l) + 1 for l in lines) + len(line) > MAX_POST:
                lines.append(f"…and {len(made) - i} more.")
                break
            lines.append(line)
    return "\n".join(lines)


def plain(text: str) -> str:
    """Text as Discord shows it, without its Markdown: names may hold * or _."""
    return re.sub(r"([\\*_~`|>])", r"\\\1", text)


def post(p: autoscene.Plugin, url: str, settings: dict, content: str) -> None:
    """Posts `content` to the webhook through AutoScene. Mentions in it never ping anyone."""
    if p.cancelled.is_set():
        sys.exit(1)
    p.progress(None, "Posting to Discord")
    body = {"content": content, "allowed_mentions": {"parse": []}}
    if settings.get("username"):
        body["username"] = settings["username"]
    request = {
        "url": url,
        "method": "POST",
        "headers": [["content-type", "application/json"]],
        "body": json.dumps(body),
        "timeoutMs": 20000,
    }
    try:
        answer = p.call("net.fetch", request)
    except autoscene.CallError as e:
        if e.code == "cancelled":
            sys.exit(1)
        # AutoScene's words name the host at most, never the URL.
        raise Refused(f"Couldn't post to Discord: {e}", e.code)
    status = answer["status"]
    if 200 <= status < 300:
        return
    if status in (401, 404):
        raise Refused("Discord doesn't know that webhook. Copy it again from the channel's settings.", f"HTTP {status}")
    if status == 429:
        wait = dict(answer["headers"]).get("retry-after", "a few")
        raise Refused(f"Discord asks to wait {wait} seconds. Try again then.", f"HTTP {status}")
    raise Refused("Discord refused the post.", f"HTTP {status}: {(answer.get('body') or '')[:500]}")


if __name__ == "__main__":
    sys.exit(main())
