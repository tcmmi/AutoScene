"""Clip List: an example AutoScene plugin in Python that runs in AutoScene's
sandbox, as a WebAssembly component that componentize-py builds.

Its export action `list` saves a text file in the export's folder, with
one line per clip: its name, its episode, and its first and last frame,
separated by tabs. The folder is the only one it sees beside its own
(`/data`): plugin/plugin.json asks for `exports:write`, so AutoScene hands
it the export's folder as `/export`. docs/PLUGINS.md describes the sandbox.
"""

import wit_world
from componentize_py_types import Err
from wit_world.imports import host, types

# What a file name can't hold on one of the systems AutoScene runs on.
NOT_IN_A_NAME = set('<>:"/\\|?*')


class WitWorld(wit_world.WitWorld):
    def run(self, request: types.RunRequest) -> str | None:
        if request.action.id != "list":
            raise Err(types.RunError(message=f"Clip List has no action {request.action.id}.", detail=None))
        export = request.export
        if export is None:
            raise Err(types.RunError(message="Save a clip list runs on an export.", detail=None))
        if export.out_dir is None:
            raise Err(types.RunError(message="Clip List can't reach the export's folder.", detail=None))
        name = f"{file_name(export.label or export.id)} clips.txt"
        host.progress(None, "Writing the clip list")
        lines = [f"{c.name}\t{c.episode}\t{c.start}\t{c.end - 1}\n" for c in export.clips]
        # Absolute paths: the sandbox has no other folder to start from.
        try:
            with open(f"{export.out_dir}/{name}", "w", encoding="utf-8", newline="\n") as f:
                f.writelines(lines)
        except OSError as e:
            raise Err(types.RunError(message="Couldn't save the clip list.", detail=str(e)))
        host.log(types.LogLevel.INFO, f"Wrote {len(lines)} clips to {name}.")
        return "Saved the clip list."


def file_name(text: str) -> str:
    """`text` as a file name on every system: what one can't hold becomes `_`."""
    name = "".join("_" if c in NOT_IN_A_NAME or ord(c) < 32 else c for c in text).strip(" .")
    return name or "Export"
