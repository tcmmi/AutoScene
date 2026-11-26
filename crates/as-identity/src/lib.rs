//! Who is who: face tracks → characters.
//!
//! 1. **Core tracks** — clear enough to define a character (quality, length)
//!    — are grouped by constrained average-linkage clustering. Two faces
//!    found in the same frame are two people (cannot-link) — unless their
//!    groups are clearly one person: then the face in the way was a mirror
//!    image, a photo, a screen or a stranger, and steps aside (see `ahc`).
//!    Tracks the user pinned to one character start in groups of like
//!    faces and are one character in the end (`pins_together`), pinned to
//!    different characters never meet.
//! 2. Groups with enough screen time (or a pin) become characters — except
//!    a group that is only ever on screen with a bigger one it clearly is
//!    (a reflection, a face on a screen). Each gets a handful of prototypes
//!    covering its looks (glasses, lighting, age).
//! 3. Every other track joins the character it clearly resembles; a close
//!    call between two goes to *unsure* with a suggestion, the rest stays
//!    unassigned (extras, background faces).
//! 4. Characters keep their identity across reclusters: a group inherits the
//!    character most of its tracks belonged to before, so names and edits
//!    stay put when episodes are added.
//!
//! After a decision the cast needn't be grouped again: [`reassign`] keeps
//! every character's members and runs step 3 for the other tracks.
//!
//! Animation: to ArcFace every character of one style looks alike, so the
//! face space itself is fitted to the footage first (see `adapt`).
//!
//! Pure computation; the project layer loads tracks and stores the result.

mod adapt;
mod ahc;
pub mod bodies;
mod judge;
pub mod metrics;
mod proto;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

pub use adapt::{Fit, FitStats, MIN_STRANGERS, Space, fit};
pub use ahc::cluster;
pub use judge::{
    Judged, Question, Read, Tuning, Verdict, cast_space, frames_together, in_cast_space, judge, judge_tuned, likeness,
};

/// A character as the caller knows it (index into its own list).
pub type CharKey = u32;

/// How faces are grouped into characters, as a number: raised when
/// [`identify`] groups differently enough that a cast grouped before should
/// be grouped again (the user's decisions stay). 2: one person seen twice
/// at once (a mirror, a screen, a face found twice) is no longer two. 3:
/// the similarity bars fit how alike strangers look (animation). 4: in
/// animation the face space itself is fitted to the footage.
pub const VERSION: u32 = 4;

/// [`VERSION`] for a cast grouped from an anime embedder's faces (CCIP's,
/// with [`Params::anime`]). `identify` groups them the same way — its
/// outcome and the stored [`Space`] are dimension-agnostic — so a cast of
/// either kind regroups on the same bumps. Kept apart so an anime-only
/// change can bump alone: +1, CCIP b36 and its bars (`link` 0.45,
/// `core_quality` 0.20).
pub const ANIME_VERSION: u32 = VERSION + 1;

/// [`VERSION`] for a cast of footage the user called (3D) animation, grouped
/// in the space fitted to it. Kept apart so a change only that space sees
/// can bump alone, and live action is not grouped again for it: +1, two
/// tight groups with a gap between them stay two (a hero and the everyday
/// self; [`Params::tight`]).
pub const ANIMATION_VERSION: u32 = VERSION + 1;

#[derive(Clone, Debug)]
pub struct TrackIn {
    /// Media the track belongs to: tracks of one media on screen together can't be one person.
    pub media: u32,
    pub start: u32,
    pub end: u32,
    /// Frames the face is actually on screen (sorted, within `start..end`);
    /// empty means all of `start..end`. A gap is a face that was gone for a
    /// while (see `as_analysis::tracks::presence_spans`).
    pub spans: Vec<(u32, u32)>,
    /// Frames the face was found in (sorted; the face pass looks at the same
    /// frames for every face). Two faces found in the same frame are two
    /// faces at once; a track that only fills another's gap is not. Empty:
    /// unknown, `spans` stand in.
    pub seen: Vec<u32>,
    /// Unit-length identity embedding, of the pass's embedder (ArcFace's
    /// 512, CCIP's 768): one run compares only the most common length.
    pub embedding: Vec<f32>,
    pub quality: f32,
    pub dets: u32,
    /// How much the track counts in the statistics of its footage — its
    /// share of the screen; a pair of strangers weighs as its lighter
    /// face. 1.0 counts like any other.
    pub weight: f32,
    /// The user put this track on this character.
    pub pin: Option<CharKey>,
    /// The user said this track is not these characters.
    pub forbid: Vec<CharKey>,
    /// Character before this run (auto or pinned), for stable identities.
    pub previous: Option<CharKey>,
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Average similarity at which two groups merge.
    pub link: f32,
    /// Tracks at or above this quality with at least `core_dets` detections define characters.
    pub core_quality: f32,
    pub core_dets: u32,
    /// A group becomes a character with this many tracks and frames on screen.
    pub min_tracks: usize,
    pub min_frames: u32,
    /// ... and when its tracks agree this much on average: groups of blurry,
    /// dark or masked faces that only resemble "a face" fall below.
    pub min_cohesion: f32,
    /// Other tracks join a character at this similarity, if the runner-up is `margin` behind.
    pub assign: f32,
    pub margin: f32,
    /// Below `assign` but at least this: unsure, with a suggestion.
    pub unsure: f32,
    /// Prototypes per character.
    pub prototypes: usize,
    /// A group inherits a previous character when that many of its tracks
    /// (quality-weighted share) were that character.
    pub inherit: f32,
    /// Whole groups whose mean faces are this alike are one person even if
    /// faces of theirs were seen at the same time (a mirror, a photo, a
    /// screen, a stranger's face in one of them) ...
    pub copy_link: f32,
    /// ... when at most this share of the smaller group's tracks (at least
    /// one) has to step aside so no two of the merged group's faces are on
    /// screen at once. People often on screen together (twins) stay two.
    pub copy_share: f32,
    /// A group that is on screen with a bigger one at least this share of
    /// the time it is seen, and most of whose faces are at least
    /// `copy_alike` like the face of the bigger one beside them — one face
    /// at one moment, seen twice — is its copy (a reflection, a face on a
    /// screen) and no character of its own.
    pub copy_together: f32,
    pub copy_alike: f32,
    /// With at least this many pairs of clear faces seen at once, the
    /// similarity bars are fitted to how alike strangers look here (see
    /// [`Params::fitted`]).
    pub calibrate_from: usize,
    /// The user said the footage is (3D) animation: the bars (and the face
    /// space) are fitted to its strangers however little alike they look,
    /// not only from 0.15 on.
    pub animated: bool,
    /// Fit the face space to footage that looks like animation (see
    /// [`fit`]); off, only the bars are fitted ([`Params::fitted`]).
    pub adapt: bool,
    /// Faces whose strangers (see [`strangers`]) are at most this alike are
    /// left as they are, from this on as alike as a style makes them they
    /// are fully fitted, in between in part — the embedder's actors and
    /// its animation.
    pub strangers_alone: f32,
    pub strangers_style: f32,
    /// A new prototype is made of a member only when it is less than this
    /// alike to the prototypes found so far: a character's table of looks
    /// needn't hold the same look twice (see `proto`).
    pub table_dedup_cos: f32,
    /// At most this many faces of two groups may step aside for one merge
    /// (see `ahc`): a merge that needs more is not made.
    pub merge_top: usize,
    /// At most this share of the mixed similarity may come from the fitted
    /// space (see [`fit`]); an embedder that knows the footage itself (an
    /// anime embedder on anime) trusts its own geometry more. 1.0: as
    /// fitted.
    pub cl_cap: f32,
    /// Two groups of at least `tight_tracks` tracks each stay two when they
    /// are less alike to each other than this share of how alike the less
    /// coherent of them is in itself: two tight groups with a gap between
    /// them. A hero and the everyday self are never on screen together, so
    /// no face seen beside another holds them apart, and in a space fitted
    /// to animation the bar lies where strangers still look alike (see
    /// `ahc`). 0: off, as it is outside that space.
    pub tight: f32,
    /// What `tight` is in the space fitted to footage the user called
    /// animation ([`Fit::params`]). The room for it is narrow, and one
    /// title set it (`bench/3d`): over two episodes of a 3D series in 42
    /// amounts the grouping finds the hero and her everyday self 0.54 to
    /// 0.68 of their own cohesion alike, and the least alike halves of one
    /// figure it joins (15 tracks a side or more) 0.73. Footage with too
    /// few faces seen together for a fitted space (one sparse episode, a
    /// short film) does not get the rule at all.
    pub tight_animated: f32,
    /// Fewer tracks say too little about how alike a group's own faces
    /// are. From 30 the rule misses the pair in a third of 42 amounts of
    /// footage (a piece of one self joins the other first); from 15 in none.
    pub tight_tracks: usize,
}

