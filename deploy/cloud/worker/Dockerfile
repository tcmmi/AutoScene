# syntax=docker/dockerfile:1.7
#
# AutoScene Cloud's GPU worker: the engine on NVIDIA's CUDA, started by RunPod
# Serverless once per scan session (docs/CLOUD.md, deploy/cloud/worker/README.md).
# Build context: `git archive` of a commit, so nothing uncommitted gets in.

# CUDA 13.0 (driver 580 or newer): ONNX Runtime 1.31.1's GPU build links
# cuBLAS 13 and cudart 13, and loads cuDNN 9.
ARG CUDA_IMAGE=nvidia/cuda:13.0.3-cudnn-runtime-ubuntu24.04

# --- the engine, the worker and the models, built with the repo's own tools ---
FROM ubuntu:24.04 AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl build-essential pkg-config cmake clang git \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
WORKDIR /src
COPY rust-toolchain.toml ./
# The pinned nightly of rust-toolchain.toml, as every other build uses.
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain none --profile minimal \
    && rustup toolchain install
COPY . .
RUN --mount=type=cache,target=/opt/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p as-engine --features cuda --bin autoscene-engine \
    && cargo build --release --locked -p as-cloud-worker --bin autoscene-cloud-worker \
    && cargo build --release --locked -p xtask \
    && mkdir -p /out \
    && cp target/release/autoscene-engine target/release/autoscene-cloud-worker /out/ \
    && target/release/xtask fetch-models --dir /out/models

# --- FFmpeg: the build the app pins (xtask/src/sidecars.rs, Linux), so the
# worker reads a file exactly as the app does (the frame index check) ---
FROM ubuntu:24.04 AS ffmpeg
ARG FFMPEG_URL=https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-31-12-59/ffmpeg-n9.0.2-39-g7adfbe40dc-linux64-gpl-shared-9.0.tar.xz
ARG FFMPEG_SHA256=90a42a7d9689c031beacc3d03b139fd8243351d616988e8a74188df949fe5d08
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl xz-utils patchelf \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL -o /tmp/ffmpeg.tar.xz "$FFMPEG_URL" \
    && echo "$FFMPEG_SHA256  /tmp/ffmpeg.tar.xz" | sha256sum -c - \
    && mkdir -p /tmp/x /out/ffmpeg && tar -xJf /tmp/ffmpeg.tar.xz -C /tmp/x \
    && d=$(ls /tmp/x) \
    && cp /tmp/x/$d/bin/ffmpeg /tmp/x/$d/bin/ffprobe /out/ffmpeg/ \
    && cp -P /tmp/x/$d/lib/*.so* /out/ffmpeg/ \
    && for f in /out/ffmpeg/*; do [ -L "$f" ] || patchelf --set-rpath '$ORIGIN' "$f"; done

# --- ONNX Runtime 1.31.1 with CUDA, Microsoft's NuGet build ---
FROM ubuntu:24.04 AS ort
ARG ORT_VERSION=1.31.1
ARG ORT_SHA256=cfb5315b87f812d8d91194075eb4aab3cbfd97a932f2b56c9bfa41e36087c4a2
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl unzip \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL -o /tmp/ort.nupkg \
        "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.gpu.linux/${ORT_VERSION}/microsoft.ml.onnxruntime.gpu.linux.${ORT_VERSION}.nupkg" \
    && echo "$ORT_SHA256  /tmp/ort.nupkg" | sha256sum -c - \
    && mkdir -p /out/ort \
    && unzip -j /tmp/ort.nupkg \
        runtimes/linux-x64/native/libonnxruntime.so \
        runtimes/linux-x64/native/libonnxruntime_providers_cuda.so \
        runtimes/linux-x64/native/libonnxruntime_providers_shared.so \
        -d /out/ort

# --- what runs on the GPU ---
FROM ${CUDA_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends python3 python3-venv ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && python3 -m venv /opt/venv \
    && /opt/venv/bin/pip install --no-cache-dir runpod==2.3.0
COPY --from=build /out/autoscene-engine /out/autoscene-cloud-worker /app/
COPY --from=build /out/models /app/models
COPY --from=ffmpeg /out/ffmpeg /app/ffmpeg
COPY --from=ort /out/ort /app/ort
COPY deploy/cloud/worker/handler.py /app/handler.py
# The CUDA provider loads beside the runtime; FFmpeg's libraries carry $ORIGIN.
ENV LD_LIBRARY_PATH=/app/ort
CMD ["/opt/venv/bin/python", "-u", "/app/handler.py"]
