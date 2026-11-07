//! Reading faces again, at the source's own resolution.
//!
//! The face pass looks at every few frames of a picture shrunk to 720p, and
//! searches half of that: a face 60 pixels tall there is 30 to the detector,
//! its landmarks a guess. Here the place is known, so a crop around it is cut
//! from the full frame and searched alone: the face fills the detector's
//! input and its landmarks are as good as the footage allows.
//!
//! A hard face also gets the frames next to its clearest one, which the pass
//! skipped (a blurred frame can sit beside a sharp one), and frames a little
//! further off. Those further off may show someone else: they can confirm a
//! reading, never stand alone.

use as_core::{CancelToken, FrameIdx, FrameRange};
use as_media::frames::{MAX_SIDE, decode_picks, sample_size};
use as_media::{MediaError, Source};
use as_vision::image::resize_region;
use as_vision::scrfd::iou;
use as_vision::{ArcFace, Scrfd};
use as_vision::{Ccip, arcface, ccip};

use crate::fast::{FastParams, Finder, Spot};
use crate::tracker::TrackerParams;
use crate::{AnalysisError, AnimeParams, FaceDet, FaceParams};

/// The side of the square a crop is searched in: the size the detector is
/// loaded for.
pub const SIDE: usize = 320;

/// Most frames one decoder is asked for (they all go on its command line).
const MAX_PICKS: usize = 500;

/// A face to read: a track as stored.
#[derive(Clone, Debug)]
pub struct RescanFace {
    pub id: u32,
    pub shot: FrameRange,
    pub quality: f32,
    pub dets: Vec<FaceDet>,
}

#[derive(Clone, Copy, Debug)]
pub struct RescanParams {
    /// Frames of a track read at most, the clearest first ...
    pub looks: usize,
    /// ... at least this many seconds apart: another angle, not the same one.
    pub apart: f64,
    /// A track below this quality is hard to read: it gets the frames around.
    pub hard: f32,
    /// Frames next to the clearest one (the pass skipped them).
    pub next_to: u32,
    /// Seconds further off, on either side.
    pub further: [f64; 2],
    /// Only the track's own frames (a drawn head holds still for frames and a
    /// box from elsewhere still gets a confident answer).
    pub own_frames: bool,
    /// The crop's side, in sides of the face's box.
    pub crop_scale: f32,
    pub det_threshold: f32,
    /// A face found here is the one looked for from this overlap with where
    /// it was (0: whatever the finder says).
    pub min_iou: f32,
    /// A look this unlike the mean of a face's looks is someone else.
    pub outlier: f32,
    /// Frames closer than this many seconds are decoded in one go.
    pub window: f64,
    /// Crops embedded at a time.
    pub batch: usize,
}

impl Default for RescanParams {
    fn default() -> Self {
        Self {
            looks: 3,
            apart: 0.5,
            hard: 0.45,
            next_to: 2,
            further: [0.25, 0.5],
            own_frames: false,
            crop_scale: 2.2,
            det_threshold: 0.3,
            min_iou: 0.2,
            outlier: 0.2,
            window: 1.0,
            batch: 32,
        }
    }
}

/// What a face turned out to be on a second reading.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceRead {
    pub id: u32,
    /// Its embedding from the looks that agree; `None`: nothing to go by.
    pub embedding: Option<Vec<f32>>,
    pub quality: f32,
    /// Frames looked at, and in how many the face was found.
    pub tried: u32,
    pub found: u32,
}

/// One frame of one face to look at.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    face: usize,
    frame: FrameIdx,
    bbox: [f32; 4],
    /// May show someone else: confirms, never stands alone.
    far: bool,
}

/// The box nearest in time to `frame`.
fn nearest(dets: &[FaceDet], frame: FrameIdx) -> Option<[f32; 4]> {
    dets.iter().min_by_key(|d| d.frame.abs_diff(frame)).map(|d| d.bbox)
}