impl Default for Params {
    fn default() -> Self {
        // For ArcFace glintr100, set on bench/identity: precision 0.988,
        // recall 0.899 for the main cast (per track). Mean faces of one
        // person's groups split by a mirror or a stray face: 0.71 – 0.83 (one
        // face of 11 – 15 seen together); of two people's: 0.31 at most.
        // A face and its reflection: 0.53 – 0.64; a face and the one beside
        // it of people on screen together most of the time (a class, a
        // party): 0.24 at most. Identical twins who are (nearly) never seen
        // apart could still look like a copy — that one rule leaves to the
        // user.
        Self {
            link: 0.30,
            core_quality: 0.45,
            core_dets: 2,
            min_tracks: 3,
            min_frames: 96,
            min_cohesion: 0.5,
            assign: 0.45,
            margin: 0.08,
            unsure: 0.30,
            prototypes: 12,
            inherit: 0.3,
            copy_link: 0.6,
            copy_share: 0.15,
            copy_together: 0.8,
            copy_alike: 0.5,
            calibrate_from: 50,
            animated: false,
            adapt: true,
            strangers_alone: 0.05,
            strangers_style: 0.15,
            table_dedup_cos: 0.98,
            merge_top: usize::MAX,
            cl_cap: 1.0,
            tight: 0.0,
            tight_animated: 0.70,
            tight_tracks: 15,
        }
    }
}

impl Params {
    /// For SFace (Fast Scan's embedder), which finds one person's faces
    /// less alike and strangers more alike than ArcFace does (on the same
    /// 1072 faces of a live-action episode: one person's median 0.40,
    /// ArcFace 0.48; strangers' 0.08, ArcFace 0.01). Set on Fast Scan's
    /// tracks of that episode (BB S01E01 from its 960×540 proxy; main cast
    /// per screen time P 0.993, R 0.940): lower bars took two men for one
    /// in every run of the stress test, and the less sure faces Fast Scan
    /// keeps split Walter in some unless groups agree a little more than
    /// ArcFace's must. Its strangers look alike from about 0.15 (actors
    /// 0.12, a CGI film's characters 0.37).
    pub fn sface() -> Self {
        Self {
            link: 0.40,
            assign: 0.40,
            margin: 0.055,
            unsure: 0.30,
            min_cohesion: 0.55,
            copy_link: 0.52,
            copy_alike: 0.43,
            strangers_alone: 0.15,
            strangers_style: 0.30,
            // Not measured for SFace: on one 3D episode of Fast Scan's
            // tracks no share helped and 0.80 already cost.
            tight_animated: 0.0,
            ..Self::default()
        }
    }

    /// For an anime embedder (CCIP's 768-d faces) on anime footage: it
    /// learned drawn faces, yet the characters of one show still look much
    /// alike to it, so the bars sit above ArcFace's. Its strangers —
    /// up to `strangers_alone` alike it stands alone like actors, fully
    /// a style from `strangers_style` on — decide how far the bars and
    /// the space are fitted (`animated` stays off: this embedder knows
    /// drawn faces); what the footage's own space may add is capped
    /// (`cl_cap`), as is the number of faces one merge may send aside
    /// (`merge_top`).
    pub fn anime() -> Self {
        Self {
            link: 0.45,
            copy_link: 0.70,
            core_quality: 0.20,
            strangers_alone: 0.02,
            strangers_style: 0.25,
            table_dedup_cos: 0.88,
            merge_top: 4,
            cl_cap: 0.25,
            ..Self::default()
        }
    }

    /// [`Params::anime`] for faces CCIP-24 found, the anime embedder before
    /// b36, until the anime pass finds them again: the bars it was grouped
    /// with (its cosines run higher than b36's, so b36's bars would merge
    /// too much).
    pub fn anime_ccip24() -> Self {
        Self { link: 0.55, core_quality: 0.50, ..Self::anime() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Assign {
    None,
    /// Clustered or clearly resembling the character.
    Auto(usize),
    /// Resembles the character, but not clearly enough.
    Unsure(usize),
    /// The user's decision.
    Pinned(usize),
}

impl Assign {
    pub fn character(self) -> Option<usize> {
        match self {
            Self::Auto(c) | Self::Pinned(c) => Some(c),
            Self::None | Self::Unsure(_) => None,
        }
    }
}

/// A character in the result: an existing one (`key`) or a new one.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterOut {
    pub key: Option<CharKey>,
    /// Quality-weighted mean embedding of its tracks.
    pub centroid: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub characters: Vec<CharacterOut>,
    /// Per input track: assignment (index into `characters`) and the
    /// similarity behind it (1 for pins and clustered tracks).
    pub tracks: Vec<(Assign, f32)>,
    /// Characters from before (`TrackIn::previous`) that none of
    /// `characters` is any more, and the one that took more than half of
    /// their clear tracks as its members (or as copies of it): one person,
    /// grouped as one now. Only [`identify`] groups; the caller keeps the
    /// old one's scenepack for it.
    pub absorbed: Vec<(CharKey, usize)>,
    /// How faces were compared (similarities and centroids are in it). The
    /// caller keeps it for [`reassign`].
    pub space: Space,
}

impl TrackIn {
    /// Frames the face is on screen.
    fn presence(&self) -> Vec<(u32, u32)> {
        if self.spans.is_empty() { vec![(self.start, self.end)] } else { self.spans.clone() }
    }

    /// The frames the face was found in or, without them, every frame it is
    /// on screen.
    fn sightings(&self) -> Vec<u32> {
        if self.seen.is_empty() {
            self.presence().into_iter().flat_map(|(s, e)| s..e).collect()
        } else {
            self.seen.clone()
        }
    }
}

/// Whether two lists of frame ranges share a frame.
fn overlaps(a: &[(u32, u32)], b: &[(u32, u32)]) -> bool {
    a.iter().any(|&(s, e)| b.iter().any(|&(s2, e2)| s < e2 && s2 < e))
}

/// Whether two sorted lists of frames share one.
fn share_a_frame(a: &[u32], b: &[u32]) -> bool {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => return true,
        }
    }
    false
}

