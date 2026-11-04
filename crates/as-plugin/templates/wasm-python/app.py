"""An AutoScene plugin that runs in AutoScene's sandbox. AutoScene calls
`run` once for each run of one of its actions (`plugin/plugin.json`)."""

import wit_world
from componentize_py_types import Err
from wit_world.imports import host, types


class WitWorld(wit_world.WitWorld):
    def run(self, request: types.RunRequest) -> str | None:
        if request.action.id == "hello":
            host.log(types.LogLevel.INFO, "Saying hello.")
            return f"Hello from {request.plugin.id}."
        raise Err(types.RunError(message=f"{request.action.id} isn't an action of this plugin.", detail=None))
