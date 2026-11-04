//! The parity check an embedder's session passes before it is trusted
//! (design-i66_67 §5.1): on anything but the processor it must embed stored
//! inputs to the vectors the processor gave them. Low-precision maths on a
//! GPU provider would bend every embedding a little and move every match;
//! a session that misses its goldens moves down its device chain:
//! NVIDIA's engine, then the GPU runtime's own provider (DirectML, Core ML),
//! then the processor. Should even the processor miss, the model file is
//! not the one the goldens were made with, and loading fails.

use as_infer::{Device, InferError, InferResult, Runtime};

use crate::arcface::Embedding;

/// How close a session's embeddings must sit to the processor's (cosine).
pub const COSINE: f32 = 0.999;

/// How a Core ML session that misses the goldens is treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnCoreMl {
    /// Rebuilt on the processor, as anywhere else.
    Enforce,
    /// Kept, and the miss logged: Core ML's Neural Engine computes in fp16,
    /// and how far that bends the model is for a run on a real Mac to say
    /// (`coreml_bench`) before it may cost every Mac its GPU.
    Log,
}

/// The cosine of a unit embedding to a stored vector. Cosine ignores scale,
/// so stored vectors stay as the network gave them; an embedding that is all
/// zeros (one the session spoilt) matches nothing.
pub fn cosine(e: &Embedding, stored: &[f32]) -> f32 {
    let norm = stored.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !(norm.is_finite() && norm > 0.0) || e.v.len() != stored.len() {
        return 0.0;
    }
    e.v.iter().zip(stored).map(|(a, b)| a * b).sum::<f32>() / norm
}

/// A stored vector: little-endian f32s.
pub fn vector(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

/// The worst cosine of `got` to the stored vectors (raw little-endian f32
/// bytes), in order; 0 when a vector is missing.
pub fn worst(got: &[Embedding], stored: &[&[u8]]) -> f32 {
    if got.len() != stored.len() {
        return 0.0;
    }
    got.iter().zip(stored).map(|(e, bytes)| cosine(e, &vector(bytes))).fold(1.0, f32::min)
}

/// What a session that just loaded on `device` does with the worst cosine
/// it reached on its goldens (`None`: it couldn't embed them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    /// Kept, the miss logged ([`OnCoreMl::Log`]).
    KeepLogged,
    /// Built again without NVIDIA's engine.
    WithoutAccel,
    /// Built again on the processor.
    OnCpu,
    /// The processor misses too: the file isn't the goldens' model.
    Fail,
}

/// [`Verdict`] of a session on `device` whose worst cosine was `worst`.
pub fn verdict(device: Device, worst: Option<f32>, coreml: OnCoreMl) -> Verdict {
    let holds = worst.is_some_and(|c| c >= COSINE);
    match device {
        _ if holds => Verdict::Keep,
        Device::Cpu => Verdict::Fail,
        Device::CoreMl if coreml == OnCoreMl::Log => Verdict::KeepLogged,
        Device::NvRtx => Verdict::WithoutAccel,
        Device::DirectMl | Device::CoreMl => Verdict::OnCpu,
    }
}

/// Load a model with `open` on `rt`, proven against its goldens: `check`
/// embeds them and gives the worst cosine (`None`: it couldn't). A session
/// that starts on the processor is trusted as it is; one that misses moves
/// down the device chain ([`verdict`]).
pub fn proven<T>(
    rt: &Runtime,
    model: &str,
    coreml: OnCoreMl,
    open: impl Fn(&Runtime) -> InferResult<T>,
    device: impl Fn(&T) -> Device,
    mut check: impl FnMut(&mut T) -> Option<f32>,
) -> InferResult<T> {
    let mut session = open(rt)?;
    if device(&session) == Device::Cpu {
        return Ok(session);
    }
    loop {
        let on = device(&session);
        let worst = check(&mut session);
        match verdict(on, worst, coreml) {
            Verdict::Keep => return Ok(session),
            Verdict::KeepLogged => {
                tracing::warn!(model, device = ?on, ?worst, "embeddings miss the CPU goldens on Core ML; kept, as Core ML's are not enforced yet");
                return Ok(session);
            }
            Verdict::WithoutAccel => {
                tracing::warn!(
                    model,
                    ?worst,
                    "embeddings on NVIDIA's engine miss the CPU goldens; rebuilding without it"
                );
                session = open(&rt.without_accel())?;
            }
            Verdict::OnCpu => {
                tracing::warn!(model, device = ?on, ?worst, "embeddings miss the CPU goldens; rebuilding on the CPU");
                session = open(&rt.on_cpu())?;
            }
            Verdict::Fail => return Err(InferError::Model(format!("{model}: embeddings don't match the CPU goldens"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_miss_moves_down_the_device_chain() {
        use Device::{CoreMl, Cpu, DirectMl, NvRtx};
        let (hit, miss) = (Some(0.99995), Some(0.979));
        for device in [NvRtx, DirectMl, CoreMl, Cpu] {
            assert_eq!(verdict(device, hit, OnCoreMl::Enforce), Verdict::Keep, "{device:?}");
        }
        assert_eq!(verdict(NvRtx, miss, OnCoreMl::Enforce), Verdict::WithoutAccel);
        assert_eq!(verdict(DirectMl, miss, OnCoreMl::Enforce), Verdict::OnCpu);
        assert_eq!(verdict(DirectMl, None, OnCoreMl::Log), Verdict::OnCpu, "couldn't embed them at all");
        assert_eq!(verdict(CoreMl, miss, OnCoreMl::Enforce), Verdict::OnCpu);
        assert_eq!(verdict(CoreMl, miss, OnCoreMl::Log), Verdict::KeepLogged);
        assert_eq!(verdict(Cpu, miss, OnCoreMl::Log), Verdict::Fail);
        // Right at the line holds.
        assert_eq!(verdict(DirectMl, Some(COSINE), OnCoreMl::Enforce), Verdict::Keep);
    }

    #[test]
    fn cosines_ignore_scale_and_spoilt_embeddings_match_nothing() {
        let e = Embedding { v: vec![0.6, 0.8], norm: 5.0 };
        assert!((cosine(&e, &[3.0, 4.0]) - 1.0).abs() < 1e-6);
        assert!((cosine(&e, &[-3.0, -4.0]) + 1.0).abs() < 1e-6);
        assert_eq!(cosine(&Embedding { v: vec![0.0, 0.0], norm: 0.0 }, &[3.0, 4.0]), 0.0);
        assert_eq!(cosine(&e, &[0.0, 0.0]), 0.0);
        assert_eq!(cosine(&e, &[f32::NAN, 1.0]), 0.0);
        let bytes: Vec<u8> = [3.0f32, 4.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert!((worst(std::slice::from_ref(&e), &[&bytes]) - 1.0).abs() < 1e-6);
        assert_eq!(worst(&[e], &[]), 0.0, "a vector missing is a miss");
    }
}
