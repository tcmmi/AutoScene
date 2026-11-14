//! The picture a media's posters show. At ingest, before anything is known
//! of the footage, it is the frame a tenth of the way in. Once its faces are
//! found and grouped into characters, the frame is picked: the one that
//! shows the most of the main cast, each of them clearly (a face big enough,
//! readable, wholly in the picture), inside a shot and away from its cuts,
//! and not in the title sequence or the credits when the rest has one.
//! A poster made of a picked frame carries the frame in its file name
//! ([`crate::project::picked_frame`]), so everything that needs to know what
//! the poster shows (finding a moved file again by its picture) reads it
//! from there.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use as_analysis::tracks::{FaceDet, dets_from_bytes};
use as_api::{MediaStatus, Segment, SegmentKind, StageState};
use as_core::{CancelToken, CharacterId, FrameIdx, MediaId};
use as_db::CharacterRow;
use as_media::{Source, Tools};

use crate::project::picked_frame;
use crate::{Project, ProjectError, ProjectResult};

/// Counts up when the pick changes: posters picked by an older rule are
/// picked again (a newer rule's are never picked again by an older one).
const POSTER_RULE: u32 = 1;
/// `poster_picked:<media>` holds the rule a media's poster frame was picked
/// by (also when no frame showed the main cast and the poster stayed).
const POSTER_PICKED: &str = "poster_picked:";

/// A face counts for a poster from this quality on (what its detection says
/// about who it is: a profile, a blur, a dark face say little) ...
const CLEAR: f32 = 0.6;
/// ... from this height, as a share of the frame's ...
const TALL: f32 = 0.08;
/// ... and this far from the frame's edges: a face the frame cuts is not seen.
const INSIDE: f32 = 0.01;
/// A bigger face counts for more, up to this height.
const FULL: f32 = 0.25;
/// Frames this close to a shot's ends are left out: the cut itself, and a
/// blend at the end of a shot.
const FROM_CUT: FrameIdx = 3;
/// A title sequence and the credits show the cast too, under lettering, and
/// nothing here reads lettering: the first and the last stretch of a media
/// are left out while the rest of it has a frame ...
const OPENING_SECS: f64 = 90.0;
const CLOSING_SECS: f64 = 60.0;
/// ... each at most this share of the media (a clip of a minute keeps most of itself).
const ENDS_SHARE: f64 = 0.1;
/// The main cast as the Cast page splits it (`splitTiers` in the interface):
/// characters with a name or this long on screen ...
const MAIN_SECS: f64 = 60.0;
/// ... everyone when the cast is this small ...
const SMALL_CAST: usize = 8;
/// ... and the most seen ones when nobody is named or on screen that long.
const AT_LEAST: usize = 6;

pub(crate) fn picked_key(id: MediaId) -> String {
    format!("{POSTER_PICKED}{id}")
}

/// The main cast among `cast` (most on screen first, as the database lists it).
fn main_cast(cast: &[CharacterRow]) -> HashSet<CharacterId> {
    let shown: Vec<&CharacterRow> = cast.iter().filter(|c| !c.hidden && c.tracks > 0).collect();
    if shown.len() <= SMALL_CAST {
        return shown.iter().map(|c| c.id).collect();
    }
    let main: HashSet<CharacterId> =
        shown.iter().filter(|c| c.name.is_some() || c.screen_sec >= MAIN_SECS).map(|c| c.id).collect();
    if main.is_empty() { shown.iter().take(AT_LEAST).map(|c| c.id).collect() } else { main }
}

/// The frame that shows the most of the cast clearly, and how many of them.
/// `faces`: who is seen and where (a detection of one of their tracks);
/// `fits`: whether a frame may be a poster at all. More people win; then the
/// clearer and bigger faces; then the earlier frame.
fn pick_frame<K: Copy + Ord>(
    faces: impl IntoIterator<Item = (K, FaceDet)>,
    fits: impl Fn(FrameIdx) -> bool,
) -> Option<(FrameIdx, usize)> {
    let mut frames: BTreeMap<FrameIdx, BTreeMap<K, f32>> = BTreeMap::new();
    for (who, d) in faces {
        let b = d.bbox;
        let whole = b[0] >= INSIDE && b[1] >= INSIDE && b[2] <= 1.0 - INSIDE && b[3] <= 1.0 - INSIDE;
        if d.quality >= CLEAR && d.height() >= TALL && whole && fits(d.frame) {
            let worth = d.quality * (d.height() / FULL).min(1.0);
            let best = frames.entry(d.frame).or_default().entry(who).or_insert(0.0);
            *best = best.max(worth);
        }
    }
    frames
        .into_iter()
        .map(|(f, seen)| (seen.len(), seen.values().sum::<f32>(), f))
        .max_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(b.2.cmp(&a.2)))
        .map(|(n, _, f)| (f, n))
}