/// The frames of `f` worth reading, `face` being its place in the list.
fn plan(face: usize, f: &RescanFace, fps: f64, p: &RescanParams) -> Vec<Look> {
    let mut by_clarity: Vec<&FaceDet> = f.dets.iter().collect();
    by_clarity.sort_by(|a, b| b.clarity_cmp(a).then(a.frame.cmp(&b.frame)));
    let Some(&clearest) = by_clarity.first() else { return Vec::new() };
    let apart = (p.apart * fps).round() as u32;
    let mut chosen: Vec<&FaceDet> = Vec::with_capacity(p.looks);
    for d in by_clarity {
        if chosen.len() < p.looks && chosen.iter().all(|c| c.frame.abs_diff(d.frame) >= apart) {
            chosen.push(d);
        }
    }
    let mut looks: Vec<Look> = chosen.iter().map(|d| Look { face, frame: d.frame, bbox: d.bbox, far: false }).collect();
    if f.quality < p.hard && !p.own_frames {
        let (first, last) = (f.dets.iter().map(|d| d.frame).min(), f.dets.iter().map(|d| d.frame).max());
        let (first, last) = (first.unwrap_or(clearest.frame), last.unwrap_or(clearest.frame));
        let mut add = |offset: i64, far: bool| {
            let frame = clearest.frame as i64 + offset;
            if frame < f.shot.start as i64 || frame >= f.shot.end as i64 {
                return;
            }
            let frame = frame as FrameIdx;
            if looks.iter().any(|l| l.frame == frame) {
                return;
            }
            let Some(bbox) = nearest(&f.dets, frame) else { return };
            // Outside the track the face was lost for a reason.
            looks.push(Look { face, frame, bbox, far: far || frame < first || frame > last });
        };
        for n in 1..=p.next_to as i64 {
            add(-n, false);
            add(n, false);
        }
        for s in p.further {
            let n = (s * fps).round() as i64;
            add(-n, true);
            add(n, true);
        }
    }
    looks
}

/// The square of a `w`×`h` frame to read `bbox` (fractions) in, in pixels,
/// shifted to stay inside the frame so nothing stretches; and where the box
/// lies in it once it is [`SIDE`] pixels wide.
fn square(bbox: [f32; 4], (w, h): (usize, usize), scale: f32) -> ([f32; 4], [f32; 4]) {
    let (wf, hf) = (w as f32, h as f32);
    let b = [bbox[0] * wf, bbox[1] * hf, bbox[2] * wf, bbox[3] * hf];
    let side = ((b[2] - b[0]).max(b[3] - b[1]) * scale).clamp(1.0, wf.min(hf));
    let x = ((b[0] + b[2] - side) / 2.0).clamp(0.0, wf - side);
    let y = ((b[1] + b[3] - side) / 2.0).clamp(0.0, hf - side);
    let k = SIDE as f32 / side;
    ([x, y, x + side, y + side], [(b[0] - x) * k, (b[1] - y) * k, (b[2] - x) * k, (b[3] - y) * k])
}

/// The look's face among those found: the one lying where it was.
fn matching(spots: Vec<Spot>, expected: &[f32; 4], min_iou: f32) -> Option<Spot> {
    if min_iou <= 0.0 {
        return spots.into_iter().next();
    }
    spots
        .into_iter()
        .map(|s| (iou(&s.bbox, expected), s))
        .filter(|(o, _)| *o >= min_iou)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The direction of `reads` (embedding, quality), each counting as its
/// quality squared, as a track's own detections do.
fn mean(reads: &[&Reading]) -> Vec<f32> {
    let mut m = vec![0f32; reads.first().map_or(0, |r| r.embedding.len())];
    for r in reads {
        let w = r.quality.max(0.05).powi(2);
        m.iter_mut().zip(&r.embedding).for_each(|(a, b)| *a += w * b);
    }
    let n = dot(&m, &m).sqrt().max(1e-6);
    m.iter_mut().for_each(|x| *x /= n);
    m
}

/// What one look saw.
#[derive(Clone, Debug, PartialEq)]
struct Reading {
    embedding: Vec<f32>,
    quality: f32,
    far: bool,
}

/// One embedding from a face's looks: the ones close by decide, a look that
/// doesn't fit them is someone else, and further ones only add to what they
/// said. `None` when no look close by saw it.
fn combine(reads: &[Reading], outlier: f32) -> Option<(Vec<f32>, f32)> {
    let near: Vec<&Reading> = reads.iter().filter(|r| !r.far).collect();
    if near.is_empty() {
        return None;
    }
    let centre = mean(&near);
    let agree: Vec<&Reading> = reads.iter().filter(|r| dot(&r.embedding, &centre) >= outlier).collect();
    if !agree.iter().any(|r| !r.far) {
        // Too few to agree on anything: the clearest look close by stands.
        let best = near.iter().max_by(|a, b| a.quality.total_cmp(&b.quality))?;
        return Some((best.embedding.clone(), best.quality));
    }
    // As a track's quality: the mean of its best three.
    let mut q: Vec<f32> = agree.iter().map(|r| r.quality).collect();
    q.sort_by(|a, b| b.total_cmp(a));
    let best = &q[..q.len().min(3)];
    Some((mean(&agree), best.iter().sum::<f32>() / best.len() as f32))
}

/// Frames of `looks` (sorted by frame) in runs close enough to decode in one
/// go: ranges of `looks`.
fn windows(looks: &[Look], gap: u32) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let mut frames = 0;
    for (i, l) in looks.iter().enumerate() {
        let new_frame = i == 0 || looks[i - 1].frame != l.frame;
        match out.last_mut() {
            Some(w) if l.frame - looks[i - 1].frame <= gap && (!new_frame || frames < MAX_PICKS) => {
                w.end = i + 1;
                frames += usize::from(new_frame);
            }
            _ => {
                out.push(i..i + 1);
                frames = 1;
            }
        }
    }
    out
}

