//! Scenes (an optional mode, the library's setting): the pictures of every
//! shot, read by a pass of their own, and which shots make a scene. That is
//! worked out from the pictures, the shots and the cast when it is asked
//! for (`as_analysis::scenes`), and kept nowhere: the cast changes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use as_analysis::scenes::{self, Piece, Recipe};
use as_analysis::{FrameSignals, pictures, signals};
use as_api::{ContentKind, MediaInfo, SegmentKind, StageState};
use as_core::{FrameIdx, FrameRange, MediaId, Msg, msg};

use crate::{Project, ProjectError, ProjectResult};

/// What the engine needs to read the pictures of a media's shots.
#[derive(Clone, Debug)]
pub struct ScenesPlan {
    pub path: PathBuf,
    pub info: MediaInfo,
    /// `FrameIndex::to_bytes`.
    pub index: Vec<u8>,
    /// The shots whose pictures are still to read, `(start, end)`, sorted.
    pub shots: Vec<(FrameIdx, FrameIdx)>,
    /// How many shots the media has.
    pub total: usize,
}

fn piece(kind: SegmentKind) -> Piece {
    match kind {
        SegmentKind::Shot => Piece::Shot,
        SegmentKind::Dissolve => Piece::Dissolve,
        SegmentKind::FadeOut | SegmentKind::FadeIn | SegmentKind::Black => Piece::Dark,
        SegmentKind::Flash => Piece::Other,
    }
}

impl Project {
    pub fn set_scenes_state(&self, id: MediaId, state: StageState, error: Option<&Msg>) -> ProjectResult<()> {
        Ok(self.db().set_scenes_state(id, state, error)?)
    }

    /// Media whose shots are found and whose pictures aren't read, in
    /// episode order — an interrupted run too: for a launch, when nothing
    /// runs yet.
    pub fn pending_scenes(&self) -> ProjectResult<Vec<MediaId>> {
        Ok(self.db().pending_scenes()?)
    }

    /// Media whose pictures wait to be read, in episode order (those being
    /// read now left as they are).
    pub fn waiting_scenes(&self) -> ProjectResult<Vec<MediaId>> {
        Ok(self.db().waiting_scenes()?)
    }

    /// What is left of the scenes pass of `id`: its shots without pictures.
    pub fn scenes_plan(&self, id: MediaId) -> ProjectResult<ScenesPlan> {
        let (m, info, index) = self.ready_source(id)?;
        if m.shots != StageState::Ready {
            return Err(ProjectError::NotReady(msg!(
                "analysis.shotsNotFound",
                "Shots of this media aren't found yet."
            )));
        }
        let all: Vec<(FrameIdx, FrameIdx)> =
            self.segments(id)?.into_iter().filter(|s| s.kind == SegmentKind::Shot).map(|s| (s.start, s.end)).collect();
        let have: HashSet<FrameIdx> = self.db().pictured_shots(id)?.into_iter().collect();
        let shots = all.iter().filter(|s| !have.contains(&s.0)).copied().collect();
        Ok(ScenesPlan { path: PathBuf::from(&m.path), info, index: index.to_bytes(), shots, total: all.len() })
    }

    /// Store the pictures of shots as the engine hands them out: each
    /// shot's first frame and its vectors (`as_analysis::pictures::to_bytes`).
    pub fn store_shot_pictures(&self, id: MediaId, shots: &[(FrameIdx, Vec<u8>)]) -> ProjectResult<()> {
        if shots.iter().any(|(_, vectors)| pictures::from_bytes(vectors).is_none()) {
            return Err(ProjectError::Invalid(msg!("analysis.unreadablePictures", "unreadable pictures")));
        }
        Ok(self.db().put_shot_pictures(id, shots)?)
    }

    /// The pictures of `id` are all read: its scenes can be told.
    pub fn finish_scenes(&self, id: MediaId) -> ProjectResult<()> {
        let left = self.scenes_plan(id)?.shots.len();
        if left > 0 {
            return Err(ProjectError::Invalid(Msg::plain(format!("{left} shots are still without their pictures"))));
        }
        self.set_scenes_state(id, StageState::Ready, None)
    }

    /// The scene of every shot of `id`, in order: the shot's frames and the
    /// number of its scene, counted from 0. `None` while its pictures
    /// aren't all read (the mode is off, or the pass hasn't run).
    pub fn scenes(&self, id: MediaId) -> ProjectResult<Option<Vec<(FrameRange, usize)>>> {
        let m = self.media(id)?;
        if m.scenes != StageState::Ready {
            return Ok(None);
        }
        let segments = self.segments(id)?;
        let (mut shots, between) = scenes::shots(segments.iter().map(|s| (s.start, s.end, piece(s.kind))));
        let mut stored: HashMap<FrameIdx, Vec<u8>> = self.db().shot_pictures(id)?.into_iter().collect();
        let read = |s: &scenes::Shot| stored.remove(&s.start).and_then(|b| pictures::from_bytes(&b));
        let Some(vectors) = shots.iter().map(read).collect::<Option<Vec<_>>>() else {
            tracing::warn!(file = %m.file_name, "a shot has no pictures though its scenes are marked ready");
            return Ok(None);
        };
        // Who the cast shows in each shot, as numbers of this media's own.
        let mut numbers: HashMap<String, usize> = HashMap::new();
        for (shot, character) in self.db().shot_cast(id)? {
            let Ok(i) = shots.binary_search_by_key(&shot, |s| s.start) else { continue };
            let next = numbers.len();
            let c = *numbers.entry(character).or_insert(next);
            if !shots[i].cast.contains(&c) {
                shots[i].cast.push(c);
            }
        }
        for s in &mut shots {
            s.cast.sort_unstable();
        }
        let fps = m.fps.map_or(24.0, |f| f.to_f64());
        let secs: Vec<f32> = shots.iter().map(|s| ((s.end - s.start) as f64 / fps) as f32).collect();
        let recipe = match self.content()? {
            ContentKind::Anime | ContentKind::Animation => Recipe::DRAWN,
            ContentKind::Live | ContentKind::Mixed => Recipe::LIVE,
        };
        let sig = if recipe.reads_colours() { self.frame_signals(id, &m.file_name)? } else { None };
        let of = scenes::scenes(&shots, &between, &secs, sig.as_deref(), &vectors, &recipe);
        Ok(Some(shots.iter().zip(of).map(|(s, scene)| (FrameRange::new(s.start, s.end), scene)).collect()))
    }

    /// The signals of every frame of `id`, as the shots pass stored them;
    /// `None` when they don't cover it from the first frame on (the scenes
    /// are then read without the colours).
    fn frame_signals(&self, id: MediaId, file: &str) -> ProjectResult<Option<Vec<FrameSignals>>> {
        let mut all: Vec<FrameSignals> = Vec::new();
        for (start, data) in self.db().shot_chunks(id)? {
            match signals::decode(&data) {
                Some(chunk) if start as usize == all.len() => all.extend(chunk),
                _ => {
                    tracing::warn!(file, start, "the stored signals have a hole: scenes are read without the colours");
                    return Ok(None);
                }
            }
        }
        Ok(Some(all))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_segment_has_its_piece() {
        assert_eq!(piece(SegmentKind::Shot), Piece::Shot);
        assert_eq!(piece(SegmentKind::Dissolve), Piece::Dissolve);
        for dark in [SegmentKind::FadeOut, SegmentKind::FadeIn, SegmentKind::Black] {
            assert_eq!(piece(dark), Piece::Dark);
        }
        assert_eq!(piece(SegmentKind::Flash), Piece::Other);
    }
}
