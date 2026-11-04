"""AutoScene's protocol 2 for a plugin's program, in the standard library.

    import autoscene

    p = autoscene.start()  # reads the first line: "run" or "serve"
    if p.mode == "run":
        hook = p.call("secrets.get", {"key": "webhook"})["value"]
        p.progress(0.5, "Posting")
        p.result("Posted 3 clips.")
    else:
        @p.handle("count")
        def count(params, context):
            return {"words": len(params["text"].split())}

        p.serve()  # answers requests until AutoScene says stop

A thread reads what AutoScene says after the first line: the replies to
calls, a service's requests, its settings, and a cancel or a stop. A
service answers its requests one at a time, in the order they came. An
exception in a handler answers its request with the exception's message,
and so does an answer JSON can't hold (a set, NaN); its traceback goes to
the plugin's log. docs/PLUGINS.md describes the protocol itself.

Only this helper reads and writes AutoScene's lines. From start() on, a
print(), a C library and a child process that writes to stdout all reach
the plugin's log, as stderr does: what they write is never read as a
message. stdin is empty for them. So call start() before anything prints.
"""

import itertools
import json
import os
import queue
import sys
import threading
import traceback

#: How long a call waits for AutoScene's reply, in seconds.
CALL_WAIT = 120


class CallError(Exception):
    """A call AutoScene refused or couldn't answer: its code, and why."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


class Plugin:
    """A run (`mode` "run") or a service (`mode` "serve") of the plugin."""

    def __init__(self, first: dict, heard, said):
        self.mode = "serve" if first.get("type") == "serve" else "run"
        #: The first line: the run line, or the serve line.
        self.first = first
        #: The plugin's settings, as they are now.
        self.settings = first.get("settings", {})
        #: Set once AutoScene asks the run to cancel, or the service to stop.
        self.cancelled = threading.Event()
        self._heard = heard
        self._said = said
        self._out = threading.Lock()
        self._ids = itertools.count(1)
        self._calls: dict = {}
        self._handlers: dict = {}
        self._requests: queue.Queue = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()

    def _send(self, message: dict) -> None:
        # Never NaN or Infinity, which AutoScene can't read: they raise.
        self._write(json.dumps(message, allow_nan=False))

    def _write(self, line: str) -> None:
        with self._out:
            self._said.write(line + "\n")
            self._said.flush()

    def _read(self) -> None:
        for line in self._heard:
            try:
                m = json.loads(line)
            except ValueError:
                continue
            if not isinstance(m, dict):
                continue
            kind = m.get("type")
            if kind == "reply":
                waiter = self._calls.pop(m.get("id"), None)
                if waiter is not None:
                    waiter["reply"] = m
                    waiter["done"].set()
            elif kind == "request":
                self._requests.put(m)
            elif kind == "settings":
                self.settings = m.get("settings", {})
            elif kind in ("cancel", "stop"):
                self.cancelled.set()
                if kind == "stop":
                    self._requests.put(None)
        # AutoScene is done with it.
        self.cancelled.set()
        self._requests.put(None)
        for waiter in list(self._calls.values()):
            waiter["done"].set()

    def call(self, method: str, params: dict | None = None):
        """Asks AutoScene for something: the result, or a CallError."""
        id = next(self._ids)
        waiter = {"done": threading.Event(), "reply": None}
        self._calls[id] = waiter
        self._send({"type": "call", "id": id, "method": method, "params": params or {}})
        if not waiter["done"].wait(CALL_WAIT) or waiter["reply"] is None:
            self._calls.pop(id, None)
            raise CallError("failed", f"AutoScene didn't answer {method}.")
        reply = waiter["reply"]
        if "error" in reply:
            raise CallError(reply["error"].get("code", "failed"), reply["error"].get("message", ""))
        return reply.get("result")

    def progress(self, fraction: float | None = None, stage: str | None = None) -> None:
        self._send({"type": "progress", "fraction": fraction, "stage": stage})

    def log(self, message: str, level: str = "info") -> None:
        self._send({"type": "log", "level": level, "message": message})

    def result(self, message: str | None = None) -> None:
        self._send({"type": "result", "message": message})

    def error(self, message: str, detail: str | None = None) -> None:
        self._send({"type": "error", "message": message, "detail": detail})

    def verdict(self, shot: str, keep: bool) -> None:
        """Says whether the pack keeps `shot`, a key of the run's shots (a shot filter's only)."""
        self._send({"type": "verdict", "shot": shot, "keep": keep})

    def emit(self, event: str, data=None) -> None:
        """Tells the plugin's open pages `event` (a service's only)."""
        self._send({"type": "emit", "event": event, "data": data})

    def handle(self, method: str):
        """Makes the function it decorates answer the requests of `method`."""

        def register(handler):
            self._handlers[method] = handler
            return handler

        return register

    def serve(self) -> None:
        """Says the service is ready, then answers each request until AutoScene says stop."""
        self._send({"type": "ready"})
        while (m := self._requests.get()) is not None:
            self._answer(m)

    def _answer(self, m: dict) -> None:
        id, method = m.get("id"), m.get("method")
        handler = self._handlers.get(method)
        if handler is None:
            self._send({"type": "response", "id": id, "error": {"message": f"No method {method}."}})
            return
        try:
            result = handler(m.get("params"), m.get("context", {}))
        except Exception as e:
            fault = {"message": str(e) or type(e).__name__, "detail": traceback.format_exc()}
            self._send({"type": "response", "id": id, "error": fault})
            return
        try:
            line = json.dumps({"type": "response", "id": id, "result": result}, allow_nan=False)
        except (TypeError, ValueError):
            # A set, an object, NaN: this request fails, the service goes on.
            fault = {"message": f"{method} gave an answer AutoScene can't read.", "detail": traceback.format_exc()}
            self._send({"type": "response", "id": id, "error": fault})
            return
        self._write(line)


def start() -> Plugin:
    """Reads AutoScene's first line, and starts listening for the rest."""
    heard, said = _keep_stdin(), _keep_stdout()
    return Plugin(json.loads(heard.readline()), heard, said)


def _keep_stdin():
    """Keeps stdin for AutoScene's lines: the file it gives reads them.

    fd 0 becomes an empty file, so a child process can't take a reply or a
    cancel line. It also can't hang: on Windows a process that starts with a
    pipe a thread is reading waits for that read, which here may never end.
    The copy isn't inherited.
    """
    heard = os.fdopen(os.dup(0), encoding="utf-8")
    nothing = os.open(os.devnull, os.O_RDONLY)
    os.dup2(nothing, 0)
    os.close(nothing)
    sys.stdin = open(0, encoding="utf-8", closefd=False)
    return heard


def _keep_stdout():
    """Keeps stdout for the protocol's lines: the file it gives writes them.

    fd 1 becomes stderr, so whatever else writes to stdout reaches the log:
    print(), a C library, and a child process, which inherits fd 1 on Linux
    and macOS, and on Windows the standard handle dup2 moves along with it.
    The copy isn't inherited, so no child can write a line or hold the pipe
    open.
    """
    sys.stdout.flush()
    said = os.fdopen(os.dup(1), "w", encoding="utf-8", newline="\n")
    os.dup2(2, 1)
    # A new file: the old one may hold on to the console it found on Windows.
    # Line by line, and never an error, so a print() can't lose or end a run.
    sys.stdout = open(1, "w", encoding="utf-8", errors="backslashreplace", buffering=1, closefd=False)
    return said