/// Read `faces` in the frames of `src` at its own resolution with `finder`,
/// its detector loaded for [`SIDE`]² pictures. Each face's reading goes to
/// `on_read` as soon as its last look is done, `on_progress` hears how far.
#[allow(clippy::too_many_arguments)]
pub fn read_faces(
    tools: &as_media::Tools,
    src: Source,
    faces: &[RescanFace],
    finder: &mut impl Finder,
    p: &RescanParams,
    cancel: &CancelToken,
    mut on_progress: impl FnMut(f64),
    mut on_read: impl FnMut(FaceRead) -> Result<(), AnalysisError>,
) -> Result<(), AnalysisError> {
    assert_eq!(finder.frame_size(), (SIDE, SIDE), "the detector is loaded for crops");
    let fps = (src.index.len().max(1) as f64 / src.index.duration().max(1e-3)).clamp(1.0, 240.0);
    let (w, h) = sample_size(src.info, (MAX_SIDE, MAX_SIDE), 2);
    let size = (w as usize, h as usize);
    let mut looks: Vec<Look> = faces.iter().enumerate().flat_map(|(i, f)| plan(i, f, fps, p)).collect();
    looks.sort_by_key(|l| (l.frame, l.face));
    let mut left: Vec<u32> = vec![0; faces.len()];
    looks.iter().for_each(|l| left[l.face] += 1);
    let mut seen: Vec<Vec<Reading>> = vec![Vec::new(); faces.len()];
    let mut tried: Vec<u32> = vec![0; faces.len()];
    for (i, f) in faces.iter().enumerate() {
        if left[i] == 0 {
            on_read(FaceRead { id: f.id, embedding: None, quality: 0.0, tried: 0, found: 0 })?;
        }
    }
    let total = looks.len().max(1);
    let search = FastParams { det_threshold: p.det_threshold, min_height: 0.0, ..FastParams::default() };
    let mut done = 0;
    for window in windows(&looks, (p.window * fps).round() as u32) {
        let part = &looks[window];
        let mut picks: Vec<FrameIdx> = part.iter().map(|l| l.frame).collect();
        picks.dedup();
        // Crops of this window, with what is known of each.
        let mut crops: Vec<(Look, Vec<u8>, [f32; 4], f32)> = Vec::with_capacity(part.len());
        decode_picks(tools, Source { ..src }, &picks, (w, h), cancel, |frame, rgb| {
            for l in part.iter().filter(|l| l.frame == frame) {
                let (region, expected) = square(l.bbox, size, p.crop_scale);
                let crop = resize_region(rgb, size, region, (SIDE, SIDE));
                crops.push((*l, crop, expected, region[2] - region[0]));
            }
            Ok(())
        })
        .map_err(media)?;
        // The face in each crop, then all of them through the embedder.
        let mut found: Vec<(Look, Vec<u8>, f32, f32)> = Vec::with_capacity(crops.len());
        for (l, crop, expected, side) in &crops {
            if let Some(s) = matching(finder.find(crop, &search)?, expected, p.min_iou) {
                // Its height as a share of the frame's.
                let height = (s.bbox[3] - s.bbox[1]) / SIDE as f32 * side / h as f32;
                found.push((*l, s.crop, height, s.score));
            }
        }
        for group in found.chunks(p.batch) {
            let input: Vec<&[u8]> = group.iter().map(|g| g.1.as_slice()).collect();
            for ((l, _, height, score), (embedding, norm)) in group.iter().zip(finder.embed(&input)?) {
                let quality = finder.quality(&search, norm, *height, *score);
                seen[l.face].push(Reading { embedding, quality, far: l.far });
            }
        }
        for l in part {
            tried[l.face] += 1;
            left[l.face] -= 1;
            if left[l.face] == 0 {
                let reads = std::mem::take(&mut seen[l.face]);
                let (embedding, quality) = combine(&reads, p.outlier).map_or((None, 0.0), |(e, q)| (Some(e), q));
                on_read(FaceRead {
                    id: faces[l.face].id,
                    embedding,
                    quality,
                    tried: tried[l.face],
                    found: reads.len() as u32,
                })?;
            }
        }
        done += part.len();
        on_progress(done as f64 / total as f64);
    }
    Ok(())
}

