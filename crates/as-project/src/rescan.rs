//! Rescan: a second look at faces. What it finds are suggestions, kept apart
//! from the cast; only the person's answers change it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use as_analysis::rescan::RescanFace as FaceToRead;
use as_analysis::tracks;
use as_api::{
    CastEdit, RescanAnswer, RescanFace, RescanPair, RescanRequest, RescanResults, RescanTally, RescanVerdict,
    SegmentKind, TrackAssign, TrackFilter,
};
use as_core::{CharacterId, FrameIndex, FrameRange, MediaId};
use as_db::{IdentityTrack, RescanFaceRow};
use as_identity::{CharKey, Judged, Params, Question, Read, Space, TrackIn, Verdict, judge};
use as_media::MediaInfo;

use crate::cast::{IdentityInput, NAME_ANCHORS, anchors, embedder_of};
use crate::{Project, ProjectResult};

/// Where the last rescan's tally is kept.
const TALLY: &str = "rescan_tally";
/// Faces of a character shown beside a "same person?" question.
const PAIR_FACES: usize = 6;
/// A face no look found is only called no face when the pass was unsure of
/// it too: its best detection score and how long its embedding was.
const NO_FACE_SCORE: f32 = 0.7;
const NO_FACE_NORM: f32 = 16.0;

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

/// What reading a face again gave.
#[derive(Clone, Debug, PartialEq)]
pub struct DoneRead {
    /// Index into [`RescanCast::tracks`].
    pub track: usize,
    /// `as_analysis::tracks::embedding_to_bytes`; empty: no look read it.
    pub embedding: Vec<u8>,
    pub quality: f32,
    pub tried: u32,
    pub found: u32,
}

/// What it takes to read a media's faces again.
pub struct RescanSource {
    pub path: PathBuf,
    pub info: MediaInfo,
    pub index: Arc<FrameIndex>,
    /// Its shots, in order.
    pub shots: Vec<FrameRange>,
    /// Its faces came from Fast Scan's models, or are anime heads: they are
    /// read by what found them.
    pub fast: bool,
    pub anime: bool,
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

    fn row(&self, i: usize) -> &IdentityTrack {
        &self.rows[self.row_of[i]]
    }

    /// The faces `request` asks about: never one the person confirmed (their
    /// word stands) or one named by its body (no face to go by).
    pub fn subjects(&self, request: &RescanRequest) -> Vec<usize> {
        let tracks: HashSet<i64> = request.tracks.iter().map(|&t| i64::from(t)).collect();
        let characters: HashSet<CharacterId> = request.characters.iter().copied().collect();
        (0..self.tracks.len())
            .filter(|&i| {
                let r = self.row(i);
                r.assign != TrackAssign::Pinned
                    && !r.via_body
                    && (tracks.contains(&r.id)
                        || r.character_id.is_some_and(|c| characters.contains(&c))
                        || (request.everyone_else && r.assign == TrackAssign::None))
            })
            .collect()
    }

    /// Of `subjects`, those worth reading again: a clear face gains nothing
    /// (measured on the Miraculous film: fewer of them came out right).
    pub fn unclear(&self, subjects: &[usize]) -> Vec<usize> {
        subjects.iter().copied().filter(|&i| self.tracks[i].quality < self.params.core_quality).collect()
    }

    /// The media of `tracks`, each once.
    pub fn media_of(&self, tracks: &[usize]) -> Vec<MediaId> {
        let mut media: Vec<MediaId> = tracks.iter().map(|&i| self.row(i).media_id).collect();
        media.sort_unstable();
        media.dedup();
        media
    }

