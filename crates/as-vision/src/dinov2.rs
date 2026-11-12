//! DINOv2-small (ViT-S/14, `onnx-community/dinov2-small-ONNX`) as a general
//! picture embedder: what a frame shows, for telling the scenes of an
//! episode apart (`bench/scenes`; `bench/models/DECISIONS.md` rows 7 and 8).
//!
//! A picture is the whole frame at [`W`]×[`H`] RGB24 (32×18 patches; never
//! cropped, whatever the frame's shape), normalised with ImageNet mean/std,
//! NCHW. The embedding is the class token of the last layer, [`DIM`] numbers,
//! as a unit vector.
//!
//! On a GPU provider the session first proves itself against goldens, as
//! the identity embedders do ([`crate::golden`]): two made-up pictures
//! ([`golden_picture`]) and the vectors the processor gives them
//! (`fixtures/scenes`, written by the `dinov2_goldens` example).

use std::path::Path;

use as_infer::onnx::InputShape;
use as_infer::{Device, InferError, InferResult, Runtime, Session, TensorRef};

use crate::arcface::{Embedding, unit};
use crate::golden::{self, OnCoreMl};

/// Size of the picture the embedder reads.
pub const W: usize = 448;
pub const H: usize = 252;
/// Bytes of one RGB24 picture.
pub const PICTURE_BYTES: usize = W * H * 3;
/// Embedding size.
pub const DIM: usize = 384;
/// The side of a patch, and the tokens of a picture: the class token and
/// one per patch.
const PATCH: usize = 14;
const TOKENS: usize = 1 + (W / PATCH) * (H / PATCH);
/// ImageNet input normalisation, channel-wise: (v/255 − mean)/std.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

const GOLDEN_VEC: [&[u8; DIM * 4]; 2] =
    [include_bytes!("../fixtures/scenes/dinov2_0.vec"), include_bytes!("../fixtures/scenes/dinov2_1.vec")];

/// The `k`-th golden picture: blocks of a hashed colour under a shade that
/// grows from left to right and slanted stripes. Made up, so that no film
/// frame sits in the repository; it only has to give the network something
/// with edges, colours and textures to work on.
pub fn golden_picture(k: u32) -> Vec<u8> {
    let mut rgb = vec![0u8; PICTURE_BYTES];
    for y in 0..H as u32 {
        for x in 0..W as u32 {
            let block =
                (x / 32).wrapping_mul(73_856_093) ^ (y / 32).wrapping_mul(19_349_663) ^ k.wrapping_mul(83_492_791);
            let shade = (x * 255 / W as u32) as u8;
            let stripe = if ((x + y * (k + 1)) / 24).is_multiple_of(2) { 40 } else { 0 };
            let at = (y as usize * W + x as usize) * 3;
            for c in 0..3 {
                let base = (block >> (8 * c)) as u8;
                rgb[at + c] = (base / 2).saturating_add(shade / 3).saturating_add(stripe);
            }
        }
    }
    rgb
}

/// The input value of each byte, per channel ((v/255 − mean)/std).
fn tables() -> [[f32; 256]; 3] {
    std::array::from_fn(|c| std::array::from_fn(|v| (v as f32 / 255.0 - MEAN[c]) / STD[c]))
}

pub struct Dinov2 {
    session: Session,
    input: String,
    device: Device,
    batch: usize,
    tables: [[f32; 256]; 3],
    x: Vec<f32>,
}

impl Dinov2 {
    /// `batch`: pictures per inference call (the last call of a run is
    /// padded). Proven against the goldens first on anything but the CPU.
    pub fn load(rt: &Runtime, model: &Path, batch: usize) -> InferResult<Self> {
        let name = model.display().to_string();
        golden::proven(rt, &name, OnCoreMl::Enforce, |rt| Self::open(rt, model, batch), Self::device, Self::goldens)
    }

    /// The session as the runtime gives it, with no proof: for making the
    /// goldens.
    pub fn open(rt: &Runtime, model: &Path, batch: usize) -> InferResult<Self> {
        assert!(batch > 0);
        let (session, device) = rt.fp32_sessions().session_shaped(model, InputShape::Fixed(&[batch, 3, H, W]))?;
        let input = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .ok_or_else(|| InferError::Model(format!("{}: no input", model.display())))?;
        tracing::info!(device = ?device, batch, "DINOv2 picture embedder ready");
        Ok(Self { session, input, device, batch, tables: tables(), x: vec![0.0; batch * 3 * W * H] })
    }

    fn goldens(&mut self) -> Option<f32> {
        let pictures: Vec<Vec<u8>> = (0..GOLDEN_VEC.len() as u32).map(golden_picture).collect();
        let stored: Vec<&[u8]> = GOLDEN_VEC.iter().map(|v| &v[..]).collect();
        let refs: Vec<&[u8]> = pictures.iter().map(Vec::as_slice).collect();
        Some(golden::worst(&self.embed(&refs).ok()?, &stored))
    }

    pub fn device(&self) -> Device {
        self.device
    }

    pub fn batch(&self) -> usize {
        self.batch
    }

    /// Embed pictures (each [`PICTURE_BYTES`] of RGB24), any number.
    pub fn embed(&mut self, pictures: &[&[u8]]) -> InferResult<Vec<Embedding>> {
        let mut out = Vec::with_capacity(pictures.len());
        let t = &self.tables;
        let plane = W * H;
        for group in pictures.chunks(self.batch) {
            self.x.fill(0.0);
            for (i, picture) in group.iter().enumerate() {
                assert_eq!(picture.len(), PICTURE_BYTES, "DINOv2 picture size");
                let base = i * 3 * plane;
                for (p, px) in picture.as_chunks::<3>().0.iter().enumerate() {
                    self.x[base + p] = t[0][px[0] as usize];
                    self.x[base + plane + p] = t[1][px[1] as usize];
                    self.x[base + 2 * plane + p] = t[2][px[2] as usize];
                }
            }
            let x = TensorRef::from_array_view(([self.batch, 3, H, W], &self.x[..]))?;
            let outs = self.session.run(as_infer::inputs![self.input.as_str() => x])?;
            let (_, tokens) = outs[0].try_extract_tensor::<f32>()?;
            if tokens.len() != self.batch * TOKENS * DIM {
                return Err(InferError::Run(format!(
                    "the picture embedder returned {} values for a batch of {}",
                    tokens.len(),
                    self.batch
                )));
            }
            for i in 0..group.len() {
                let class = &tokens[i * TOKENS * DIM..i * TOKENS * DIM + DIM];
                out.push(unit(class));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_patches_of_fourteen() {
        assert_eq!((W % PATCH, H % PATCH), (0, 0));
        assert_eq!(TOKENS, 577);
    }

    #[test]
    fn golden_pictures_differ_and_repeat() {
        let (a, b) = (golden_picture(0), golden_picture(1));
        assert_eq!(a.len(), PICTURE_BYTES);
        assert_eq!(a, golden_picture(0), "the same picture every time");
        assert!(a.iter().zip(&b).filter(|(x, y)| x != y).count() > PICTURE_BYTES / 4, "two different pictures");
    }

    #[test]
    fn goldens_are_unit_free_vectors_of_the_model() {
        // Written by the `dinov2_goldens` example; a file of zeros would
        // send every GPU session back to the processor.
        for v in GOLDEN_VEC {
            let v = golden::vector(v);
            assert_eq!(v.len(), DIM);
            assert!(v.iter().map(|x| x * x).sum::<f32>().sqrt() > 1.0, "a golden vector is missing");
        }
    }
}