fn media(e: MediaError) -> AnalysisError {
    e.into()
}

/// The full pass's models as a [`Finder`], the detector loaded for crops.
pub struct LiveFinder<'a> {
    pub detector: &'a mut Scrfd,
    pub embedder: &'a mut ArcFace,
    pub params: FaceParams,
}

impl Finder for LiveFinder<'_> {
    fn frame_size(&self) -> (usize, usize) {
        self.detector.frame_size()
    }

    fn find(&mut self, rgb: &[u8], p: &FastParams) -> Result<Vec<Spot>, AnalysisError> {
        let (w, h) = self.detector.frame_size();
        Ok(self
            .detector
            .detect_many(&[rgb], p.det_threshold)?
            .pop()
            .unwrap_or_default()
            .into_iter()
            .map(|b| Spot { bbox: b.bbox, score: b.score, crop: arcface::align(rgb, w, h, &b.kps) })
            .collect())
    }

    fn embed(&mut self, crops: &[&[u8]]) -> Result<Vec<(Vec<f32>, f32)>, AnalysisError> {
        Ok(self.embedder.embed(crops)?.into_iter().map(|e| (e.v, e.norm)).collect())
    }

    /// As `FaceParams::quality` with nothing pulling it down: how long the
    /// embedding is, how big the face is in the frame.
    fn quality(&self, _: &FastParams, norm: f32, height: f32, _: f32) -> f32 {
        let p = &self.params;
        let n = ((norm - p.norm_floor) / (p.norm_full - p.norm_floor)).clamp(0.0, 1.0);
        n * (height / p.full_height).clamp(0.0, 1.0).sqrt()
    }

    fn tracker(&self, p: &FastParams, fps: f64) -> TrackerParams {
        p.tracker(fps)
    }
}

/// `finder`'s faces put to another embedder: a second opinion on the same
/// crops, from a model that learned faces its own way.
pub struct SecondOpinion<F, E> {
    pub finder: F,
    pub embed: E,
}

impl<F: Finder, E: FnMut(&[&[u8]]) -> Result<Vec<(Vec<f32>, f32)>, AnalysisError>> Finder for SecondOpinion<F, E> {
    fn frame_size(&self) -> (usize, usize) {
        self.finder.frame_size()
    }

    fn find(&mut self, rgb: &[u8], p: &FastParams) -> Result<Vec<Spot>, AnalysisError> {
        self.finder.find(rgb, p)
    }

    fn embed(&mut self, crops: &[&[u8]]) -> Result<Vec<(Vec<f32>, f32)>, AnalysisError> {
        (self.embed)(crops)
    }

    /// Only the embedding counts here: every look weighs the same.
    fn quality(&self, _: &FastParams, _: f32, _: f32, _: f32) -> f32 {
        1.0
    }

    fn tracker(&self, p: &FastParams, fps: f64) -> TrackerParams {
        self.finder.tracker(p, fps)
    }
}

/// The anime embedder as a [`Finder`]: heads have no landmarks to find, the
/// crop is the head (read it with [`RescanParams::anime`]).
pub struct HeadFinder<'a> {
    pub embedder: &'a mut Ccip,
    pub params: AnimeParams,
}

impl Finder for HeadFinder<'_> {
    fn frame_size(&self) -> (usize, usize) {
        (SIDE, SIDE)
    }

    fn find(&mut self, rgb: &[u8], _: &FastParams) -> Result<Vec<Spot>, AnalysisError> {
        let side = SIDE as f32;
        let crop = ccip::crop(rgb, SIDE, SIDE, 0.0, 0.0, side);
        Ok(vec![Spot { bbox: [0.0, 0.0, side, side], score: 1.0, crop }])
    }

    fn embed(&mut self, crops: &[&[u8]]) -> Result<Vec<(Vec<f32>, f32)>, AnalysisError> {
        Ok(self.embedder.embed(crops)?.into_iter().map(|e| (e.v, e.norm)).collect())
    }

    fn quality(&self, _: &FastParams, norm: f32, height: f32, _: f32) -> f32 {
        self.params.quality(norm, height / self.params.crop_scale)
    }

    fn tracker(&self, p: &FastParams, fps: f64) -> TrackerParams {
        p.tracker(fps)
    }
}

