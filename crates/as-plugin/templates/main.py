# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""The plugin's program.

AutoScene starts it with `uv run --script --locked main.py`, in one of two
modes. A run does one thing and ends: an export action from an export's
menu, a palette command, or what a page asked for with actions.run. A
service (runtime type "service") keeps running while one of the plugin's
pages is open, and answers what they ask with service.request.

autoscene.py, beside it, speaks AutoScene's protocol. After a change to the
`# /// script` block above, lock it again: `uv lock --script main.py`.
"""

import sys

import autoscene


def run(p: autoscene.Plugin) -> int:
    """Says how many clips the export wrote."""
    export = p.first.get("export")
    if export is None:
        p.error("Run it from an export's menu.")
        return 1
    p.progress(None, "Counting the clips")
    written = [clip for clip in export["clips"] if clip.get("path")]
    p.result("The export wrote 1 clip." if len(written) == 1 else f"The export wrote {len(written)} clips.")
    return 0


def serve(p: autoscene.Plugin) -> int:
    """Counts the words of what a page sends, until AutoScene says stop."""

    @p.handle("count")
    def count(params, context):
        text = (params or {}).get("text")
        if not isinstance(text, str):
            raise ValueError("Give it some text to count.")
        return {"words": len(text.split())}

    p.serve()
    return 0


def main() -> int:
    p = autoscene.start()
    return serve(p) if p.mode == "serve" else run(p)


if __name__ == "__main__":
    sys.exit(main())