/// Whether `frame` lies inside a shot of `segments`, away from its cuts (a
/// dissolve, a fade, black frames and flashes are no shots).
fn in_a_shot(segments: &[Segment], frame: FrameIdx) -> bool {
    let at = segments.partition_point(|s| s.end <= frame);
    segments.get(at).is_some_and(|s| {
        s.kind == SegmentKind::Shot && !s.tiny && frame >= s.start + FROM_CUT && frame + FROM_CUT < s.end
    })
}

/// The frames of a media of `frames` frames at `fps` between its opening and
/// its closing stretch.
fn body(frames: FrameIdx, fps: f64) -> std::ops::Range<FrameIdx> {
    let most = (f64::from(frames) * ENDS_SHARE) as FrameIdx;
    let ends = |secs: f64| ((secs * fps.max(1.0)) as FrameIdx).min(most);
    ends(OPENING_SECS)..frames - ends(CLOSING_SECS)
}

impl Project {
    /// Ready media whose faces are found and whose poster frame was not
    /// picked yet (or by an older rule): after an update, a new ingest or a
    /// face pass. In episode order.
    pub fn posters_to_pick(&self) -> ProjectResult<Vec<MediaId>> {
        let picked: HashSet<String> = (self.db().meta_range(POSTER_PICKED)?.into_iter())
            .filter(|(_, rule)| rule.parse::<u32>().is_ok_and(|r| r >= POSTER_RULE))
            .map(|(key, _)| key)
            .collect();
        Ok((self.media_list_uncounted()?.iter())
            .filter(|m| m.status == MediaStatus::Ready && m.faces == StageState::Ready && m.poster.is_some())
            .filter(|m| !picked.contains(&picked_key(m.id)))
            .map(|m| m.id)
            .collect())
    }

    /// A media's faces were found again or grouped anew: its poster frame is
    /// picked again by the next [`Project::pick_poster`] round.
    pub fn poster_pick_again(&self, id: MediaId) -> ProjectResult<()> {
        Ok(self.db().delete_meta(&picked_key(id))?)
    }

    /// The frame of `id` (`frames` frames at `fps`) that shows the most of
    /// the main cast clearly, and how many of them; `None` when no frame
    /// shows any of them so.
    pub(crate) fn poster_frame(
        &self,
        id: MediaId,
        frames: FrameIdx,
        fps: f64,
    ) -> ProjectResult<Option<(FrameIdx, usize)>> {
        let main = main_cast(&self.db().characters()?);
        let segments = self.segments(id)?;
        let faces: Vec<(CharacterId, FaceDet)> = (self.db().media_cast_dets(id)?.into_iter())
            .filter(|(who, _)| main.contains(who))
            .flat_map(|(who, dets)| dets_from_bytes(&dets).unwrap_or_default().into_iter().map(move |d| (who, d)))
            .collect();
        // (Without shots, as when they are looked for again, any frame of the file may be it.)
        let in_shot = |f: FrameIdx| f < frames && (segments.is_empty() || in_a_shot(&segments, f));
        let body = body(frames, fps);
        Ok(pick_frame(faces.iter().copied(), |f| body.contains(&f) && in_shot(f))
            .or_else(|| pick_frame(faces.iter().copied(), in_shot)))
    }