impl RescanParams {
    /// Drawn heads: the crop is what the anime pass embeds, and only the
    /// track's own frames count.
    pub fn anime(p: &AnimeParams) -> Self {
        Self { own_frames: true, crop_scale: p.crop_scale, min_iou: 0.0, ..Self::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(frame: FrameIdx, quality: f32) -> FaceDet {
        FaceDet { frame, bbox: [0.4, 0.4, 0.5, 0.6], score: 0.9, norm: 20.0, quality }
    }

    fn face(quality: f32, dets: Vec<FaceDet>) -> RescanFace {
        RescanFace { id: 7, shot: FrameRange::new(0, 1000), quality, dets }
    }

    fn frames(looks: &[Look]) -> Vec<FrameIdx> {
        let mut f: Vec<FrameIdx> = looks.iter().map(|l| l.frame).collect();
        f.sort_unstable();
        f
    }

    #[test]
    fn a_clear_face_is_read_at_its_clearest_moments_apart() {
        let f = face(0.8, vec![det(100, 0.9), det(103, 0.85), det(130, 0.5), det(200, 0.4), det(300, 0.3)]);
        let p = RescanParams::default();
        // 103 is too close to 100 at 24 fps; the next clearest ones count.
        assert_eq!(frames(&plan(0, &f, 24.0, &p)), [100, 130, 200]);
    }

    #[test]
    fn a_hard_face_gets_the_frames_the_pass_skipped_and_some_further_off() {
        let f = face(0.2, vec![det(99, 0.2), det(102, 0.3), det(105, 0.2)]);
        let looks = plan(0, &f, 24.0, &RescanParams::default());
        assert_eq!(frames(&looks), [90, 96, 100, 101, 102, 103, 104, 108, 114]);
        // Outside the track or further off: never alone.
        assert!(looks.iter().filter(|l| l.frame == 96 || l.frame == 114).all(|l| l.far));
        assert!(looks.iter().filter(|l| l.frame == 101 || l.frame == 103).all(|l| !l.far));
    }

    #[test]
    fn looks_stay_inside_the_shot() {
        let mut f = face(0.2, vec![det(101, 0.2)]);
        f.shot = FrameRange::new(100, 104);
        assert_eq!(frames(&plan(0, &f, 24.0, &RescanParams::default())), [100, 101, 102, 103]);
    }

    #[test]
    fn drawn_heads_keep_to_their_own_frames() {
        let f = face(0.2, vec![det(99, 0.2), det(102, 0.3)]);
        let p = RescanParams { own_frames: true, ..RescanParams::default() };
        assert_eq!(frames(&plan(0, &f, 24.0, &p)), [102]);
    }

    #[test]
    fn a_crop_at_the_frames_edge_slides_in_instead_of_stretching() {
        let (region, expected) = square([0.0, 0.0, 0.1, 0.2], (1000, 500), 2.0);
        assert_eq!(region, [0.0, 0.0, 200.0, 200.0]);
        assert_eq!(region[2] - region[0], region[3] - region[1]);
        assert_eq!(expected[0], 0.0);
        assert!((expected[3] - 160.0).abs() < 1e-3);
    }

    fn reading(v: [f32; 2], quality: f32, far: bool) -> Reading {
        Reading { embedding: v.to_vec(), quality, far }
    }

    #[test]
    fn someone_else_further_off_doesnt_count() {
        let (e, _) = combine(&[reading([1.0, 0.0], 0.8, false), reading([0.0, 1.0], 0.9, true)], 0.2).unwrap();
        assert!((e[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn looks_further_off_never_stand_alone() {
        assert_eq!(combine(&[reading([1.0, 0.0], 0.9, true)], 0.2), None);
        assert_eq!(combine(&[], 0.2), None);
    }

    #[test]
    fn the_clear_look_counts_most() {
        let (e, q) = combine(&[reading([1.0, 0.0], 0.9, false), reading([0.8, 0.6], 0.1, false)], 0.2).unwrap();
        assert!(e[0] > 0.99);
        assert!((q - 0.5).abs() < 1e-5);
    }

    #[test]
    fn close_frames_decode_together() {
        let l = |frame| Look { face: 0, frame, bbox: [0.0; 4], far: false };
        assert_eq!(windows(&[l(1), l(5), l(5), l(200), l(205)], 96), [0..3, 3..5]);
        assert!(windows(&[], 96).is_empty());
    }

    #[test]
    fn one_decoder_is_asked_for_a_bounded_number_of_frames() {
        let looks: Vec<Look> =
            (0..MAX_PICKS as u32 + 3).map(|f| Look { face: 0, frame: f, bbox: [0.0; 4], far: false }).collect();
        assert_eq!(windows(&looks, 96).len(), 2);
    }
}