/// Two tracks of one media whose faces were found in the same frame: two
/// faces at once — two people, or one person and their copy (a mirror, a
/// photo, a screen). A track that only fills the gap of another (the
/// tracker lost the face for a moment) is not. Without the frames, two
/// faces on screen at the same time.
pub(crate) fn together(a: &TrackIn, b: &TrackIn) -> bool {
    if a.media != b.media || a.start >= b.end || b.start >= a.end {
        return false;
    }
    if a.seen.is_empty() || b.seen.is_empty() {
        return overlaps(&a.presence(), &b.presence());
    }
    share_a_frame(&a.seen, &b.seen)
}

/// What the user pinned to one character is that character, however its
/// faces grouped (they start in groups of like faces, see `ahc`): the groups
/// pinned to one character become one.
///
/// Before that, a pinned group lets go of what it swept up of another
/// character: one face of B that the user moved to A groups with B's faces,
/// and would bring them all to A. That is so when the group is not the
/// character's own (it holds no more than half of what the character held
/// besides the pins) and what it holds of other characters outweighs its
/// pins and the character's earlier faces in it. A group of pinned faces
/// that another character's faces join in smaller number keeps them: two
/// characters that turn out to be one person become one. For a character
/// that is nothing but its pinned faces (one the user just made of a few
/// faces), only another character's own faces weigh against the pins, most
/// of what that one held: a few faces of a second person inside another
/// character have no character to go back to, and follow the faces picked
/// of them.
///
/// And no face comes to a character through one of the groups pinned to it
/// beside a face of the character's own in another: when the pinned faces
/// were one group, a face seen in a frame with any of them could not join
/// it. So a face that is neither pinned nor was the character before, and
/// was seen in a frame with a face in another of the groups that is pinned
/// or was the character before, leaves. If it was another character, that
/// one's other faces in its group leave with it: one face seen beside the
/// character says who the others are not (a look-alike must not be taken in
/// by halves). What leaves one group is a group of its own. Two new faces
/// seen together stay, one in each group: the user may have merged two
/// people who are on screen together. (A group that follows the pinned one
/// later, in `keep_own_faces`, is not looked at here.)
fn pins_together(tracks: &[TrackIn], core: &[usize], groups: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    let pin_of = |g: &[usize]| g.iter().find_map(|&i| tracks[i].pin);
    if !groups.iter().any(|g| pin_of(g).is_some()) {
        return groups;
    }
    let held = held_besides_pins(tracks, core);
    let held_by = |k: CharKey| held.get(&k).copied().unwrap_or(0.0);
    let mut out: Vec<Vec<usize>> = Vec::with_capacity(groups.len());
    // Per pinned character, in the order they come: where its groups are in `out`.
    let mut keys: Vec<CharKey> = Vec::new();
    let mut at: HashMap<CharKey, Vec<usize>> = HashMap::new();
    let mut apart: Vec<Vec<usize>> = Vec::new();
    for g in groups {
        let Some(k) = pin_of(&g) else {
            out.push(g);
            continue;
        };
        let (mut pins, mut own) = (0f32, 0f32);
        let mut of_others: HashMap<CharKey, f32> = HashMap::new();
        for &i in &g {
            match (tracks[i].pin, tracks[i].previous) {
                (Some(_), _) => pins += tracks[i].quality,
                (None, Some(was)) if was == k => own += tracks[i].quality,
                (None, Some(was)) => *of_others.entry(was).or_default() += tracks[i].quality,
                (None, None) => {}
            }
        }
        let made_by_hand = held_by(k) <= 0.0;
        let others: f32 =
            of_others.iter().filter(|&(&c, &w)| !made_by_hand || w > 0.5 * held_by(c)).map(|(_, &w)| w).sum();
        let its_own = own > 0.5 * held_by(k);
        let g = if !its_own && others > own + pins {
            let (stay, go): (Vec<usize>, Vec<usize>) =
                g.into_iter().partition(|&i| tracks[i].pin.is_some() || tracks[i].previous == Some(k));
            apart.push(go);
            stay
        } else {
            g
        };
        let slots = at.entry(k).or_default();
        if slots.is_empty() {
            keys.push(k);
        }
        slots.push(out.len());
        out.push(g);
    }
    for k in keys {
        let slots = &at[&k];
        if slots.len() < 2 {
            continue;
        }
        let parts: Vec<&[usize]> = slots.iter().map(|&s| out[s].as_slice()).collect();
        let leaving = beside_their_own(tracks, k, &parts);
        for (&s, beside) in slots.iter().zip(leaving) {
            if beside.is_empty() {
                continue;
            }
            // With them, the other faces here of the characters they were.
            let beside: HashSet<usize> = beside.into_iter().collect();
            let were: HashSet<CharKey> = beside.iter().filter_map(|&i| tracks[i].previous).collect();
            let (gone, stay): (Vec<usize>, Vec<usize>) = std::mem::take(&mut out[s]).into_iter().partition(|i| {
                let t = &tracks[*i];
                beside.contains(i) || (t.pin.is_none() && t.previous.is_some_and(|c| c != k && were.contains(&c)))
            });
            out[s] = stay;
            apart.push(gone);
        }
        for &s in &slots[1..] {
            let g = std::mem::take(&mut out[s]);
            out[slots[0]].extend(g);
        }
    }
    out.retain(|g| !g.is_empty());
    out.extend(apart);
    out.iter_mut().for_each(|g| g.sort_unstable());
    // Largest first, as the clustering hands them over (the order among equals stays).
    out.sort_by_key(|g| std::cmp::Reverse(g.len()));
    out
}

/// Per group of `groups` (all pinned to `k`), its faces that are neither
/// pinned nor were `k` before and were seen in a frame with one of `k`'s own
/// faces (pinned, or `k` before) in another of the groups, unless the own
/// faces of the two groups are themselves seen together: then the character
/// holds two people the user merged who share the screen, and a face beside
/// the other one is no stranger. Seen together means in more than one frame:
/// on each side at least two own faces, and a tenth of the smaller group's,
/// share a frame with the other's (a face on a screen, a poster or a face
/// tracked wrong once must not count).
fn beside_their_own(tracks: &[TrackIn], k: CharKey, groups: &[&[usize]]) -> Vec<Vec<usize>> {
    let own = |i: usize| tracks[i].pin.is_some() || tracks[i].previous == Some(k);
    let beside = Beside::new(
        tracks,
        groups.iter().enumerate().flat_map(|(gi, g)| g.iter().filter(|&&i| own(i)).map(move |&i| (i, gi))),
    );
    // Per group and other group: its own faces seen in a frame with an own face of the other.
    let mut shared: HashMap<(usize, usize), usize> = HashMap::new();
    for (gi, g) in groups.iter().enumerate() {
        for &i in g.iter().filter(|&&i| own(i)) {
            let mut with: HashSet<usize> = HashSet::new();
            beside.any(&tracks[i], |_, gj| {
                if gj != gi {
                    with.insert(gj);
                }
                false
            });
            with.into_iter().for_each(|gj| *shared.entry((gi, gj)).or_default() += 1);
        }
    }
    let owns: Vec<usize> = groups.iter().map(|g| g.iter().filter(|&&i| own(i)).count()).collect();
    let mut two: HashSet<(usize, usize)> = HashSet::new();
    for (&(gi, gj), &n) in &shared {
        let back = shared.get(&(gj, gi)).copied().unwrap_or(0);
        let need = 2usize.max(owns[gi].min(owns[gj]).div_ceil(10));
        if n >= need && back >= need {
            two.insert((gi.min(gj), gi.max(gj)));
        }
    }
    groups
        .iter()
        .enumerate()
        .map(|(gi, g)| {
            g.iter()
                .copied()
                .filter(|&u| {
                    !own(u) && beside.any(&tracks[u], |_, gj| gj != gi && !two.contains(&(gi.min(gj), gi.max(gj))))
                })
                .collect()
        })
        .collect()
}

