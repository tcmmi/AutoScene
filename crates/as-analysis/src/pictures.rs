//! The scenes pass over a video: a picture embedding of a few frames of
//! every shot ([`crate::scenes::picture_frames`]), which is what
//! [`crate::scenes::scenes`] reads besides what a project already stores.
//!
//! The video is decoded once, every frame at the embedder's size, from the
//! first shot asked for to the last picture; a run that was cut short is
//! taken up by asking for the shots still missing.

use as_core::{CancelToken, FrameIdx, FrameRange};
use as_media::frames::{Decoder, decode_samples_with};
use as_media::{Source, Tools};
use as_vision::dinov2::{self, Dinov2};
use half::f16;

use crate::AnalysisError;
use crate::scenes::picture_frames;

/// The size frames are decoded at: the embedder's own.
pub const SIZE: (u32, u32) = (dinov2::W as u32, dinov2::H as u32);
/// Shots handed out at a time.
pub const CHUNK: usize = 64;

/// The pictures of one shot: the unit vectors of its frames, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct ShotPictures {
    /// The shot's first frame.
    pub start: FrameIdx,
    pub vectors: Vec<Vec<f32>>,
}

/// Pack a shot's vectors as little-endian f16, one after the other (what
/// the database keeps).
pub fn to_bytes(vectors: &[Vec<f32>]) -> Vec<u8> {
    vectors.iter().flatten().flat_map(|&v| f16::from_f32(v).to_le_bytes()).collect()
}

/// Unpack [`to_bytes`], each vector renormalised to unit length.
pub fn from_bytes(b: &[u8]) -> Option<Vec<Vec<f32>>> {
    let each = dinov2::DIM * 2;
    if b.is_empty() || !b.len().is_multiple_of(each) {
        return None;
    }
    Some(
        b.chunks_exact(each)
            .map(|one| {
                let mut v: Vec<f32> = one.as_chunks::<2>().0.iter().map(|c| f16::from_le_bytes(*c).to_f32()).collect();
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                v.iter_mut().for_each(|x| *x /= n);
                v
            })
            .collect(),
    )
}

/// Embeds the pictures of `shots` (sorted, disjoint) of `src` with `net` and
/// hands them out in order, [`CHUNK`] shots at a time. `progress` gets 0..1.
#[allow(clippy::too_many_arguments)]
pub fn shot_pictures(
    tools: &Tools,
    src: Source,
    net: &mut Dinov2,
    shots: &[FrameRange],
    decoder: Decoder,
    cancel: &CancelToken,
    mut progress: impl FnMut(f64),
    mut on_shots: impl FnMut(Vec<ShotPictures>) -> Result<(), AnalysisError>,
) -> Result<(), AnalysisError> {
    assert!(shots.is_sorted_by(|a, b| a.end <= b.start), "shots in order, apart");
    // Every picture wanted, in frame order: (frame, its shot).
    let wanted: Vec<(FrameIdx, usize)> = shots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.end > s.start)
        .flat_map(|(i, s)| picture_frames(s.start, s.end).into_iter().map(move |f| (f, i)))
        .collect();
    let (Some(&(first, _)), Some(&(last, _))) = (wanted.first(), wanted.last()) else { return Ok(()) };
    let span = (last - first + 1) as f64;

    let mut out: Vec<ShotPictures> = Vec::new();
    let mut held: Vec<(usize, Vec<u8>)> = Vec::with_capacity(net.batch());
    let mut failure: Option<AnalysisError> = None;
    // Embeds what is held and hands out the shots that are complete; all
    // of them once `all` is set.
    let mut flush = |held: &mut Vec<(usize, Vec<u8>)>, out: &mut Vec<ShotPictures>, all: bool| {
        if !held.is_empty() {
            let pictures: Vec<&[u8]> = held.iter().map(|(_, rgb)| rgb.as_slice()).collect();
            let vectors = net.embed(&pictures)?;
            for ((shot, _), e) in held.drain(..).zip(vectors) {
                match out.last_mut() {
                    Some(o) if o.start == shots[shot].start => o.vectors.push(e.v),
                    _ => out.push(ShotPictures { start: shots[shot].start, vectors: vec![e.v] }),
                }
            }
        }
        // The last shot may still wait for pictures, unless this is the end.
        let ready = if all { out.len() } else { out.len().saturating_sub(1) };
        if ready >= CHUNK || (all && ready > 0) {
            on_shots(out.drain(..ready).collect())?;
        }
        Ok::<(), AnalysisError>(())
    };

    let mut next = 0;
    let delivered = decode_samples_with(tools, src, first, 1, SIZE, decoder, cancel, |f, rgb| {
        if f != wanted[next].0 {
            return Ok(true);
        }
        held.push((wanted[next].1, rgb.to_vec()));
        next += 1;
        if next.is_multiple_of(256) {
            progress((f - first + 1) as f64 / span);
        }
        let done = next == wanted.len();
        if (held.len() == held.capacity() || done)
            && let Err(e) = flush(&mut held, &mut out, done)
        {
            failure = Some(e);
            return Ok(false);
        }
        Ok(!done)
    });
    if let Some(e) = failure {
        return Err(e);
    }
    delivered?;
    if next != wanted.len() {
        return Err(AnalysisError::Internal(format!(
            "the video ended after {next} of the {} pictures of its shots",
            wanted.len()
        )));
    }
    progress(1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_survive_the_database() {
        let unit = |v: Vec<f32>| {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.into_iter().map(|x| x / n).collect::<Vec<f32>>()
        };
        let vectors: Vec<Vec<f32>> =
            (0..3).map(|k| unit((0..dinov2::DIM).map(|i| ((i * 7 + k * 13) % 31) as f32 - 15.0).collect())).collect();
        let back = from_bytes(&to_bytes(&vectors)).expect("three vectors");
        assert_eq!(back.len(), 3);
        for (a, b) in vectors.iter().zip(&back) {
            let cos: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            assert!(cos > 0.9999, "{cos}");
        }
        assert!(from_bytes(&[]).is_none());
        assert!(from_bytes(&[0; 5]).is_none());
    }
}
