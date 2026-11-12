//! SFace (Zhong et al. 2021; OpenCV Zoo `face_recognition_sface_2021dec`):
//! a 128-d identity embedding of a face aligned to 112×112 by its five
//! landmarks, on the same template as ArcFace ([`crate::arcface::align`]) —
//! Fast Scan's embedder (the pipeline of the Scener app). Much smaller and
//! faster than ArcFace glintr100, and less sure of who is who.
//!
//! The graph reads RGB bytes as they are (it scales them itself) and its
//! batch dimension is free apart from what it declares. A graphics card's
//! session is planned for fixed batches like ArcFace's, the last one of a
//! call padded. The processor's takes each run as it comes, holding just its
//! faces ([`LEAST`] at least): a processor computes a padded batch in full —
//! 16 rows take it 100 ms with one face in them as with 16. Measured on
//! Breaking Bad's pilot (960×540 proxy, Ryzen 7 5800X3D, 2026-11-12) while
//! Fast Scan still embedded the few faces of a shot at a time: its 4,738
//! faces came in 619 calls, padded 722 runs of 16 with 59% of their rows
//! empty; with runs that fit, the pass took 155 s instead of 193 s on an
//! idle PC (two runs each) and gave the same tracks to the byte. Fast Scan
//! has since handed over whole runs (`as_analysis::fast`): only what a
//! chunk leaves over, and what other callers bring, still comes short.

use std::path::Path;

use as_infer::onnx::InputShape;
use as_infer::{Device, InferError, InferResult, Runtime, Session, TensorRef};

use crate::arcface::{CROP, CROP_BYTES, Embedding};

/// Embedding size.
pub const DIM: usize = 128;

/// Rows a run on the processor holds at least. For a single row ONNX
/// Runtime multiplies the last layer's matrix another way, which rounds the
/// embedding's last bits otherwise (all before that layer comes out the
/// same); from two rows on, a face's embedding is the same to the bit
/// whatever shares its run. So a face alone runs beside an empty row, and
/// embeds as it did in a padded batch.
pub const LEAST: usize = 2;

pub struct SFace {
    session: Session,
    input: String,
    device: Device,
    batch: usize,
    /// The session takes any number of faces a run (the processor's).
    fitted: bool,
    x: Vec<f32>,
}

impl SFace {
    /// `batch`: faces per inference call at most. On a graphics card every
    /// call holds exactly that many (the last call of a run is padded); on
    /// the processor, as many as there are faces left.
    pub fn load(rt: &Runtime, model: &Path, batch: usize) -> InferResult<Self> {
        assert!(batch > 0);
        let planned = match rt.preferred_device() {
            Device::Cpu => None,
            _ => Some(rt.session_shaped(model, InputShape::Fixed(&[batch, 3, CROP, CROP]))?),
        };
        let (session, device, fitted) = match planned {
            Some((session, device)) if device != Device::Cpu => (session, device, false),
            // The processor's own, or in place of a card's that didn't build.
            _ => {
                let (session, device) = rt.on_cpu().session_shaped(model, InputShape::FreeBatch)?;
                (session, device, true)
            }
        };
        let input = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .ok_or_else(|| InferError::Model(format!("{}: no input", model.display())))?;
        tracing::info!(device = ?device, batch, fitted, "SFace ready");
        Ok(Self { session, input, device, batch, fitted, x: vec![0.0; batch.max(LEAST) * 3 * CROP * CROP] })
    }

    pub fn device(&self) -> Device {
        self.device
    }

    pub fn batch(&self) -> usize {
        self.batch
    }

    /// Embed aligned crops (each [`CROP_BYTES`] of RGB24), any number. The
    /// norm is the raw output's length.
    pub fn embed(&mut self, crops: &[&[u8]]) -> InferResult<Vec<Embedding>> {
        let mut out = Vec::with_capacity(crops.len());
        for group in crops.chunks(self.batch) {
            let plane = CROP * CROP;
            let rows = rows(group.len(), self.batch, self.fitted);
            let x = &mut self.x[..rows * 3 * plane];
            x.fill(0.0);
            for (i, crop) in group.iter().enumerate() {
                assert_eq!(crop.len(), CROP_BYTES, "aligned crop size");
                let base = i * 3 * plane;
                for (p, px) in crop.as_chunks::<3>().0.iter().enumerate() {
                    x[base + p] = f32::from(px[0]);
                    x[base + plane + p] = f32::from(px[1]);
                    x[base + 2 * plane + p] = f32::from(px[2]);
                }
            }
            let t = TensorRef::from_array_view(([rows, 3, CROP, CROP], &*x))?;
            let outs = self.session.run(as_infer::inputs![self.input.as_str() => t])?;
            let (_, e) = outs[0].try_extract_tensor::<f32>()?;
            if e.len() != rows * DIM {
                return Err(InferError::Run(format!("SFace returned {} values for a batch of {rows}", e.len())));
            }
            for i in 0..group.len() {
                // One with no direction (NaN, infinite) matches no one.
                out.push(crate::arcface::unit(&e[i * DIM..(i + 1) * DIM]));
            }
        }
        Ok(out)
    }
}

/// The rows of a run of `faces` (1..=`batch`): the batch the session is
/// planned for, or — `fitted`, the processor's — the faces, [`LEAST`] at
/// least.
fn rows(faces: usize, batch: usize, fitted: bool) -> usize {
    if fitted { faces.max(LEAST) } else { batch }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_on_the_processor_holds_its_faces_and_never_one_alone() {
        // A card's session computes its whole batch, whatever is in it.
        assert_eq!([1, 5, 16].map(|faces| rows(faces, 16, false)), [16, 16, 16]);
        // The processor's computes the faces there are ...
        assert_eq!([2, 3, 5, 15, 16].map(|faces| rows(faces, 16, true)), [2, 3, 5, 15, 16]);
        // ... and a face alone beside an empty row, whatever the batch.
        assert_eq!(rows(1, 16, true), LEAST);
        assert_eq!(rows(1, 1, true), LEAST);
        assert_eq!(rows(1, 1, false), 1);
    }
}
