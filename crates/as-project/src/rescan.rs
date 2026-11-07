//! Rescan: a second look at faces. What it finds are suggestions, kept apart
//! from the cast; only the person's answers change it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use as_analysis::rescan::RescanFace as FaceToRead;
use as_analysis::tracks;
use as_api::{
    CastEdit, ContentKind, RescanAnswer, RescanFace, RescanLikeness, RescanPair, RescanRequest, RescanResults,
    RescanTally, RescanVerdict, SegmentKind, TrackAssign, TrackFilter,
};
use as_core::{CharacterId, FrameIndex, FrameRange, MediaId};
use as_db::{IdentityTrack, RescanFaceRow};
use as_identity::{CharKey, Judged, Params, Question, Read, Space, TrackIn, Verdict, cast_space, in_cast_space, judge};
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
/// Frames of one mirror or screen moment: a few seconds of samples.
const MIRROR_FRAMES: u32 = 40;

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
    /// The space faces are compared in, and the bars of it.
    pub space: Space,
    pub bars: Params,
}

/// Clearest faces of each character the "same person?" check reads.
pub const PAIR_READS: usize = 8;
/// How alike a second model must find two characters to back the first: at
/// its own bar for one person (`Params::sface`'s link, `Params::default`'s).
const SFACE_AGREES: f32 = 0.40;
const ARCFACE_AGREES: f32 = 0.30;

