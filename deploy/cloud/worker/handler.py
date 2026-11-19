"""RunPod Serverless handler of AutoScene Cloud's GPU worker.

The gateway posts each scan session as a RunPod job whose `input` is the
session's launch (`as_cloud::WorkerLaunch`). The worker then talks to the
gateway itself: it fetches the uploaded file, runs the engine on the GPU for
the app's jobs and sends the replies back, until the session closes or idles.
The job's own answer only says how the worker ended.

Nothing here prints the launch: it holds the session's token.
"""

import json
import subprocess

import runpod

WORKER = [
    "/app/autoscene-cloud-worker",
    "--engine", "/app/autoscene-engine",
    "--ffmpeg", "/app/ffmpeg",
    "--work", "/tmp",
    "--",
    "--ort", "/app/ort",
    "--models", "/app/models",
    "--ffmpeg", "/app/ffmpeg",
]

FIELDS = ("session", "base", "token", "size", "sha256", "idle_secs")


def handler(job):
    launch = job.get("input") or {}
    missing = [f for f in FIELDS if f not in launch]
    if missing:
        return {"ok": False, "error": f"the launch lacks {', '.join(missing)}"}
    session = str(launch["session"])
    print(f"session {session}: starting the worker", flush=True)
    done = subprocess.run(WORKER, input=json.dumps(launch).encode())
    print(f"session {session}: the worker ended with {done.returncode}", flush=True)
    return {"ok": done.returncode == 0, "exit": done.returncode}


if __name__ == "__main__":
    runpod.serverless.start({"handler": handler})