    /// `tracks` of `media` as the reader takes them; `id` is the index. A
    /// track outside every shot keeps to itself.
    pub fn faces_of(&self, media: MediaId, tracks: &[usize], shots: &[FrameRange]) -> Vec<FaceToRead> {
        tracks
            .iter()
            .filter_map(|&i| {
                let r = self.row(i);
                (r.media_id == media).then(|| FaceToRead {
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

    /// Whether the pass itself had little faith in this face.
    fn barely_a_face(&self, i: usize) -> bool {
        let dets = tracks::dets_from_bytes(&self.row(i).det_bytes).unwrap_or_default();
        !dets.is_empty() && dets.iter().all(|d| d.score < NO_FACE_SCORE && d.norm < NO_FACE_NORM)
    }

    /// `judged` as what is kept for the person, and the tally.
    fn suggestions(&self, judged: &[Judged], reads: &HashMap<usize, &DoneRead>) -> (Vec<RescanFaceRow>, RescanTally) {
        let mut tally = RescanTally::default();
        let mut rows = Vec::new();
        let id = |k: CharKey| self.characters[k as usize];
        for j in judged {
            let read = reads.get(&j.track);
            let score_of = |k: CharKey| j.scores.iter().find(|s| s.1 == k).map_or(0.0, |s| s.0);
            let next_after = |k: CharKey| j.scores.iter().find(|s| s.1 != k).map_or(0.0, |s| s.0);
            let (verdict, target, other, score, score2) = match j.verdict {
                Verdict::Elsewhere(k) => (RescanVerdict::Elsewhere, Some(id(k)), None, score_of(k), next_after(k)),
                Verdict::Newly(k) => (RescanVerdict::Newly, Some(id(k)), None, score_of(k), next_after(k)),
                Verdict::Maybe(k) => (RescanVerdict::Maybe, Some(id(k)), None, score_of(k), next_after(k)),
                Verdict::Between(a, b) => (RescanVerdict::Between, Some(id(a)), Some(id(b)), score_of(a), score_of(b)),
                Verdict::Doubt => {
                    let on = self.now[j.track];
                    (RescanVerdict::Doubt, on.map(id), None, on.map_or(0.0, score_of), 0.0)
                }
                Verdict::Nobody if read.is_some_and(|r| r.tried > 0 && r.found == 0) && self.barely_a_face(j.track) => {
                    (RescanVerdict::NoFace, None, None, 0.0, 0.0)
                }
                Verdict::Nobody => {
                    tally.nobody += 1;
                    continue;
                }
                Verdict::Right | Verdict::Thin => {
                    tally.right += 1;
                    continue;
                }
            };
            let sharp = read.filter(|r| !r.embedding.is_empty());
            rows.push(RescanFaceRow {
                track_id: self.row(j.track).id,
                verdict,
                target,
                other,
                score,
                score2,
                looks: read.map_or(0, |r| r.found),
                read_quality: sharp.map(|r| r.quality),
                emb: sharp.map(|r| r.embedding.clone()),
            });
        }
        (rows, tally)
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
        let models = self.db().ready_faces_models()?;
        let model = models.iter().find(|(id, _)| *id == media).and_then(|(_, m)| m.as_deref()).unwrap_or_default();
        let (fast, anime) =
            (embedder_of(model) == as_models::ids::SFACE, embedder_of(model) == as_models::ids::ANIME_CCIP);
        Ok(RescanSource { path: PathBuf::from(m.path), info, index, shots, fast, anime })
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

    /// Judge `subjects` with what was read and keep what comes of it for the
    /// person. `unread`: faces whose file couldn't be read. Writes suggestions
    /// only: no track, character or cast revision changes.
    pub fn rescan_keep(
        &self,
        cast: &RescanCast,
        subjects: &[usize],
        reads: &[DoneRead],
        unread: u32,
    ) -> ProjectResult<RescanTally> {
        let sharp: Vec<Read> = reads
            .iter()
            .filter_map(|r| {
                let embedding = tracks::embedding_from_bytes(&r.embedding)?;
                Some(Read { track: r.track, embedding, quality: r.quality })
            })
            .collect();
        let by_track: HashMap<usize, &DoneRead> = reads.iter().map(|r| (r.track, r)).collect();
        let judged = cast.judge(subjects, &sharp);
        let (rows, mut tally) = cast.suggestions(&judged, &by_track);
        let ids: Vec<i64> = subjects.iter().map(|&i| cast.row(i).id).collect();
        self.db().put_rescan_faces(&ids, &rows)?;
        tally.looked = subjects.len() as u32;
        tally.read = sharp.len() as u32;
        tally.unread = unread;
        let json = serde_json::to_string(&tally).unwrap_or_default();
        self.db().set_meta(TALLY, &json)?;
        Ok(tally)
    }

    /// What Rescan is waiting to hear about.
    pub fn rescan_results(&self) -> ProjectResult<RescanResults> {
        let rows = self.db().rescan_faces()?;
        let ids: Vec<i64> = rows.iter().map(|r| r.track_id).collect();
        let items: HashMap<i64, as_db::TrackRow> =
            self.db().tracks_by_id(&ids)?.into_iter().map(|t| (t.id, t)).collect();
        let mut faces = Vec::with_capacity(rows.len());
        for r in rows {
            let Some(t) = items.get(&r.track_id) else { continue };
            faces.push(RescanFace {
                face: self.track_item(t.clone()),
                verdict: r.verdict,
                target: r.target,
                other: r.other,
                score: r.score,
                score2: r.score2,
                read: r.read_quality.is_some(),
            });
        }
        let clearest = |c: CharacterId| -> ProjectResult<Vec<as_api::FaceTrackItem>> {
            let mut rows = self.db().tracks(TrackFilter::Character(c))?;
            rows.sort_by(|a, b| b.quality.total_cmp(&a.quality).then(a.id.cmp(&b.id)));
            Ok(rows.into_iter().take(PAIR_FACES).map(|t| self.track_item(t)).collect())
        };
        let mut pairs = Vec::new();
        for (a, b, main, second, together, likeness) in self.db().rescan_pairs()? {
            pairs.push(RescanPair {
                a,
                b,
                likeness,
                main,
                second,
                together,
                a_faces: clearest(a)?,
                b_faces: clearest(b)?,
            });
        }
        let tally = self.db().meta(TALLY)?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        Ok(RescanResults { faces, pairs, tally })
    }

    /// Apply the person's answer. Returns the cast edits it made, for the
    /// caller to follow up as any decision.
    pub fn rescan_answer(&self, answer: &RescanAnswer) -> ProjectResult<Vec<CastEdit>> {
        let ids = |t: &[u32]| t.iter().map(|&x| i64::from(x)).collect::<Vec<i64>>();
        let edit = |e: CastEdit| -> ProjectResult<Vec<CastEdit>> {
            self.cast_edit(&e)?;
            Ok(vec![e])
        };
        match answer {
            RescanAnswer::Accept { tracks, character_id } => {
                // Its sharper reading first: the faces placed after this
                // decision go by it.
                self.db().adopt_reads(&ids(tracks))?;
                edit(CastEdit::Assign { tracks: tracks.clone(), character_id: *character_id })
            }
            RescanAnswer::NotThem { tracks, character_id } => {
                self.db().dismiss_rescan_faces(&ids(tracks))?;
                edit(CastEdit::NotThem { tracks: tracks.clone(), character_id: *character_id })
            }
            RescanAnswer::NotAFace { tracks } => {
                self.db().dismiss_rescan_faces(&ids(tracks))?;
                edit(CastEdit::NotAFace { tracks: tracks.clone() })
            }
            RescanAnswer::Dismiss { tracks } => {
                self.db().dismiss_rescan_faces(&ids(tracks))?;
                Ok(Vec::new())
            }
            RescanAnswer::SamePerson { from, into } => edit(CastEdit::Merge { from: *from, into: *into }),
            RescanAnswer::TwoPeople { a, b } => {
                // Pinned faces of each keep the grouping from joining them.
                let p = self.identity_params()?;
                for c in [*a, *b] {
                    self.db().pin_character_anchors(c, &anchors(&self.db().own_faces(c)?, NAME_ANCHORS, &p))?;
                }
                self.db().set_character_apart(*a, *b)?;
                Ok(Vec::new())
            }
            RescanAnswer::LeaveAll => {
                self.db().dismiss_all_rescan()?;
                Ok(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use as_api::TrackFilter;

    use super::*;
    use crate::analysis::tests::project_with_media;
    use crate::cast::tests::face;

    /// A project of two people of six shots each, and one more face.
    fn cast_of_two() -> (tempfile::TempDir, Arc<Project>) {
        let frames = 4000;
        let (dir, project, id) = project_with_media(frames);
        let mut found = Vec::new();
        for n in 0..6u32 {
            found.push(face(1, n * 200, 0.1, 0.9));
            found.push(face(2, n * 200 + 100, 0.5, 0.9));
        }
        // Blurry, so it stays out of what makes a character.
        found.push(face(1, 3000, 0.3, 0.2));
        project.store_face_chunk(id, 0, frames, &tracks::encode(&found), &[], &[]).unwrap();
        project.finish_faces(id).unwrap();
        project.recluster().unwrap();
        (dir, project)
    }

    /// Everything the cast is made of, as it stands.
    fn snapshot(p: &Project) -> (i64, Vec<(i64, Option<CharacterId>, TrackAssign, u32, Vec<u8>)>, usize) {
        let mut rows: Vec<_> = p
            .db()
            .identity_tracks()
            .unwrap()
            .into_iter()
            .map(|r| (r.id, r.character_id, r.assign, r.quality.to_bits(), r.emb))
            .collect();
        rows.sort_by_key(|r| r.0);
        (p.db().cast_rev().unwrap(), rows, p.db().characters().unwrap().len())
    }

    fn all(cast: &RescanCast) -> Vec<usize> {
        (0..cast.tracks.len()).collect()
    }

    fn sharp(cast: &RescanCast, track: usize, like: usize) -> DoneRead {
        let donor = cast.members[like].1[0];
        DoneRead {
            track,
            embedding: tracks::embedding_to_bytes(&cast.tracks[donor].embedding),
            quality: 0.8,
            tried: 3,
            found: 3,
        }
    }

    /// The blurry face, and the character it should go to.
    fn blurry(cast: &RescanCast) -> usize {
        (0..cast.tracks.len()).min_by(|&a, &b| cast.tracks[a].quality.total_cmp(&cast.tracks[b].quality)).unwrap()
    }

    #[test]
    fn a_rescan_changes_nothing_of_the_cast() {
        let (_dir, p) = cast_of_two();
        let before = snapshot(&p);
        let cast = p.rescan_cast().unwrap();
        let read = sharp(&cast, blurry(&cast), 1);
        p.rescan_keep(&cast, &all(&cast), &[read], 2).unwrap();
        assert_eq!(snapshot(&p), before);
    }

    #[test]
    fn a_suggestion_waits_and_an_answer_applies_it_with_its_reading() {
        let (_dir, p) = cast_of_two();
        let cast = p.rescan_cast().unwrap();
        let i = blurry(&cast);
        // The stored face is like the first person, the reading the second.
        let read = sharp(&cast, i, 1);
        let target = cast.characters[cast.members[1].0 as usize];
        let tally = p.rescan_keep(&cast, &[i], std::slice::from_ref(&read), 0).unwrap();
        assert_eq!((tally.looked, tally.read), (1, 1));
        let found = p.rescan_results().unwrap();
        assert_eq!(found.faces.len(), 1);
        let f = &found.faces[0];
        assert!(f.read && f.target == Some(target), "{f:?}");
        let id = f.face.id;
        p.rescan_answer(&RescanAnswer::Accept { tracks: vec![id], character_id: target }).unwrap();
        let after = p.db().identity_tracks().unwrap();
        let r = after.iter().find(|r| r.id == i64::from(id)).unwrap();
        assert_eq!((r.character_id, r.assign), (Some(target), TrackAssign::Pinned));
        assert_eq!(r.emb, read.embedding);
        assert!(r.quality >= 0.8);
        assert!(p.rescan_results().unwrap().faces.is_empty(), "answered");
    }

    #[test]
    fn leaving_a_face_as_it_is_forbids_nothing() {
        let (_dir, p) = cast_of_two();
        let cast = p.rescan_cast().unwrap();
        let i = blurry(&cast);
        p.rescan_keep(&cast, &[i], &[sharp(&cast, i, 1)], 0).unwrap();
        let id = p.rescan_results().unwrap().faces[0].face.id;
        let before = snapshot(&p);
        p.rescan_answer(&RescanAnswer::Dismiss { tracks: vec![id] }).unwrap();
        assert!(p.rescan_results().unwrap().faces.is_empty());
        assert_eq!(snapshot(&p), before);
        assert!(p.db().identity_tracks().unwrap().iter().all(|r| r.forbids.is_empty()));
    }

    #[test]
    fn a_face_the_person_moved_meanwhile_is_no_longer_asked_about() {
        let (_dir, p) = cast_of_two();
        let cast = p.rescan_cast().unwrap();
        let i = blurry(&cast);
        p.rescan_keep(&cast, &[i], &[sharp(&cast, i, 1)], 0).unwrap();
        let f = p.rescan_results().unwrap().faces.remove(0);
        let elsewhere = cast.characters[cast.members[0].0 as usize];
        p.cast_edit(&CastEdit::Assign { tracks: vec![f.face.id], character_id: elsewhere }).unwrap();
        assert!(p.rescan_results().unwrap().faces.is_empty());
    }

    #[test]
    fn a_merge_carries_what_was_said_along() {
        let (_dir, p) = cast_of_two();
        let cast = p.rescan_cast().unwrap();
        let i = blurry(&cast);
        p.rescan_keep(&cast, &[i], &[sharp(&cast, i, 1)], 0).unwrap();
        let f = p.rescan_results().unwrap().faces.remove(0);
        let (from, into) = (f.target.unwrap(), cast.characters[cast.members[0].0 as usize]);
        p.rescan_answer(&RescanAnswer::SamePerson { from, into }).unwrap();
        for f in p.rescan_results().unwrap().faces {
            assert_ne!(f.target, Some(from));
        }
    }

    #[test]
    fn two_people_stay_two_and_are_not_asked_about_again() {
        let (_dir, p) = cast_of_two();
        let cast = p.rescan_cast().unwrap();
        let (a, b) = (cast.characters[0], cast.characters[1]);
        p.db().put_rescan_pair(a, b, 0.6, None, 0, as_api::RescanLikeness::Maybe).unwrap();
        assert_eq!(p.rescan_results().unwrap().pairs.len(), 1);
        p.rescan_answer(&RescanAnswer::TwoPeople { a, b }).unwrap();
        assert!(p.rescan_results().unwrap().pairs.is_empty());
        assert!(p.db().characters_apart(b, a).unwrap());
        let pinned = |c| p.db().tracks(TrackFilter::Character(c)).unwrap();
        assert!(pinned(a).iter().any(|t| t.assign == TrackAssign::Pinned));
        assert!(pinned(b).iter().any(|t| t.assign == TrackAssign::Pinned));
    }

    #[test]
    fn a_confirmed_face_and_one_named_by_its_body_are_never_asked_about() {
        let (_dir, p) = cast_of_two();
        let mut cast = p.rescan_cast().unwrap();
        let everyone: Vec<u32> = cast.rows.iter().map(|r| r.id as u32).collect();
        cast.rows[0].assign = TrackAssign::Pinned;
        cast.rows[1].via_body = true;
        let asked = cast.subjects(&RescanRequest { tracks: everyone, characters: Vec::new(), everyone_else: false });
        assert!(asked.iter().all(|&i| cast.row_of[i] > 1));
    }

    #[test]
    fn only_unclear_faces_are_read_again() {
        let (_dir, p) = cast_of_two();
        let cast = p.rescan_cast().unwrap();
        assert_eq!(cast.unclear(&all(&cast)), [blurry(&cast)]);
    }
}