/// Faces by media and start, to find the ones a face was seen in a frame
/// with (`together`) without looking at all of them.
struct Beside<'a> {
    tracks: &'a [TrackIn],
    by_media: HashMap<u32, Starts>,
}

/// The faces of one media by their start (start, track, tag), and the
/// longest of them: no face that starts more than that before another
/// reaches it.
type Starts = (Vec<(u32, usize, usize)>, u32);

impl<'a> Beside<'a> {
    /// `faces`: track and tag.
    fn new(tracks: &'a [TrackIn], faces: impl IntoIterator<Item = (usize, usize)>) -> Self {
        let mut by_media: HashMap<u32, Starts> = HashMap::new();
        for (i, tag) in faces {
            let t = &tracks[i];
            let (list, longest) = by_media.entry(t.media).or_default();
            list.push((t.start, i, tag));
            *longest = (*longest).max(t.end.saturating_sub(t.start));
        }
        by_media.values_mut().for_each(|(list, _)| list.sort_unstable());
        Self { tracks, by_media }
    }

    /// Whether `f` says yes to one of the faces `t` was seen in a frame with
    /// (it is given the face's track and tag).
    fn any(&self, t: &TrackIn, mut f: impl FnMut(usize, usize) -> bool) -> bool {
        let Some((faces, reach)) = self.by_media.get(&t.media) else {
            return false;
        };
        let from = faces.partition_point(|&(start, _, _)| start.saturating_add(*reach) <= t.start);
        faces[from..]
            .iter()
            .take_while(|&&(start, _, _)| start < t.end)
            .any(|&(_, v, tag)| together(t, &self.tracks[v]) && f(v, tag))
    }
}

/// Per character, what it held besides the user's pins: the quality of the
/// clear tracks that were it and are not pinned.
fn held_besides_pins(tracks: &[TrackIn], core: &[usize]) -> HashMap<CharKey, f32> {
    let mut held: HashMap<CharKey, f32> = HashMap::new();
    for &i in core {
        if tracks[i].pin.is_none()
            && let Some(k) = tracks[i].previous
        {
            *held.entry(k).or_default() += tracks[i].quality;
        }
    }
    held
}

/// A character the user pinned faces to keeps the faces it had. A merge by
/// hand pins the merged character's faces to the other one (`as-db`
/// `merge_characters`), and that one's own faces are often too unlike them
/// to join their group (had they been alike, the grouping would have made
/// one character of the two). The pins hold the character, so its own faces
/// would go on as a new one: the merge undone, and a name left on the faces
/// that were merged in. A merge made now also pins a few of the other's own
/// faces, and those hold them. For a character without such pins (a merge
/// made before; a name whose pinned faces are in the smaller part of it)
/// the unpinned group that holds most of what the character held besides
/// the pins, and was that character more than any other, goes on as that
/// character: it joins the group pinned to it. (A few faces once placed
/// with a character are not its own faces: the group must hold as many of
/// them as make a character.)
///
/// The few faces a merge pins do not always start as a group of their own:
/// a character's most typical faces are also the ones most like its other
/// look, and where they are as like the merged-in faces as two groups must
/// be to join, they start among those. Then the character's own faces have
/// no pinned group to join, and are not most of what it held once the
/// merged-in person outgrew them. So a group that was this character more
/// than any other also follows when the pins say so (`held_by_pins`).
/// `company`: per track, the group it left the clustering in; `loose`: the
/// faces of no group.
fn keep_own_faces(
    tracks: &[TrackIn],
    core: &[usize],
    chars: &mut Vec<(Vec<usize>, Option<CharKey>)>,
    loose: &[usize],
    p: &Params,
    company: &[usize],
) {
    let pin_of = |g: &[usize]| g.iter().find_map(|&i| tracks[i].pin);
    let pinned: HashMap<CharKey, usize> =
        chars.iter().enumerate().filter_map(|(ci, (g, _))| Some((pin_of(g)?, ci))).collect();
    if pinned.is_empty() {
        return;
    }
    let held = held_besides_pins(tracks, core);
    // Per unpinned group: the pinned group of the character most of its earlier faces were (a tie:
    // the lower key), with their quality and number.
    let was: Vec<Option<(usize, CharKey, f32, usize)>> = chars
        .iter()
        .map(|(g, _)| {
            if pin_of(g).is_some() {
                return None;
            }
            let mut was: HashMap<CharKey, (f32, usize)> = HashMap::new();
            for &i in g {
                if let Some(k) = tracks[i].previous {
                    let of = was.entry(k).or_default();
                    *of = (of.0 + tracks[i].quality, of.1 + 1);
                }
            }
            let (&k, &(w, n)) = was.iter().max_by(|a, b| a.1.0.total_cmp(&b.1.0).then(b.0.cmp(a.0)))?;
            Some((*pinned.get(&k)?, k, w, n))
        })
        .collect();
    // Per pinned group, its pinned faces ready to vote; every clear face, tagged by the group it is in
    // (`usize::MAX`: none). Both made when first asked.
    let mut votes: HashMap<usize, Vec<(usize, f32)>> = HashMap::new();
    let mut around: Option<Beside<'_>> = None;
    // Per character pinned, its faces of no group that were it (too vague to make one): one more part of it.
    let mut vague: HashMap<CharKey, Vec<usize>> = HashMap::new();
    for &i in loose {
        if let Some(k) = tracks[i].previous.filter(|k| tracks[i].pin.is_none() && pinned.contains_key(k)) {
            vague.entry(k).or_default().push(i);
        }
    }
    let mut follows: Vec<(usize, usize)> = Vec::new();
    for (ci, (g, _)) in chars.iter().enumerate() {
        let Some((pi, k, w, n)) = was[ci] else {
            continue;
        };
        if n < p.min_tracks {
            continue;
        }
        let follow = w > 0.5 * held[&k] || {
            let pins = votes.entry(pi).or_insert_with(|| pins_and_their_company(tracks, &chars[pi].0, k, company));
            let around = around.get_or_insert_with(|| {
                let mut tag = vec![usize::MAX; tracks.len()];
                chars.iter().enumerate().for_each(|(cj, (g, _))| g.iter().for_each(|&i| tag[i] = cj));
                Beside::new(tracks, core.iter().map(|&i| (i, tag[i])))
            });
            // The character's other parts: groups that may follow it, and its vague faces.
            let mut parts: Vec<&[usize]> = was
                .iter()
                .enumerate()
                .filter(|&(cj, x)| cj != ci && x.is_some_and(|(pj, _, _, nj)| pj == pi && nj >= p.min_tracks))
                .map(|(cj, _)| chars[cj].0.as_slice())
                .collect();
            parts.extend(vague.get(&k).map(Vec::as_slice));
            held_by_pins(tracks, g, ci, pins, around, &parts, p)
        };
        if follow {
            follows.push((ci, pi));
        }
    }
    for &(ci, pi) in &follows {
        let g = std::mem::take(&mut chars[ci].0);
        chars[pi].0.extend(g);
    }
    chars.retain(|(g, _)| !g.is_empty());
}