/// Two characters to compare, and the clearest faces of each.
#[derive(Clone, Debug, PartialEq)]
pub struct PairToCheck {
    pub a: CharKey,
    pub b: CharKey,
    pub a_faces: Vec<usize>,
    pub b_faces: Vec<usize>,
    pub together: u32,
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

/// What two characters' faces say: from the link bar they may be one person,
/// halfway to where groups count as copies they likely are, unless a second
/// model disagrees. Measured on five casts (clearest faces, stored): one
/// person split by looks 0.33-0.79, people seen together up to 0.27 (0.57 at
/// worst) in live action.
fn likeness_of(alike: f32, second_agrees: Option<bool>, p: &Params) -> Option<RescanLikeness> {
    if alike < p.link {
        return None;
    }
    let sure = alike >= (p.link + p.copy_link) / 2.0;
    Some(if sure && second_agrees != Some(false) { RescanLikeness::Likely } else { RescanLikeness::Maybe })
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
        judge(&q, &self.params, Some(&self.space))
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

    /// The clearest faces of a character.
    fn clearest(&self, k: CharKey) -> Vec<usize> {
        let mut g = self.members.iter().find(|m| m.0 == k).map_or(Vec::new(), |m| m.1.clone());
        g.sort_by(|&a, &b| self.tracks[b].quality.total_cmp(&self.tracks[a].quality).then(a.cmp(&b)));
        g.truncate(PAIR_READS);
        g
    }

    /// The pair `a`, `b` if it is worth asking about: faces of one person
    /// are not found in the same frame, bar a mirror or a screen now and then.
    fn pair(&self, a: CharKey, b: CharKey) -> Option<PairToCheck> {
        let group = |k: CharKey| self.members.iter().find(|m| m.0 == k).map(|m| m.1.as_slice()).unwrap_or_default();
        let (ga, gb) = (group(a), group(b));
        let small = ga.len().min(gb.len());
        let together = as_identity::frames_together(&self.tracks, ga, gb);
        // Frames, not faces: a face is seen in many frames of its track.
        let allowed = ((self.params.copy_share * small as f32) as u32).max(1) * MIRROR_FRAMES;
        if small == 0 || together > allowed {
            return None;
        }
        Some(PairToCheck { a, b, a_faces: self.clearest(a), b_faces: self.clearest(b), together })
    }

    /// How alike the clearest faces of each side are, by their `reads`
    /// (`read`: track to embedding, as the cast keeps them) and their stored
    /// faces where there is no reading.
    fn alike(&self, p: &PairToCheck, read: &HashMap<usize, Vec<f32>>, stored: bool) -> Option<f32> {
        let faces = |g: &[usize]| -> Vec<Vec<f32>> {
            g.iter()
                .filter_map(|i| read.get(i).cloned().or_else(|| stored.then(|| self.tracks[*i].embedding.clone())))
                .collect()
        };
        let (a, b) = (faces(&p.a_faces), faces(&p.b_faces));
        if a.is_empty() || b.is_empty() {
            return None;
        }
        if !stored {
            return Some(as_identity::likeness(&a, &b));
        }
        Some(as_identity::likeness(&in_cast_space(&a, &self.space), &in_cast_space(&b, &self.space)))
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

    /// The model that can give a second opinion on this project's faces: only
    /// where the footage is what face models learned from. (On 3D animation
    /// none tells characters apart: strangers measure 0.92-0.95 to CCIP.)
    pub fn second_model(&self) -> Option<&'static str> {
        if self.content().ok()? != ContentKind::Live {
            return None;
        }
        Some(if self.face_embedder() == as_models::ids::SFACE {
            as_models::ids::ARCFACE
        } else {
            as_models::ids::SFACE
        })
    }

    /// How alike [`Self::second_model`] must find two faces to call them one.
    fn second_bar(&self) -> f32 {
        if self.face_embedder() == as_models::ids::SFACE { ARCFACE_AGREES } else { SFACE_AGREES }
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
        let (space, bars) = cast_space(&cast.tracks, &params, self.grouped_space()?.as_ref());
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
            bars,
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

    /// The pairs worth asking "same person?" about: characters a face could
    /// be either of, and characters the person chose that look alike.
    pub fn rescan_pairs_to_check(&self, cast: &RescanCast, request: &RescanRequest) -> ProjectResult<Vec<PairToCheck>> {
        let key = |c: CharacterId| cast.characters.iter().position(|x| *x == c).map(|k| k as CharKey);
        let mut wanted: HashSet<(CharKey, CharKey)> = HashSet::new();
        let mut add = |a: CharKey, b: CharKey| {
            wanted.insert((a.min(b), a.max(b)));
        };
        for r in self.db().rescan_faces()? {
            if let (RescanVerdict::Between, Some(a), Some(b)) =
                (r.verdict, r.target.and_then(key), r.other.and_then(key))
            {
                add(a, b);
            }
        }
        let chosen: Vec<CharKey> = request.characters.iter().filter_map(|c| key(*c)).collect();
        for (n, &a) in chosen.iter().enumerate() {
            for &b in &chosen[n + 1..] {
                let Some(p) = cast.pair(a, b) else { continue };
                if cast.alike(&p, &HashMap::new(), true).is_some_and(|s| s >= cast.bars.link) {
                    add(a, b);
                }
            }
        }
        let mut pairs: Vec<(CharKey, CharKey)> = wanted.into_iter().collect();
        pairs.sort_unstable();
        let mut out = Vec::new();
        for (a, b) in pairs {
            let apart = self.db().characters_apart(cast.characters[a as usize], cast.characters[b as usize])?;
            if !apart && let Some(p) = cast.pair(a, b) {
                out.push(p);
            }
        }
        Ok(out)
    }

    /// Keep what the second check says of `pairs`, from what was read (`main`
    /// the cast's own embedder, `second` the other model; `None`: there is
    /// none here). Only what may be one person reaches the person.
    pub fn rescan_keep_pairs(
        &self,
        cast: &RescanCast,
        pairs: &[PairToCheck],
        main: &[DoneRead],
        second: Option<&[DoneRead]>,
    ) -> ProjectResult<u32> {
        let vectors = |reads: &[DoneRead]| -> HashMap<usize, Vec<f32>> {
            reads.iter().filter_map(|r| Some((r.track, tracks::embedding_from_bytes(&r.embedding)?))).collect()
        };
        let (main, second) = (vectors(main), second.map(vectors));
        let agrees = self.second_bar();
        let mut kept = 0;
        for p in pairs {
            let (a, b) = (cast.characters[p.a as usize], cast.characters[p.b as usize]);
            // What was asked before no longer stands, whatever comes now.
            self.db().drop_rescan_pair(a, b)?;
            let Some(alike) = cast.alike(p, &main, true) else { continue };
            // Too few faces read for it to have an opinion: it has none.
            let other = second.as_ref().and_then(|s| cast.alike(p, s, false));
            if let Some(likeness) = likeness_of(alike, other.map(|s| s >= agrees), &cast.bars) {
                self.db().put_rescan_pair(a, b, alike, other, p.together, likeness)?;
                kept += 1;
            }
        }
        Ok(kept)
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
                second_agrees: second.map(|s| s >= self.second_bar()),
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
                let made = edit(CastEdit::Assign { tracks: tracks.clone(), character_id: *character_id })?;
                // Faces are placed after this returns: they go by its sharper reading.
                self.db().adopt_reads(&ids(tracks))?;
                Ok(made)
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

    type Track = (i64, Option<CharacterId>, TrackAssign, u32, Vec<u8>);

    /// Everything the cast is made of, as it stands.
    fn snapshot(p: &Project) -> (i64, Vec<Track>, usize) {
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
    fn a_second_model_that_disagrees_leaves_it_a_maybe() {
        let p = Params::default();
        assert_eq!(likeness_of(0.2, None, &p), None);
        assert_eq!(likeness_of(0.35, None, &p), Some(RescanLikeness::Maybe));
        assert_eq!(likeness_of(0.7, None, &p), Some(RescanLikeness::Likely));
        assert_eq!(likeness_of(0.7, Some(true), &p), Some(RescanLikeness::Likely));
        assert_eq!(likeness_of(0.7, Some(false), &p), Some(RescanLikeness::Maybe));
    }

    #[test]
    fn two_groups_of_one_person_are_asked_about_and_two_people_are_not() {
        let (_dir, p) = cast_of_two();
        let mut cast = p.rescan_cast().unwrap();
        let (a, b) = (cast.characters[0], cast.characters[1]);
        let request = RescanRequest { tracks: Vec::new(), characters: vec![a, b], everyone_else: false };
        assert!(p.rescan_pairs_to_check(&cast, &request).unwrap().is_empty(), "they look nothing alike");
        // Now the second group looks like the first.
        let like = cast.tracks[cast.members[0].1[0]].embedding.clone();
        for i in cast.members[1].1.clone() {
            cast.tracks[i].embedding = like.clone();
        }
        let pairs = p.rescan_pairs_to_check(&cast, &request).unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(p.rescan_keep_pairs(&cast, &pairs, &[], None).unwrap(), 1);
        assert_eq!(p.rescan_results().unwrap().pairs[0].likeness, RescanLikeness::Likely);
        p.rescan_answer(&RescanAnswer::TwoPeople { a, b }).unwrap();
        assert!(p.rescan_pairs_to_check(&cast, &request).unwrap().is_empty(), "answered");
    }

    #[test]
    fn only_unclear_faces_are_read_again() {
        let (_dir, p) = cast_of_two();
        let cast = p.rescan_cast().unwrap();
        assert_eq!(cast.unclear(&all(&cast)), [blurry(&cast)]);
    }
}
