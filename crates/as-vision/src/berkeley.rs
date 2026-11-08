//! Berkeley's animated-character embedder (Bamman et al. 2026: DINOv2
//! ViT-L/14 fine-tuned on drawn and rendered faces, `dino_vitl14_crop25`;
//! exported by `tools/model-prep/berkeley.py`): a 1024-d identity of a
//! character's head, which tells the characters of one show apart far
//! better than CCIP does (bench/anime DECISIONS row 7).
//!
//! A crop is the head box padded by a quarter of its size on each side,
//! clipped to the frame and resized to 224×224 (not kept square, as the
//! model was trained), sampled as [`crate::ccip::crop`] samples, then
//! normalised with ImageNet mean/std, NCHW. The output is DINOv2's
//! layer-normed CLS feature: its length is nearly the same for every head
//! and says nothing about how clear it is, so [`unit`] gives it direction
//! and the anime pass reads clarity from CCIP.
//!
//! On a GPU provider the session first proves itself against CPU goldens
//! (`fixtures/anime/berkeley_*`, made by the bench's `norms.py` from the
//! CCIP golden crops), as CCIP does ([`crate::golden`]).

use std::path::Path;

use as_infer::onnx::InputShape;
use as_infer::{Device, InferError, InferResult, Runtime, Session, TensorRef};

use crate::arcface::{Embedding, unit};
use crate::ccip::resample;
use crate::golden::{self, OnCoreMl};

/// Side of the crop the embedder reads.
pub const CROP: usize = 224;
/// Bytes of one RGB24 crop.
pub const CROP_BYTES: usize = CROP * CROP * 3;
/// Embedding size.
pub const DIM: usize = 1024;
/// How far the crop reaches past the head box on each side, in box sizes.
pub const PAD: f32 = 0.25;
/// ImageNet input normalisation, channel-wise: (v/255 − mean)/std.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

const GOLDEN_RGB: [&[u8; CROP_BYTES]; 5] = [
    include_bytes!("../fixtures/anime/berkeley_0.rgb"),
    include_bytes!("../fixtures/anime/berkeley_1.rgb"),
    include_bytes!("../fixtures/anime/berkeley_2.rgb"),
    include_bytes!("../fixtures/anime/berkeley_3.rgb"),
    include_bytes!("../fixtures/anime/berkeley_4.rgb"),
];
const GOLDEN_VEC: [&[u8; DIM * 4]; 5] = [
    include_bytes!("../fixtures/anime/berkeley_0.vec"),
    include_bytes!("../fixtures/anime/berkeley_1.vec"),
    include_bytes!("../fixtures/anime/berkeley_2.vec"),
    include_bytes!("../fixtures/anime/berkeley_3.vec"),
    include_bytes!("../fixtures/anime/berkeley_4.vec"),
];

/// The input value of each byte, per channel ((v/255 − mean)/std).
fn tables() -> [[f32; 256]; 3] {
    std::array::from_fn(|c| std::array::from_fn(|v| (v as f32 / 255.0 - MEAN[c]) / STD[c]))
}

pub struct Berkeley {
    session: Session,
    input: String,
    device: Device,
    batch: usize,
    tables: [[f32; 256]; 3],
    x: Vec<f32>,
    spoilt: u64,
}

impl Berkeley {
    /// `batch`: crops per inference call (the last call of a run is padded).
    /// Proven against the goldens first on anything but the CPU, as
    /// [`crate::Ccip::load`] is.
    pub fn load(rt: &Runtime, model: &Path, batch: usize) -> InferResult<Self> {
        assert!(batch > 0);
        let name = model.display().to_string();
        golden::proven(rt, &name, OnCoreMl::Enforce, |rt| Self::open(rt, model, batch), Self::device, Self::goldens)
    }

    fn open(rt: &Runtime, model: &Path, batch: usize) -> InferResult<Self> {
        let (session, device) = rt.fp32_sessions().session_shaped(model, InputShape::Fixed(&[batch, 3, CROP, CROP]))?;
        let input = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .ok_or_else(|| InferError::Model(format!("{}: no input", model.display())))?;
        tracing::info!(device = ?device, batch, "Berkeley character embedder ready");
        Ok(Self { session, input, device, batch, tables: tables(), x: vec![0.0; batch * 3 * CROP * CROP], spoilt: 0 })
    }

    fn goldens(&mut self) -> Option<f32> {
        let crops: Vec<&[u8]> = GOLDEN_RGB.iter().map(|r| &r[..]).collect();
        let stored: Vec<&[u8]> = GOLDEN_VEC.iter().map(|v| &v[..]).collect();
        Some(golden::worst(&self.embed(&crops).ok()?, &stored))
    }