/// Whether the faces pinned to a character hold the group `g` (number `me`
/// among the groups; faces that were the character and grouped apart from
/// the group pinned to it). At least two of them must vote for it: be as
/// like the faces of `g` on average as a face must be to be placed on a
/// character (`assign`, and at least `link`), more like them than like the
/// other faces of the group they left the clustering in, and seen in no
/// frame with a face of `g`. Such pins are of `g`'s look and started
/// in other company. The company is the clustering's group, not all that is
/// pinned to the character: on a character that also holds someone merged
/// in by hand, every pin would be in unlike company, and a part the grouping
/// rightly parted (the other self of a named character) would be held.
///
/// And the faces seen in a frame with a face of `g` that are as like the
/// pins that vote as two groups must be to join (`around`: every clear face,
/// tagged by its group) must be on average less like them than `g` is: if
/// they are as like them, the pins are theirs, and `g` is someone beside
/// them (someone like the character, once on it by mistake). Two people
/// merged into one character who share the screen are unlike each other,
/// and each part is held.
///
/// And the pins hold the part of the character they are most like: another
/// of its `parts` (groups that were it) more like the pins that vote, and
/// too unlike `g` to be one person with it, is their person, and `g`
/// someone else (someone like them, placed on the character where it was
/// not on screen, so never seen beside its faces). `pins`: see
/// `pins_and_their_company`.
fn held_by_pins(
    tracks: &[TrackIn],
    g: &[usize],
    me: usize,
    pins: &[(usize, f32)],
    around: &Beside<'_>,
    parts: &[&[usize]],
    p: &Params,
) -> bool {
    let weight = |i: usize| tracks[i].quality.max(0.05);
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let dim = tracks[g[0]].embedding.len();
    let mut of_g = vec![0f32; dim];
    let mut total = 0f32;
    for &i in g {
        of_g.iter_mut().zip(&tracks[i].embedding).for_each(|(s, x)| *s += weight(i) * x);
        total += weight(i);
    }
    let beside = Beside::new(tracks, g.iter().map(|&i| (i, 0)));
    let votes: Vec<usize> = pins
        .iter()
        .filter(|&&(i, to_own)| {
            let to_g = dot(&tracks[i].embedding, &of_g) / total.max(1e-9);
            to_g >= p.assign.max(p.link) && to_g > to_own && !beside.any(&tracks[i], |_, _| true)
        })
        .map(|&(i, _)| i)
        .collect();
    if votes.len() < 2 {
        return false;
    }
    // Who the pins that vote are: their weighted mean (a face's likeness to it is its average likeness to them).
    let mut who = vec![0f32; dim];
    let mut of_votes = 0f32;
    for &i in &votes {
        who.iter_mut().zip(&tracks[i].embedding).for_each(|(s, x)| *s += weight(i) * x);
        of_votes += weight(i);
    }
    who.iter_mut().for_each(|s| *s /= of_votes);
    let to_g = dot(&who, &of_g) / total.max(1e-9);
    let mut near: HashMap<usize, f32> = HashMap::new();
    for &i in g {
        around.any(&tracks[i], |j, cj| {
            let like = dot(&who, &tracks[j].embedding);
            if cj != me && like >= p.link {
                near.insert(j, like);
            }
            false
        });
    }
    let near_weight: f32 = near.keys().map(|&j| weight(j)).sum();
    let to_near = near.iter().map(|(&j, &like)| weight(j) * like).sum::<f32>() / near_weight.max(1e-9);
    if !near.is_empty() && to_near >= to_g {
        return false;
    }
    !parts.iter().any(|h| {
        let mut of_h = vec![0f32; dim];
        let mut weight_h = 0f32;
        for &i in h.iter() {
            of_h.iter_mut().zip(&tracks[i].embedding).for_each(|(s, x)| *s += weight(i) * x);
            weight_h += weight(i);
        }
        dot(&who, &of_h) / weight_h.max(1e-9) > to_g && dot(&of_g, &of_h) / (total * weight_h).max(1e-9) < p.link
    })
}

/// The faces pinned to `k` in `pinned` (the group pinned to it, with what
/// joined that), each with how like it is on average to the other faces of
/// the group it left the clustering in (`company`, per track; a face alone
/// there is like no one): what the face must beat to vote in
/// `held_by_pins`.
fn pins_and_their_company(tracks: &[TrackIn], pinned: &[usize], k: CharKey, company: &[usize]) -> Vec<(usize, f32)> {
    let weight = |i: usize| tracks[i].quality.max(0.05);
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let dim = pinned.first().map_or(0, |&i| tracks[i].embedding.len());
    // Per group of the clustering that the pinned group's faces came from: their weighted sum, their weight.
    let mut companies: HashMap<usize, (Vec<f32>, f32)> = HashMap::new();
    for &i in pinned {
        let (sum, total) = companies.entry(company[i]).or_insert_with(|| (vec![0f32; dim], 0.0));
        sum.iter_mut().zip(&tracks[i].embedding).for_each(|(s, x)| *s += weight(i) * x);
        *total += weight(i);
    }
    pinned
        .iter()
        .copied()
        .filter(|&i| tracks[i].pin == Some(k))
        .map(|i| {
            let e = &tracks[i].embedding;
            let (sum, total) = &companies[&company[i]];
            // (Unit-length faces: the face itself is `weight` of its group's sum.)
            let rest = total - weight(i);
            let to_own = if rest > 1e-6 { (dot(e, sum) - weight(i) * dot(e, e)) / rest } else { 0.0 };
            (i, to_own)
        })
        .collect()
}