    /// Pick the frame a media's posters show and make them of it. `Ok(true)`:
    /// the posters changed. The pick is remembered either way, so it is not
    /// made again at every launch; a media that is not ready, whose faces
    /// are not found yet or that has no poster is left for later.
    pub fn pick_poster(&self, tools: &Tools, id: MediaId, cancel: &CancelToken) -> ProjectResult<bool> {
        let m = match self.media(id) {
            Ok(m) if m.status == MediaStatus::Ready && m.faces == StageState::Ready && m.poster.is_some() => m,
            Ok(_) | Err(ProjectError::NotFound(_)) => return Ok(false),
            Err(e) => return Err(e),
        };
        let (_, info, index) = self.ready_source(id)?;
        let fps = f64::from(index.len()) / index.duration().max(1e-6);
        let picked = self.poster_frame(id, index.len(), fps)?;
        let done = |changed: bool| -> ProjectResult<bool> {
            self.db().set_meta(&picked_key(id), &POSTER_RULE.to_string())?;
            Ok(changed)
        };
        let Some((frame, people)) = picked else { return done(false) };
        let (big, small) = self.poster_paths(id, Some(frame));
        if picked_frame(&m) == Some(frame) && big.is_file() && small.is_file() {
            return done(false);
        }
        let path = PathBuf::from(&m.path);
        self.write_posters(tools, id, Source { path: &path, info: &info, index: &index }, Some(frame), cancel)?;
        let (big_s, small_s) = (big.display().to_string(), small.display().to_string());
        if !self.db().set_posters(id, &big_s, &small_s)? {
            // Removed while we worked.
            let _ = std::fs::remove_file(&big);
            let _ = std::fs::remove_file(&small);
            return Ok(false);
        }
        self.remove_stale_posters(&m, [Some(&big_s), Some(&small_s)]);
        tracing::info!(media = %id, frame, people, "poster frame picked");
        done(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(frame: FrameIdx, x: f32, height: f32, quality: f32) -> FaceDet {
        FaceDet { frame, bbox: [x, 0.3, x + height * 0.5, 0.3 + height], score: 0.9, norm: 20.0, quality }
    }

    #[test]
    fn the_frame_with_the_most_people_wins() {
        // Frame 30: one big, clear face. Frame 60: three smaller ones.
        let faces = [
            ("a", face(30, 0.4, 0.5, 0.95)),
            ("a", face(60, 0.1, 0.12, 0.7)),
            ("b", face(60, 0.4, 0.12, 0.7)),
            ("c", face(60, 0.7, 0.12, 0.7)),
            ("b", face(90, 0.4, 0.3, 0.9)),
            ("c", face(90, 0.7, 0.3, 0.9)),
        ];
        assert_eq!(pick_frame(faces, |_| true), Some((60, 3)));
        // Two tracks of one character in a frame are one person.
        let twice = [("a", face(30, 0.1, 0.2, 0.9)), ("a", face(30, 0.6, 0.2, 0.9)), ("b", face(60, 0.4, 0.2, 0.9))];
        assert_eq!(pick_frame(twice, |_| true).map(|(_, n)| n), Some(1));
    }

    #[test]
    fn among_frames_with_as_many_people_the_clearer_and_bigger_faces_win() {
        let faces = [
            ("a", face(30, 0.2, 0.10, 0.65)),
            ("b", face(30, 0.6, 0.10, 0.65)),
            ("a", face(60, 0.2, 0.22, 0.9)),
            ("b", face(60, 0.6, 0.22, 0.9)),
            ("a", face(90, 0.2, 0.22, 0.9)),
            ("b", face(90, 0.6, 0.22, 0.9)),
        ];
        // 60 and 90 are as good: the earlier one.
        assert_eq!(pick_frame(faces, |_| true), Some((60, 2)));
        // A face bigger than a quarter of the frame counts no more for it.
        let huge = [("a", face(30, 0.2, 0.6, 0.9)), ("a", face(60, 0.2, 0.3, 0.9))];
        assert_eq!(pick_frame(huge, |_| true), Some((30, 1)));
    }

    #[test]
    fn a_face_that_is_small_unclear_or_cut_by_the_frame_does_not_count() {
        let whole = ("a", face(90, 0.4, 0.2, 0.9));
        let small = ("b", face(30, 0.4, 0.05, 0.9));
        let unclear = ("c", face(30, 0.6, 0.2, 0.4));
        let cut = ("d", FaceDet { frame: 30, bbox: [0.0, 0.3, 0.1, 0.5], score: 0.9, norm: 20.0, quality: 0.9 });
        assert_eq!(pick_frame([small, unclear, cut, whole], |_| true), Some((90, 1)));
        assert_eq!(pick_frame([small, unclear, cut], |_| true), None);
        // Nor does a face in a frame that is no poster.
        assert_eq!(pick_frame([whole], |f| f != 90), None);
        assert_eq!(pick_frame(std::iter::empty::<(&str, FaceDet)>(), |_| true), None);
    }

    #[test]
    fn a_poster_frame_lies_in_a_shot_away_from_its_cuts() {
        let seg = |start, end, kind, tiny| Segment { start, end, kind, tiny };
        let segments = [
            seg(0, 100, SegmentKind::Shot, false),
            seg(100, 120, SegmentKind::Dissolve, false),
            seg(120, 122, SegmentKind::Shot, true),
            seg(122, 300, SegmentKind::Shot, false),
        ];
        assert!(in_a_shot(&segments, 50));
        assert!(!in_a_shot(&segments, 1) && in_a_shot(&segments, 3));
        assert!(!in_a_shot(&segments, 98) && in_a_shot(&segments, 96));
        assert!(!in_a_shot(&segments, 110), "a dissolve");
        assert!(!in_a_shot(&segments, 121), "a shot of two frames");
        assert!(in_a_shot(&segments, 200) && !in_a_shot(&segments, 300) && !in_a_shot(&segments, 5000));
        assert!(!in_a_shot(&[], 10));
    }

    #[test]
    fn the_opening_and_the_closing_stretch_are_left_out() {
        // An episode of 22 minutes at 24 frames a second: the first minute and a half, the last minute.
        assert_eq!(body(31_680, 24.0), 2_160..30_240);
        // A clip of a minute: a tenth at either end.
        assert_eq!(body(1_440, 24.0), 144..1_296);
        assert_eq!(body(0, 24.0), 0..0);
        // A frame in the body wins over one with more people in the titles ...
        let faces =
            [("a", face(300, 0.1, 0.2, 0.9)), ("b", face(300, 0.5, 0.2, 0.9)), ("a", face(5_000, 0.3, 0.2, 0.9))];
        let body = body(31_680, 24.0);
        let pick = |faces: &[(&'static str, FaceDet)]| {
            pick_frame(faces.iter().copied(), |f| body.contains(&f))
                .or_else(|| pick_frame(faces.iter().copied(), |_| true))
        };
        assert_eq!(pick(&faces), Some((5_000, 1)));
        // ... and the titles will do when the body shows nobody.
        assert_eq!(pick(&faces[..2]), Some((300, 2)));
    }

    fn character(n: u32, name: Option<&str>, secs: f64, hidden: bool) -> CharacterRow {
        CharacterRow {
            id: CharacterId::new(),
            number: n,
            name: name.map(str::to_owned),
            hidden,
            cover: None,
            tracks: 5,
            shots: 5,
            media: 1,
            screen_sec: secs,
            unsure: 0,
            waiting: 0,
        }
    }

    #[test]
    fn the_main_cast_is_the_cast_pages() {
        // A small cast: everyone who is shown.
        let small: Vec<CharacterRow> =
            (0..8).map(|n| character(n, None, 5.0, false)).chain([character(8, None, 300.0, true)]).collect();
        assert_eq!(main_cast(&small).len(), 8);
        // A bigger one: the named and those a minute on screen.
        let mut big: Vec<CharacterRow> = (0..12).map(|n| character(n, None, 200.0 - 15.0 * n as f64, false)).collect();
        big[11].name = Some("Walter".into());
        let main = main_cast(&big);
        assert_eq!(main.len(), 11);
        assert!(main.contains(&big[11].id) && main.contains(&big[9].id) && !main.contains(&big[10].id));
        // Nobody named or long on screen: the six most seen.
        let extras: Vec<CharacterRow> = (0..12).map(|n| character(n, None, 40.0 - n as f64, false)).collect();
        let main = main_cast(&extras);
        assert_eq!(main.len(), 6);
        assert!(main.contains(&extras[5].id) && !main.contains(&extras[6].id));
    }
}