    pub fn device(&self) -> Device {
        self.device
    }

    pub fn batch(&self) -> usize {
        self.batch
    }

    /// Embed crops (each [`CROP_BYTES`] of RGB24), any number.
    pub fn embed(&mut self, crops: &[&[u8]]) -> InferResult<Vec<Embedding>> {
        let mut out = Vec::with_capacity(crops.len());
        let t = &self.tables;
        let plane = CROP * CROP;
        for group in crops.chunks(self.batch) {
            self.x.fill(0.0);
            for (i, crop) in group.iter().enumerate() {
                assert_eq!(crop.len(), CROP_BYTES, "Berkeley crop size");
                let base = i * 3 * plane;
                for (p, px) in crop.as_chunks::<3>().0.iter().enumerate() {
                    self.x[base + p] = t[0][px[0] as usize];
                    self.x[base + plane + p] = t[1][px[1] as usize];
                    self.x[base + 2 * plane + p] = t[2][px[2] as usize];
                }
            }
            let x = TensorRef::from_array_view(([self.batch, 3, CROP, CROP], &self.x[..]))?;
            let outs = self.session.run(as_infer::inputs![self.input.as_str() => x])?;
            let (_, e) = outs[0].try_extract_tensor::<f32>()?;
            if e.len() != self.batch * DIM {
                return Err(InferError::Run(format!(
                    "the Berkeley embedder returned {} values for a batch of {}",
                    e.len(),
                    self.batch
                )));
            }
            for i in 0..group.len() {
                let embedding = unit(&e[i * DIM..(i + 1) * DIM]);
                if embedding.norm == 0.0 {
                    if self.spoilt == 0 {
                        tracing::warn!(device = ?self.device, "the Berkeley embedder gave an embedding with no direction");
                    }
                    self.spoilt += 1;
                }
                out.push(embedding);
            }
        }
        Ok(out)
    }
}

/// The crop of a head box (x1, y1, x2, y2 in pixels) of a `w`×`h` RGB24
/// frame: the box padded by [`PAD`] on each side, clipped to the frame,
/// resized to [`CROP`]×[`CROP`].
pub fn crop(rgb: &[u8], w: usize, h: usize, bbox: [f32; 4]) -> Vec<u8> {
    assert!(w > 0 && h > 0 && rgb.len() == w * h * 3, "Berkeley crop of {w}×{h}");
    let (bw, bh) = (bbox[2] - bbox[0], bbox[3] - bbox[1]);
    let x0 = (bbox[0] - PAD * bw).max(0.0);
    let y0 = (bbox[1] - PAD * bh).max(0.0);
    let x1 = (bbox[2] + PAD * bw).min(w as f32);
    let y1 = (bbox[3] + PAD * bh).min(h as f32);
    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        return vec![0; CROP_BYTES];
    }
    resample(rgb, w, h, [x0, y0, x1 - x0, y1 - y0], CROP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_tables_apply_imagenet_normalisation() {
        let t = tables();
        assert!((t[0][0] + MEAN[0] / STD[0]).abs() < 1e-6);
        assert!((t[2][255] - (1.0 - MEAN[2]) / STD[2]).abs() < 1e-6);
    }

    #[test]
    fn the_crop_pads_the_box_and_stays_inside_the_frame() {
        // 400×400, left half white: a box at the left edge reaches no
        // further left than the frame, so its crop starts on white.
        let (w, h) = (400, 400);
        let rgb: Vec<u8> = (0..w * h).flat_map(|i| [if i % w < 200 { 255 } else { 0 }; 3]).collect();
        let c = crop(&rgb, w, h, [0.0, 100.0, 100.0, 200.0]);
        assert_eq!(c.len(), CROP_BYTES);
        assert_eq!(&c[..3], &[255, 255, 255]);
        // Padded by 25 on the right: 125 of the 200 white columns, all white.
        assert!(c.as_chunks::<3>().0.iter().all(|p| p[0] == 255), "the crop stays in the white half");
        // A box spanning the seam shows both halves, white left of black.
        let c = crop(&rgb, w, h, [150.0, 100.0, 250.0, 200.0]);
        let px = |u: usize| c[(CROP * CROP / 2 + u) * 3];
        assert_eq!((px(0), px(CROP - 1)), (255, 0));
    }

    #[test]
    fn goldens_have_the_crop_and_vector_sizes() {
        for (r, v) in GOLDEN_RGB.iter().zip(&GOLDEN_VEC) {
            assert_eq!(r.len(), CROP_BYTES);
            assert_eq!(v.len(), DIM * 4);
        }
    }
}
