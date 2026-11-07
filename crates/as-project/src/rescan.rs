//! Rescan: a second look at faces. What it finds are suggestions; only the
//! person's answers change the cast.

use std::path::PathBuf;
use std::sync::Arc;

use as_analysis::rescan::RescanFace;
use as_analysis::tracks;
use as_api::{SegmentKind, TrackAssign};
use as_core::{CharacterId, FrameIndex, FrameRange, MediaId};
use as_db::IdentityTrack;
use as_identity::{CharKey, Judged, Params, Question, Read, Space, TrackIn, judge};
use as_media::MediaInfo;

use crate::cast::IdentityInput;
use crate::{Project, ProjectResult};

/// The cast as it stands, ready to be asked about faces.
#[derive(Clone)]
pub struct RescanCast {
    pub rows: Vec<IdentityTrack>,
    /// Per input track, its row in `rows`.
    pub row_of: Vec<usize>,
    pub tracks: Vec<TrackIn>,
    /// Per track, the first frame of its shot.
    pub shots: Vec<u32>,
    pub members: Vec<(CharKey, Vec<usize>)>,
    /// Per track, the character it is on now.
    pub now: Vec<Option<CharKey>>,
    /// A character's key is its index here.
    pub characters: Vec<CharacterId>,
    pub params: Params,
    pub space: Option<Space>,
}

impl RescanCast {
    /// Who each of `subjects` is (indexes into `tracks`), with sharper `reads`
    /// where there are any. Nothing is stored.
    pub fn judge(&self, subjects: &[usize], reads: &[Read]) -> Vec<Judged> {
        let q = Question {
            tracks: &self.tracks,
            shots: &self.shots,
            members: &self.members,
            now: &self.now,
            subjects,
            reads,
        };
        judge(&q, &self.params, self.space.as_ref())
    }
}

/// What it takes to read a media's faces again.
pub struct RescanSource {
    pub path: PathBuf,
    pub info: MediaInfo,
    pub index: Arc<FrameIndex>,
    /// Its shots, in order.
    pub shots: Vec<FrameRange>,
}

impl RescanCast {
    /// `tracks` (indexes into [`Self::tracks`]) of `media` as the reader takes
    /// them; `id` is the index. A track outside every shot keeps to itself.
    pub fn faces_of(&self, media: MediaId, tracks: &[usize], shots: &[FrameRange]) -> Vec<RescanFace> {
        tracks
            .iter()
            .filter_map(|&i| {
                let r = &self.rows[self.row_of[i]];
                (r.media_id == media).then(|| RescanFace {
                    id: i as u32,
                    shot: shots
                        .iter()
                        .find(|s| s.contains(r.start))
                        .copied()
                        .unwrap_or_else(|| FrameRange::new(r.start, r.end)),
                    quality: r.quality,
                    dets: tracks::dets_from_bytes(&r.det_bytes).unwrap_or_default(),
                })
            })
            .collect()
    }
}

impl Project {
    /// Where a media's frames are, or why they can't be read.
    pub fn rescan_source(&self, media: MediaId) -> ProjectResult<RescanSource> {
        let (m, info, index) = self.ready_source(media)?;
        let shots = self
            .segments(media)?
            .into_iter()
            .filter(|s| s.kind == SegmentKind::Shot)
            .map(|s| FrameRange::new(s.start, s.end))
            .collect();
        Ok(RescanSource { path: PathBuf::from(m.path), info, index, shots })
    }

    /// The cast as [`RescanCast`] needs it; reads only.
    pub fn rescan_cast(&self) -> ProjectResult<RescanCast> {
        let rows = self.db().identity_tracks()?;
        let characters: Vec<CharacterId> = self.db().characters()?.into_iter().map(|c| c.id).collect();
        let cast = IdentityInput::new(&rows, characters, &self.grids()?);
        let members = cast.members(&rows);
        let shots = cast.rows.iter().map(|&r| rows[r].shot).collect();
        // A face named by its body is on that character too, unlike
        // `TrackIn::previous`; an unsure face is on no one yet.
        let now = cast
            .rows
            .iter()
            .map(|&r| {
                let on = matches!(rows[r].assign, TrackAssign::Auto | TrackAssign::Pinned);
                rows[r].character_id.filter(|_| on).and_then(|c| cast.key.get(&c).copied())
            })
            .collect();
        let params = self.identity_params()?;
        let space = self.grouped_space()?;
        Ok(RescanCast {
            row_of: cast.rows,
            tracks: cast.tracks,
            shots,
            members,
            now,
            characters: cast.existing,
            rows,
            params,
            space,
        })
    }
}