/// Groups that are (nearly) only ever on screen with a bigger group, most of
/// whose faces are the face beside them seen twice — a reflection, a face on
/// a screen — go to `loose`: no character of their own, and their faces
/// can't join the person they copy (who is on screen in the same frames).
/// People who are always on screen together (a class, a crowd) are two
/// faces at every moment. A pinned group always stays. Returns, per track of
/// a group that went, the index (in `chars` as left) of the group it copied.
fn drop_copies(
    tracks: &[TrackIn],
    chars: &mut Vec<(Vec<usize>, Option<CharKey>)>,
    loose: &mut Vec<usize>,
    p: &Params,
) -> HashMap<usize, usize> {
    if chars.len() < 2 {
        return HashMap::new();
    }
    let frames: Vec<u32> =
        chars.iter().map(|(g, _)| g.iter().map(|&i| tracks[i].end - tracks[i].start).sum()).collect();
    // Per group, where its faces were seen: (media, frame) → its tracks there.
    let at: Vec<HashMap<(u32, u32), Vec<usize>>> = chars
        .iter()
        .map(|(g, _)| {
            let mut at: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
            for &i in g {
                for f in tracks[i].sightings() {
                    at.entry((tracks[i].media, f)).or_default().push(i);
                }
            }
            at
        })
        .collect();
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    // Most of `b`'s faces resemble the face of `a`'s beside them that much.
    let beside = |a: usize, b: usize| -> Option<f32> {
        let mut best: Vec<f32> = chars[b]
            .0
            .iter()
            .filter_map(|&i| {
                let t = &tracks[i];
                t.sightings()
                    .iter()
                    .filter_map(|&f| at[a].get(&(t.media, f)))
                    .flatten()
                    .map(|&j| dot(&t.embedding, &tracks[j].embedding))
                    .max_by(f32::total_cmp)
            })
            .collect();
        best.sort_by(f32::total_cmp);
        best.get(best.len() / 2).copied()
    };
    // The group each one copies, if any.
    let copy: Vec<Option<usize>> = (0..chars.len())
        .map(|b| {
            if chars[b].0.iter().any(|&i| tracks[i].pin.is_some()) {
                return None;
            }
            let total = at[b].len().max(1) as f32;
            (0..chars.len()).find(|&a| {
                a != b
                    && (frames[a], std::cmp::Reverse(a)) > (frames[b], std::cmp::Reverse(b))
                    && at[b].keys().filter(|f| at[a].contains_key(f)).count() as f32 / total >= p.copy_together
                    && beside(a, b).is_some_and(|s| s >= p.copy_alike)
            })
        })
        .collect();
    // Where each group that stays will be.
    let mut left = vec![None; chars.len()];
    let mut n = 0;
    for (b, c) in copy.iter().enumerate() {
        if c.is_none() {
            left[b] = Some(n);
            n += 1;
        }
    }
    let mut copied = HashMap::new();
    let mut kept = Vec::with_capacity(chars.len());
    for ((g, key), c) in std::mem::take(chars).into_iter().zip(&copy) {
        match c {
            Some(a) => {
                if let Some(at) = left[*a] {
                    copied.extend(g.iter().map(|&i| (i, at)));
                }
                loose.extend(g);
            }
            None => kept.push((g, key)),
        }
    }
    *chars = kept;
    copied
}

/// Quality-weighted mean similarity between the different tracks of a group:
/// `(|S|² − Σw²) / (W² − Σw²)` with `S = Σ wᵢeᵢ`, `W = Σ wᵢ` (unit `eᵢ`).
fn cohesion(tracks: &[TrackIn], g: &[usize]) -> f32 {
    if g.len() < 2 {
        return 1.0;
    }
    let dim = tracks[g[0]].embedding.len();
    let mut sum = vec![0f32; dim];
    let (mut w, mut w2) = (0f32, 0f32);
    for &i in g {
        let q = tracks[i].quality.max(0.05);
        for (s, x) in sum.iter_mut().zip(&tracks[i].embedding) {
            *s += q * x;
        }
        w += q;
        w2 += q * q;
    }
    let s2: f32 = sum.iter().map(|x| x * x).sum();
    (s2 - w2) / (w * w - w2).max(1e-9)
}

/// How alike two different people's faces look in these tracks: the
/// median similarity of clear faces found in the same frame (two faces at
/// once are two people but for the odd reflection). About 0.02 – 0.04 for
/// actors; ArcFace learned real faces, and to it the characters of one
/// animated film (3D or drawn) are first of all faces in one style — 0.22
/// on a Miraculous film, whose characters resemble each other more than
/// many actors resemble themselves. `None` with fewer than
/// `p.calibrate_from` such pairs. A pair counts for its less-seen face
/// ([`TrackIn::weight`]).
pub fn strangers(tracks: &[TrackIn], p: &Params) -> Option<f32> {
    let dim = session_dim(tracks)?;
    let clear = |t: &TrackIn| t.embedding.len() == dim && t.quality >= p.core_quality && t.dets >= p.core_dets;
    let mut at: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in tracks.iter().enumerate().filter(|(_, t)| clear(t)) {
        for &f in &t.seen {
            at.entry((t.media, f)).or_default().push(i);
        }
    }
    let mut pairs: HashSet<(usize, usize)> = HashSet::new();
    for v in at.values() {
        for (k, &a) in v.iter().enumerate() {
            pairs.extend(v[k + 1..].iter().map(|&b| (a.min(b), a.max(b))));
        }
    }
    if pairs.len() < p.calibrate_from.max(1) {
        return None;
    }
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let mut sims: Vec<(f64, f64)> = pairs
        .into_iter()
        .filter_map(|(a, b)| {
            let (s, w) = (dot(&tracks[a].embedding, &tracks[b].embedding), pair_weight(tracks, a, b));
            (s.is_finite() && w > 0.0).then_some((s as f64, w))
        })
        .collect();
    if sims.len() < p.calibrate_from.max(1) {
        return None;
    }
    sims.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(adapt::weighted_quantile(&sims, 0.5) as f32)
}

impl Params {
    /// These parameters for faces whose strangers look `alike` (see
    /// [`strangers`]): every similarity bar moves up as if `alike` were 0,
    /// `s ↦ (s − alike) / (1 − alike)`. Parameters are set for actors, whose
    /// strangers are near 0, so they change little there.
    pub fn for_strangers(&self, alike: f32) -> Params {
        let o = alike.clamp(0.0, 0.5);
        let up = |t: f32| t + o * (1.0 - t);
        Params {
            link: up(self.link),
            assign: up(self.assign),
            margin: self.margin * (1.0 - o),
            unsure: up(self.unsure),
            copy_link: up(self.copy_link),
            copy_alike: up(self.copy_alike),
            ..*self
        }
    }

    /// These parameters fitted to `tracks` (see [`Params::for_strangers`]):
    /// as they are for strangers up to 0.05 alike (actors), fully moved from
    /// 0.15 on (animation), in between in part; fully moved however alike
    /// they are when the footage is known to be [`Params::animated`]. One
    /// fit for the whole project: one mixing live action with animation
    /// gets the bars of what most of its faces seen together are.
    pub fn fitted(&self, tracks: &[TrackIn]) -> Params {
        strangers(tracks, self).map_or(*self, |s| self.for_strangers(s * self.style_share(s)))
    }

    /// How far faces whose strangers look `alike` are fitted, 0..=1 (see
    /// [`Params::strangers_alone`]); fully when the footage is known to be
    /// [`Params::animated`].
    pub fn style_share(&self, alike: f32) -> f32 {
        if self.animated {
            return 1.0;
        }
        ((alike - self.strangers_alone) / (self.strangers_style - self.strangers_alone).max(1e-6)).clamp(0.0, 1.0)
    }
}

/// The embedding length this pass compares in: the most common one — the
/// session's embedder's (ArcFace's 512, CCIP's 768); a tie goes to the
/// first seen.
fn session_dim(tracks: &[TrackIn]) -> Option<usize> {
    let mut count: HashMap<usize, usize> = HashMap::new();
    for t in tracks {
        *count.entry(t.embedding.len()).or_default() += 1;
    }
    let mut best: Option<usize> = None;
    for t in tracks {
        let d = t.embedding.len();
        if best.is_none_or(|b| count[&d] > count[&b]) {
            best = Some(d);
        }
    }
    best
}

/// The tracks sharing [`session_dim`]'s length, in input order; the rest
/// compare to no one (a face another embedder made waits for a pass of
/// its own) and are warned about once per pass.
fn matching_dim(tracks: &[TrackIn]) -> Vec<usize> {
    let dim = session_dim(tracks);
    let kept: Vec<usize> = (0..tracks.len()).filter(|&i| Some(tracks[i].embedding.len()) == dim).collect();
    if kept.len() < tracks.len() {
        tracing::warn!(
            tracks = tracks.len(),
            kept = kept.len(),
            "faces of another embedder's dimension left out of the cast's grouping"
        );
    }
    kept
}

