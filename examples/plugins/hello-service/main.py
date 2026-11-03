# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Hello service: an example AutoScene plugin whose program is a service.

AutoScene starts this script with `uv run --script --locked main.py` when
the plugin's page opens, and stops it a minute after the last one closed.
The first line on stdin is the serve line. The script says it is ready,
then answers each request of its page, until AutoScene says stop.
autoscene.py, beside it, speaks the protocol; docs/PLUGINS.md describes it.
"""

import sys

import autoscene


def main() -> int:
    p = autoscene.start()
    if p.mode != "serve":
        p.error("Hello service has nothing to run. Open its page.")
        return 1
    total = 0

    @p.handle("count")
    def count(params, context):
        """Counts the words of the page's text, and tells each of its open pages."""
        nonlocal total
        text = (params or {}).get("text")
        if not isinstance(text, str):
            raise ValueError("Give it some text to count.")
        words = len(text.split())
        total += words
        p.emit("counted", {"words": words, "total": total})
        return {"words": words}

    p.log(f"Serving its page in AutoScene {p.first['app']['version']}.")
    p.serve()
    return 0


if __name__ == "__main__":
    sys.exit(main())
