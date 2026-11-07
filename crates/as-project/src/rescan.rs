//! Rescan: a second look at faces. What it finds are suggestions; only the
//! person's answers change the cast.

use as_api::TrackAssign;
use as_core::CharacterId;
use as_db::IdentityTrack;
use as_identity::{CharKey, Judged, Params, Question, Read, Space, TrackIn, judge};

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

impl Project {
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