/// `out`, computed on `tracks[kept]`, given back for every track: a
/// left-out one is no one's.
fn scatter(n: usize, kept: &[usize], out: Outcome) -> Outcome {
    let mut placed = vec![(Assign::None, 0f32); n];
    for (k, &i) in kept.iter().enumerate() {
        placed[i] = out.tracks[k];
    }
    Outcome { tracks: placed, ..out }
}

/// How much a pair of faces counts in the footage's statistics: as its
/// less-seen face ([`TrackIn::weight`]), never less than nothing.
fn pair_weight(tracks: &[TrackIn], a: usize, b: usize) -> f64 {
    let w = tracks[a].weight.min(tracks[b].weight);
    if w.is_finite() { w.max(0.0) as f64 } else { 0.0 }
}

/// Group `tracks` into characters. Faces of another embedding length than
/// the session's stay unassigned (`matching_dim`).
pub fn identify(tracks: &[TrackIn], p: &Params) -> Outcome {
    let kept = matching_dim(tracks);
    if kept.len() == tracks.len() {
        return identify_run(tracks, p);
    }
    let sub: Vec<TrackIn> = kept.iter().map(|&i| tracks[i].clone()).collect();
    scatter(tracks.len(), &kept, identify_run(&sub, p))
}

fn identify_run(tracks: &[TrackIn], p: &Params) -> Outcome {
    match p.adapt.then(|| fit(tracks, p)).flatten() {
        Some((f, _)) => Outcome { space: Space::Fitted(f.clone()), ..identify_in(&f.tracks(tracks), &f.params(p)) },
        None => identify_in(tracks, &p.fitted(tracks)),
    }
}

fn identify_in(tracks: &[TrackIn], p: &Params) -> Outcome {
    let n = tracks.len();
    // A track the user said "not them" about doesn't shape any group: it
    // would drag the rest of its person away from that character.
    let is_core: Vec<bool> = tracks
        .iter()
        .map(|t| t.pin.is_some() || (t.forbid.is_empty() && t.quality >= p.core_quality && t.dets >= p.core_dets))
        .collect();
    let core: Vec<usize> = (0..n).filter(|&i| is_core[i]).collect();

    // 1. Constrained clustering of the core tracks.
    let clustered = cluster(tracks, &core, p);
    // Per track, the group it left the clustering in (for `held_by_pins`).
    let mut company = vec![usize::MAX; n];
    for (gi, g) in clustered.iter().enumerate() {
        g.iter().for_each(|&i| company[i] = gi);
    }
    let groups = pins_together(tracks, &core, clustered);

    // 2. Which groups are characters; which existing character each one is.
    let frames = |g: &[usize]| -> u32 { g.iter().map(|&i| tracks[i].end - tracks[i].start).sum() };
    let pinned_key = |g: &[usize]| g.iter().find_map(|&i| tracks[i].pin);
    let qualifies =
        |g: &[usize]| g.len() >= p.min_tracks && frames(g) >= p.min_frames && cohesion(tracks, g) >= p.min_cohesion;
    let mut chars: Vec<(Vec<usize>, Option<CharKey>)> = Vec::new();
    let mut loose: Vec<usize> = (0..n).filter(|&i| !is_core[i]).collect();
    for g in groups {
        if pinned_key(&g).is_some() || qualifies(&g) {
            chars.push((g, None));
        } else {
            loose.extend(g);
        }
    }
    keep_own_faces(tracks, &core, &mut chars, &loose, p, &company);
    let copied = drop_copies(tracks, &mut chars, &mut loose, p);
    // Pins decide first; then the biggest previous overlaps, one to one.
    let mut taken: HashSet<CharKey> = HashSet::new();
    for (g, key) in &mut chars {
        if let Some(k) = pinned_key(g) {
            *key = Some(k);
            taken.insert(k);
        }
    }
    // How much of each earlier character's clear tracks this run has.
    let mut before: HashMap<CharKey, f32> = HashMap::new();
    for &i in &core {
        if let Some(k) = tracks[i].previous {
            *before.entry(k).or_default() += tracks[i].quality;
        }
    }
    let mut inherits: Vec<(f32, usize, CharKey)> = Vec::new();
    for (ci, (g, key)) in chars.iter().enumerate() {
        if key.is_some() {
            continue;
        }
        let total: f32 = g.iter().map(|&i| tracks[i].quality).sum::<f32>().max(1e-6);
        let mut by: HashMap<CharKey, f32> = HashMap::new();
        for &i in g {
            if let Some(k) = tracks[i].previous {
                *by.entry(k).or_default() += tracks[i].quality;
            }
        }
        for (k, w) in by {
            // Enough of the group was that character — or most of that
            // character went into the group (it grew with new episodes).
            if w / total >= p.inherit || w / before[&k].max(1e-6) > 0.5 {
                inherits.push((w, ci, k));
            }
        }
    }
    inherits.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (_, ci, k) in inherits {
        if chars[ci].1.is_none() && !taken.contains(&k) {
            chars[ci].1 = Some(k);
            taken.insert(k);
        }
    }

    // Earlier characters no group took, grouped into one that did: more
    // than half of their clear tracks are its members now (or copies of it).
    let mut group_of: HashMap<usize, usize> = copied;
    for (ci, (g, _)) in chars.iter().enumerate() {
        group_of.extend(g.iter().map(|&i| (i, ci)));
    }
    let mut had: HashMap<CharKey, f32> = HashMap::new();
    let mut went: HashMap<(CharKey, usize), f32> = HashMap::new();
    for &i in &core {
        let Some(k) = tracks[i].previous.filter(|k| !taken.contains(k)) else { continue };
        *had.entry(k).or_default() += tracks[i].quality;
        if let Some(&ci) = group_of.get(&i) {
            *went.entry((k, ci)).or_default() += tracks[i].quality;
        }
    }
    let mut absorbed: Vec<(CharKey, usize)> =
        went.into_iter().filter(|&((k, _), w)| w > 0.5 * had[&k]).map(|(kc, _)| kc).collect();
    absorbed.sort_unstable();

    Outcome { absorbed, ..place(tracks, &chars, loose, p) }
}

/// The user decided about some faces: let the others follow at once,
/// without grouping everything again. `members` are the tracks that define
/// each existing character (its pins and the tracks clustered into it);
/// every other track joins the character it clearly resembles now. No
/// character is made or merged, and a pin never moves. A member the user
/// said isn't them stops shaping the character, so faces that only
/// resembled that one leave with it. Faces are compared in the `space` the
/// grouping used (`None`: unknown, fitted again as [`identify`] does).
/// Faces of another embedding length than the session's stay unassigned.
pub fn reassign(tracks: &[TrackIn], members: &[(CharKey, Vec<usize>)], p: &Params, space: Option<&Space>) -> Outcome {
    let kept = matching_dim(tracks);
    if kept.len() == tracks.len() {
        return reassign_run(tracks, members, p, space);
    }
    // Member lists index `tracks`; move them to the kept tracks' positions.
    let mut at = vec![usize::MAX; tracks.len()];
    kept.iter().enumerate().for_each(|(k, &i)| at[i] = k);
    let members: Vec<(CharKey, Vec<usize>)> = members
        .iter()
        .map(|(k, g)| (*k, g.iter().filter_map(|&i| at.get(i).copied()).filter(|&j| j != usize::MAX).collect()))
        .collect();
    let sub: Vec<TrackIn> = kept.iter().map(|&i| tracks[i].clone()).collect();
    scatter(tracks.len(), &kept, reassign_run(&sub, &members, p, space))
}

