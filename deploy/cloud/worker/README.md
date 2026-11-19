# AutoScene Cloud's GPU worker

The image RunPod Serverless starts once per scan session. The gateway posts the
session's launch as a RunPod job (`[backend] kind = "http"`, `wrap = "input"`);
`handler.py` runs `autoscene-cloud-worker` on it, which fetches the upload from
the gateway, runs the app's jobs on the engine (CUDA) and sends the replies back.

## What is in it

| Part | Where it comes from |
|---|---|
| `autoscene-engine` (feature `cuda`) and `autoscene-cloud-worker` | this repo, built with the pinned nightly of `rust-toolchain.toml` |
| The models | `xtask fetch-models` (every model of `models/manifest.toml`, checked) |
| FFmpeg | the BtbN build the app pins for Linux (`xtask/src/sidecars.rs`), so the worker reads a file as the app does |
| ONNX Runtime 1.31.1 with CUDA | Microsoft's NuGet package `Microsoft.ML.OnnxRuntime.Gpu.Linux` |
| CUDA 13.0 and cuDNN 9 | `nvidia/cuda:13.0.3-cudnn-runtime-ubuntu24.04` (hosts need driver 580 or newer) |

The image holds the models, some of them under non-commercial terms: keep the
repository **private**.

## Build and push (on the VDS)

```sh
# on a computer with the repo: the commit, nothing uncommitted
git archive --format=tar <commit> | gzip -1 > worker-ctx.tgz
scp worker-ctx.tgz root@<vds>:/root/
# on the VDS
rm -rf /root/worker-build && mkdir /root/worker-build && tar -xzf /root/worker-ctx.tgz -C /root/worker-build
cd /root/worker-build && DOCKER_BUILDKIT=1 nice -n 15 docker build -f deploy/cloud/worker/Dockerfile -t autoscene-cloud-worker:<commit> .
docker tag autoscene-cloud-worker:<commit> docker.io/<user>/autoscene-cloud-worker:<commit>
docker push docker.io/<user>/autoscene-cloud-worker:<commit>
```

## RunPod

- A container registry credential with the Docker Hub token, so RunPod can pull the private image.
- A serverless template: the image, container disk large enough for the biggest upload plus work files (`max_upload_bytes` + 10 GB).
- An endpoint in EU data centres: 24 GB GPUs, CUDA 13.0 or newer, 0 workers kept warm, a few at most, FlashBoot on, an execution timeout above the longest session (30 minutes plus four times the video's length).

## The gateway

```toml
[backend]
kind = "http"
url = "https://api.runpod.ai/v2/<endpoint id>/run"
authorization = "Bearer <RunPod API key>"
wrap = "input"
```

Restart `autoscene-cloud`, then lift the pause (`/cloud stats` → Resume scans).

## Check a cloud scan

`crates/as-cloud-worker/examples/cloud_smoke.rs` runs one as the app does: a shots job and a
faces job on a video, with `--local` the same jobs on this computer's engine to compare.

```sh
cargo run --release -p as-cloud-worker --example cloud_smoke -- --file clip.mp4 --code <beta code> \
  --ffmpeg app/src-tauri/ffmpeg \
  --local target/release/autoscene-engine --ort app/src-tauri/onnxruntime --models fixtures/models
```