fn reassign_run(tracks: &[TrackIn], members: &[(CharKey, Vec<usize>)], p: &Params, space: Option<&Space>) -> Outcome {
    let (faces, params, space) = in_space(tracks, p, space);
    let out = reassign_in(&faces, members, &params);
    match space {
        Space::Fitted(_) => Outcome { space, ..out },
        Space::Faces => out,
    }
}

/// `tracks` as the cast compares them: in `space` (`None`: unknown, fitted
/// again as [`identify`] does), with the bars of that space.
fn in_space<'a>(tracks: &'a [TrackIn], p: &Params, space: Option<&Space>) -> (Cow<'a, [TrackIn]>, Params, Space) {
    // A space fitted on another embedder's faces can't place these.
    let space = match (space, tracks.first()) {
        (Some(Space::Fitted(f)), Some(t)) if f.mean.len() != t.embedding.len() => {
            tracing::warn!("the space the cast was grouped in is another embedder's; fitting again");
            None
        }
        (s, _) => s,
    };
    let fitted = match space {
        Some(s) => Some(s.clone()),
        None => p.adapt.then(|| fit(tracks, p)).flatten().map(|(f, _)| Space::Fitted(f)),
    };
    match fitted {
        Some(Space::Fitted(f)) => (Cow::Owned(f.tracks(tracks)), f.params(p), Space::Fitted(f)),
        _ => (Cow::Borrowed(tracks), p.fitted(tracks), Space::Faces),
    }
}

fn reassign_in(tracks: &[TrackIn], members: &[(CharKey, Vec<usize>)], p: &Params) -> Outcome {
    let n = tracks.len();
    let mut member = vec![false; n];
    let mut chars: Vec<(Vec<usize>, Option<CharKey>)> = Vec::with_capacity(members.len());
    for (key, group) in members {
        // As in `identify`: a track the user said "not them" about defines
        // no one (unless pinned).
        let g: Vec<usize> = group
            .iter()
            .copied()
            .filter(|&i| i < n && !member[i] && (tracks[i].pin == Some(*key) || tracks[i].forbid.is_empty()))
            .collect();
        if g.is_empty() {
            continue;
        }
        g.iter().for_each(|&i| member[i] = true);
        chars.push((g, Some(*key)));
    }
    let loose = (0..n).filter(|&i| !member[i]).collect();
    place(tracks, &chars, loose, p)
}

/// Step 3: characters from their members (prototypes, where they are on
/// screen), then every `loose` track, most similar first.
fn place(tracks: &[TrackIn], chars: &[(Vec<usize>, Option<CharKey>)], loose: Vec<usize>, p: &Params) -> Outcome {
    let n = tracks.len();
    let mut out = vec![(Assign::None, 0f32); n];
    let mut characters = Vec::with_capacity(chars.len());
    let mut protos: Vec<Vec<Vec<f32>>> = Vec::with_capacity(chars.len());
    // Per character and media, the members' time spans (cannot-link for the rest).
    let mut spans: Vec<HashMap<u32, Vec<(u32, u32)>>> = Vec::with_capacity(chars.len());
    for (ci, (g, key)) in chars.iter().enumerate() {
        let mut sp: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
        for &i in g {
            out[i] = (if tracks[i].pin.is_some() { Assign::Pinned(ci) } else { Assign::Auto(ci) }, 1.0);
            sp.entry(tracks[i].media).or_default().extend(tracks[i].presence());
        }
        let members: Vec<(&[f32], f32)> =
            g.iter().map(|&i| (tracks[i].embedding.as_slice(), tracks[i].quality.max(0.05))).collect();
        characters.push(CharacterOut { key: *key, centroid: proto::centroid(&members) });
        protos.push(proto::prototypes(&members, p.prototypes, p.table_dedup_cos));
        spans.push(sp);
    }
    // Every other track, most similar first: a character already on screen
    // in a frame (a member, or a closer look-alike placed before) can't take
    // a second face there — on screen with its short gaps bridged, stricter
    // than clustering's faces found in one frame: a stranger found while
    // their face was missed for a moment doesn't become them, and a piece
    // of their own face found there loses nothing (they are on screen
    // anyway). A forbidden character never gets a track. A face
    // most like a character on screen in the same frames — their reflection,
    // a face on a screen, someone beside them who resembles them — goes to
    // another character only when it clearly is them, never as a guess.
    let mut loose = scored(tracks, loose, &characters, &protos);
    let top = |s: &[(f32, usize)]| s.first().map_or(f32::NEG_INFINITY, |x| x.0);
    loose.sort_by(|a, b| top(&b.1).total_cmp(&top(&a.1)).then(a.0.cmp(&b.0)));
    for (i, scores) in loose {
        let t = &tracks[i];
        let here = t.presence();
        let taken = |ci: usize| spans[ci].get(&t.media).is_some_and(|v| overlaps(&here, v));
        let beside = scores.first().is_some_and(|&(_, ci)| taken(ci));
        let mut free = scores.into_iter().filter(|&(_, ci)| !taken(ci));
        let Some((s1, c1)) = free.next() else { continue };
        let s2 = free.next().map_or(-1.0, |x| x.0);
        out[i] = if s1 >= p.assign && s1 - s2 >= p.margin {
            spans[c1].entry(t.media).or_default().extend(here);
            (Assign::Auto(c1), s1)
        } else if s1 >= p.unsure && !beside {
            (Assign::Unsure(c1), s1)
        } else {
            (Assign::None, s1)
        };
    }
    // Forbidden assignments inside clusters (the cluster turned out to be
    // that character) are undone.
    for (i, t) in tracks.iter().enumerate() {
        if let Assign::Auto(ci) | Assign::Unsure(ci) = out[i].0
            && characters[ci].key.is_some_and(|k| t.forbid.contains(&k))
        {
            out[i] = (Assign::None, 0.0);
        }
    }
    Outcome { characters, tracks: out, absorbed: Vec::new(), space: Space::Faces }
}

/// How much each of `loose` resembles each character it may be (best
/// first), on all cores: a big series has tens of thousands of faces.
fn scored(
    tracks: &[TrackIn],
    loose: Vec<usize>,
    characters: &[CharacterOut],
    protos: &[Vec<Vec<f32>>],
) -> Vec<(usize, Vec<(f32, usize)>)> {
    let score = |i: usize| {
        let t = &tracks[i];
        let mut scores: Vec<(f32, usize)> = (0..characters.len())
            .filter(|&ci| characters[ci].key.is_none_or(|k| !t.forbid.contains(&k)))
            .map(|ci| (proto::similarity(&t.embedding, &protos[ci]), ci))
            .collect();
        scores.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        (i, scores)
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
    if threads < 2 || loose.len() * characters.len() < 20_000 {
        return loose.into_iter().map(score).collect();
    }
    let chunk = loose.len().div_ceil(threads);
    std::thread::scope(|s| {
        let parts: Vec<_> = loose
            .chunks(chunk)
            .map(|part| s.spawn(move || part.iter().map(|&i| score(i)).collect::<Vec<_>>()))
            .collect();
        parts.into_iter().flat_map(|h| h.join().expect("scoring never panics")).collect()
    })
}

#[cfg(test)]
mod tests;
