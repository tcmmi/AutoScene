//! A second look at faces: who each one is, asked of every character at
//! once. Nothing changes by asking: a verdict is a suggestion, and only the
//! person's answer moves a face.
//!
//! A face is compared with every shot of every character, not with a summary
//! of them: a look a character has in a few shots only (a disguise, a costume)
//! would drown in any average. What a character scores is how alike the faces
//! of its best few shots are, so one stray face can't carry it: a person's
//! rare look still shows in several shots, a chance likeness in one.
//!
//! A face that defines a character is held out of it, with the rest of its
//! shot: a face can't vouch for itself.
//!
//! The bars are the cast's own, in the space it was grouped in, raised by
//! [`Tuning::lift`]: the closest faces of a person lie nearer than the middle of
//! their looks that grouping compares with.

use std::collections::{HashMap, HashSet};

use crate::{CharKey, Params, Space, TrackIn, in_space, overlaps, proto, session_dim};

/// With fewer faces than this left after holding out, a character can't say
/// a face is theirs.
const MIN_ANCHORS: usize = 3;
/// ... or from fewer shots than this; also how many shots of a character
/// must resemble a face for it to be suggested.
const MIN_SHOTS: usize = 2;

/// How a verdict is reached; set on real casts (`rescan_bench`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// A character scores the mean of its this many best shots.
    pub shots: usize,
    /// How far above the cast's bars a clear verdict lies.
    pub lift: f32,
    /// How far above its `unsure` bar a guess lies.
    pub lift_unsure: f32,
    /// The lead a clear verdict needs, as a share of the cast's margin.
    pub margin: f32,
    /// A character on screen within `near` frames of the face is likelier: a
    /// scene keeps its people, also when one of them turns away. Added to
    /// their score, in full right beside the face, fading to nothing at `near`.
    pub near_bonus: f32,
    pub near: u32,
    /// A sharper reading counts from this share of the stored face's quality:
    /// one that came out much worse than what the pass saw is no sharper.
    pub read_floor: f32,
}

impl Tuning {
    /// As measured for faces compared in `space`. Set on the Miraculous film
    /// (fitted space: looks are close, the scene says much) and on Breaking
    /// Bad's labels (actors: looks say much, and a stranger in the scene is
    /// still a stranger).
    pub fn of(space: &Space) -> Self {
        match space {
            Space::Fitted(_) => Self::default(),
            Space::Faces => Self { lift: 0.05, lift_unsure: 0.1, near_bonus: 0.05, ..Self::default() },
        }
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Self { shots: 2, lift: 0.05, lift_unsure: -0.05, margin: 1.0, near_bonus: 0.15, near: 480, read_floor: 0.5 }
    }
}

/// A sharper reading of a face: its embedding (unit length, of the cast's
/// embedder) in place of the stored one.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    /// Index into [`Question::tracks`].
    pub track: usize,
    pub embedding: Vec<f32>,
    pub quality: f32,
}

/// What is asked.
#[derive(Clone, Copy, Debug)]
pub struct Question<'a> {
    pub tracks: &'a [TrackIn],
    /// Per track, the first frame of its shot.
    pub shots: &'a [u32],
    /// What defines each character: its pins and the faces clustered into it.
    pub members: &'a [(CharKey, Vec<usize>)],
    /// Per track, the character it is on now (a face named by its body too).
    pub now: &'a [Option<CharKey>],
    /// The faces to judge.
    pub subjects: &'a [usize],
    pub reads: &'a [Read],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It looks like the character it is on.
    Right,
    /// It is on a character it looks too little like.
    Doubt,
    /// It is on one character and clearly another.
    Elsewhere(CharKey),
    /// It is on no one and clearly this character.
    Newly(CharKey),
    /// It is on no one and may be this character.
    Maybe(CharKey),
    /// It could be either of these (its own, if it has one, comes first).
    Between(CharKey, CharKey),
    /// It looks like no one.
    Nobody,
    /// Its character has too few other faces to say.
    Thin,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Judged {
    /// Index into [`Question::tracks`].
    pub track: usize,
    pub verdict: Verdict,
    /// How much it resembles the characters it may be, best first.
    pub scores: Vec<(f32, CharKey)>,
}

/// Who each of `q.subjects` is. Faces are compared in `space` (`None`:
/// unknown, fitted again as [`crate::identify`] does); nothing is decided.
pub fn judge(q: &Question, p: &Params, space: Option<&Space>) -> Vec<Judged> {
    judge_tuned(q, p, space, &Tuning::default())
}

/// [`judge`] with `tuning` (for the bench).
pub fn judge_tuned(q: &Question, p: &Params, space: Option<&Space>, tuning: &Tuning) -> Vec<Judged> {
    let Some(dim) = session_dim(q.tracks) else { return Vec::new() };
    let usable = |i: usize| q.tracks.get(i).is_some_and(|t| t.embedding.len() == dim);
    let (faces, p, space) = in_space(q.tracks, p, space);
    let tracks: &[TrackIn] = &faces;
    let (assign, unsure) = (p.assign + tuning.lift, p.unsure + tuning.lift_unsure);
    let margin = p.margin * tuning.margin;

    // Sharper readings, in the same space as everything else.
    let mut read: HashMap<usize, Vec<f32>> = HashMap::new();
    for r in q.reads {
        let sharper = usable(r.track) && r.quality >= q.tracks[r.track].quality * tuning.read_floor;
        if sharper && r.embedding.len() == dim && r.embedding.iter().all(|x| x.is_finite()) {
            let e = match &space {
                Space::Fitted(f) => f.embed(&r.embedding),
                Space::Faces => r.embedding.clone(),
            };
            read.insert(r.track, e);
        }
    }

    // As in `reassign`: a face said not to be them defines no one (unless pinned).
    let chars: Vec<(CharKey, Vec<usize>)> = q
        .members
        .iter()
        .map(|(key, g)| {
            let keep = |i: &usize| usable(*i) && (tracks[*i].pin == Some(*key) || tracks[*i].forbid.is_empty());
            (*key, g.iter().copied().filter(|i| keep(i)).collect())
        })
        .collect();
    let index: HashMap<CharKey, usize> = chars.iter().enumerate().map(|(ci, (k, _))| (*k, ci)).collect();
    let shot = |i: usize| (tracks[i].media, q.shots[i]);
    let anchor: HashSet<usize> = chars.iter().flat_map(|(_, g)| g.iter().copied()).collect();

    // Who is on screen when: every face on a character counts, whether it
    // defines them or was placed there.
    type Screen = HashMap<u32, Vec<(usize, Vec<(u32, u32)>)>>;
    let mut screen: Vec<Screen> = vec![HashMap::new(); chars.len()];
    for (i, now) in q.now.iter().enumerate() {
        if let Some(&ci) = now.and_then(|k| index.get(&k).copied()).as_ref().filter(|_| usable(i)) {
            screen[ci].entry(tracks[i].media).or_default().push((i, tracks[i].presence()));
        }
    }

    let embedding = |i: usize| read.get(&i).map_or(tracks[i].embedding.as_slice(), Vec::as_slice);
    // What a face makes of a character: its score, how many shots look like
    // it, and whether enough of the character is left to go by.
    let compare = |i: usize, e: &[f32], g: &[usize]| -> Sight {
        let held_out = anchor.contains(&i);
        let mut best: HashMap<(u32, u32), f32> = HashMap::new();
        let (mut left, mut gap) = (0, u32::MAX);
        for &j in g {
            if j == i || (held_out && shot(j) == shot(i)) {
                continue;
            }
            left += 1;
            let (a, b) = (&tracks[i], &tracks[j]);
            // On screen at the same time speaks against, not for: that is
            // someone else in the picture.
            if a.media == b.media && (a.start >= b.end || b.start >= a.end) {
                gap = gap.min(a.start.saturating_sub(b.end).max(b.start.saturating_sub(a.end)));
            }
            let s = proto::dot(e, &tracks[j].embedding);
            let b = best.entry(shot(j)).or_insert(f32::NEG_INFINITY);
            *b = b.max(s);
        }
        let mut shots: Vec<f32> = best.into_values().collect();
        shots.sort_by(|a, b| b.total_cmp(a));
        let n = tuning.shots.min(shots.len());
        // Only a face too unclear to tell: a sharp one that doesn't look like
        // them isn't them, however close by they are.
        let unclear = q.tracks[i].quality < p.core_quality;
        let near = if unclear { tuning.near_bonus } else { 0.0 };
        let bonus = near * (1.0 - gap as f32 / tuning.near.max(1) as f32).max(0.0);
        let score =
            if n == 0 { f32::NEG_INFINITY } else { shots[..n].iter().sum::<f32>() / tuning.shots as f32 + bonus };
        Sight {
            score,
            looks: score - bonus,
            in_scene: bonus > 0.0,
            alike: shots.iter().filter(|&&s| s + bonus >= unsure).count(),
            thin: left < MIN_ANCHORS || shots.len() < MIN_SHOTS,
        }
    };
    let look = |i: usize| -> Asked {
        let e = embedding(i);
        let sights: Vec<Sight> = chars.iter().map(|(_, g)| compare(i, e, g)).collect();
        let mut scores: Vec<(f32, usize)> = (0..chars.len())
            .filter(|&ci| sights[ci].score.is_finite() && !tracks[i].forbid.contains(&chars[ci].0))
            .map(|ci| (sights[ci].score, ci))
            .collect();
        scores.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        (i, sights, scores)
    };
    let subjects: Vec<usize> = q.subjects.iter().copied().filter(|&i| usable(i)).collect();
    // Every face against every shot: on all cores for a big series.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
    let mut asked: Vec<Asked> = if subjects.len() < 64 || threads < 2 {
        subjects.iter().map(|&i| look(i)).collect()
    } else {
        let part = subjects.len().div_ceil(threads);
        std::thread::scope(|s| {
            let look = &look;
            let parts: Vec<_> = subjects
                .chunks(part)
                .map(|c| s.spawn(move || c.iter().map(|&i| look(i)).collect::<Vec<_>>()))
                .collect();
            parts.into_iter().flat_map(|h| h.join().expect("comparing never panics")).collect()
        })
    };
    let top = |s: &[(f32, usize)]| s.first().map_or(f32::NEG_INFINITY, |x| x.0);
    asked.sort_by(|a, b| top(&b.2).total_cmp(&top(&a.2)).then(a.0.cmp(&b.0)));

    let key = |ci: usize| chars[ci].0;
    let mut out = Vec::with_capacity(asked.len());
    for (i, sights, scores) in asked {
        let t = &tracks[i];
        let here = t.presence();
        // As in `place`: a character on screen in these frames can't be this
        // face too. Its own spans don't count against it.
        let taken =
            |ci: usize| screen[ci].get(&t.media).is_some_and(|v| v.iter().any(|(j, s)| *j != i && overlaps(&here, s)));
        // Whether more than one shot of a character looks like this face.
        let backed = |ci: usize| sights[ci].alike >= MIN_SHOTS;
        let clear = |s: f32, ci: usize, next: f32| {
            sights[ci].looks >= assign && s - next >= margin && !sights[ci].thin && backed(ci)
        };
        // In the scene, one shot that looks like it is enough for a guess.
        let close = |s: f32, ci: usize| s >= unsure && (backed(ci) || (sights[ci].in_scene && sights[ci].alike > 0));

        // Who it looks most like, the scene aside.
        let lookalike = scores.iter().max_by(|a, b| sights[a.1].looks.total_cmp(&sights[b.1].looks));
        // Only a real likeness: the nearest of faces it looks nothing like says nothing.
        let beside = lookalike.is_some_and(|&(_, ci)| taken(ci) && sights[ci].looks >= unsure);
        let free: Vec<(f32, usize)> = scores.iter().copied().filter(|&(_, ci)| !taken(ci)).collect();
        let second = free.get(1).copied();
        let next = second.map_or(-1.0, |x| x.0);
        // The runner-up, when it is as good a candidate as the first.
        let rival = |s1: f32| second.filter(|&(s2, c2)| close(s2, c2) && s1 - s2 < margin).map(|x| x.1);
        let own = q.now[i].and_then(|k| index.get(&k).copied());
        // The one it looks most like, when they are on screen already and it
        // looks clearly like them; with who else it may be.
        let twice = lookalike
            .filter(|&&(_, ci)| taken(ci) && sights[ci].looks >= assign)
            .map(|&(s0, ci)| (ci, free.first().filter(|&&(s, c)| s0 - s < margin && close(s, c)).map(|x| x.1)));

        let verdict = match (own, free.first().copied()) {
            (Some(_), None) => Verdict::Doubt,
            (None, None) => Verdict::Nobody,
            // Far more like someone already in the picture than like anyone
            // else: a second of them (a mirror, a copy, a clone) is for the
            // person to judge, so it is put to them, never placed.
            (None, _) if twice.is_some() => {
                let (c0, other) = twice.expect("just looked");
                other.map_or(Verdict::Maybe(key(c0)), |c| Verdict::Between(key(c0), key(c)))
            }
            (None, Some((s1, c1))) => {
                if clear(s1, c1, next) {
                    Verdict::Newly(key(c1))
                } else if beside || !close(s1, c1) {
                    // A face most like someone beside it is never a guess.
                    Verdict::Nobody
                } else if let Some(c2) = rival(s1) {
                    Verdict::Between(key(c1), key(c2))
                } else {
                    Verdict::Maybe(key(c1))
                }
            }
            (Some(o), Some((s1, c1))) => {
                let own_score = scores.iter().find(|x| x.1 == o).map_or(f32::NEG_INFINITY, |x| x.0);
                if sights[o].thin {
                    if c1 != o && clear(s1, c1, next) { Verdict::Elsewhere(key(c1)) } else { Verdict::Thin }
                } else if c1 == o {
                    match rival(s1) {
                        _ if s1 < unsure => Verdict::Doubt,
                        Some(c2) => Verdict::Between(key(o), key(c2)),
                        None => Verdict::Right,
                    }
                } else if clear(s1, c1, next) {
                    Verdict::Elsewhere(key(c1))
                } else if own_score >= unsure && close(s1, c1) {
                    Verdict::Between(key(o), key(c1))
                } else if own_score >= unsure {
                    Verdict::Right
                } else {
                    Verdict::Doubt
                }
            }
        };
        // The user's word stands: at most a doubt is raised about it.
        let verdict = match verdict {
            Verdict::Elsewhere(_) if t.pin.is_some() => Verdict::Doubt,
            _ if t.pin.is_some() => Verdict::Right,
            v => v,
        };
        // Two faces of one frame can't both go to one character.
        if let Verdict::Elsewhere(k) | Verdict::Newly(k) = verdict
            && let Some(&ci) = index.get(&k)
        {
            screen[ci].entry(t.media).or_default().push((i, here.clone()));
        }
        let scores = scores.iter().map(|&(s, ci)| (s, key(ci))).collect();
        out.push(Judged { track: i, verdict, scores });
    }
    out
}

/// A face, what it makes of each character, and who it may be, best first.
type Asked = (usize, Vec<Sight>, Vec<(f32, usize)>);

/// What a face makes of one character.
#[derive(Clone, Copy, Debug)]
struct Sight {
    /// How likely they are, the scene included.
    score: f32,
    /// How alike they look, nothing else: all a clear verdict goes by.
    looks: f32,
    /// They are on screen around it.
    in_scene: bool,
    /// Shots of theirs that look like it.
    alike: usize,
    /// Too little of them is left to go by.
    thin: bool,
}

/// How alike two groups of faces are: each face's best match in the other
/// group, averaged both ways. A person with several looks still finds
/// themselves in the other group's look that fits; a mean of means wouldn't.
pub fn likeness(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let best = |from: &[Vec<f32>], to: &[Vec<f32>]| -> f32 {
        let sum: f32 = from.iter().map(|e| proto::similarity(e, to)).sum();
        sum / from.len() as f32
    };
    (best(a, b) + best(b, a)) / 2.0
}

/// In how many frames faces of `a` and `b` (indexes into `tracks`) were both
/// found: two faces at once.
pub fn frames_together(tracks: &[TrackIn], a: &[usize], b: &[usize]) -> u32 {
    let seen: HashSet<(u32, u32)> =
        a.iter().flat_map(|&i| tracks[i].sightings().into_iter().map(move |f| (tracks[i].media, f))).collect();
    let both: HashSet<(u32, u32)> = b
        .iter()
        .flat_map(|&i| tracks[i].sightings().into_iter().map(move |f| (tracks[i].media, f)))
        .filter(|k| seen.contains(k))
        .collect();
    both.len() as u32
}

/// The space the cast compares faces in and the bars of it: `space`, or
/// fitted to `tracks` when it is unknown. Worked out once for a whole job.
pub fn cast_space(tracks: &[TrackIn], p: &Params, space: Option<&Space>) -> (Space, Params) {
    let (_, p, space) = in_space(tracks, p, space);
    (space, p)
}

/// `faces` (embeddings as tracks keep them) in `space`.
pub fn in_cast_space(faces: &[Vec<f32>], space: &Space) -> Vec<Vec<f32>> {
    match space {
        Space::Fitted(f) => faces.iter().map(|e| f.embed(e)).collect(),
        Space::Faces => faces.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIM: usize = 8;

    fn unit(axis: usize) -> Vec<f32> {
        let mut e = vec![0f32; DIM];
        e[axis] = 1.0;
        e
    }

    /// A face of `axis` on screen at `at`, in the shot starting there.
    fn face(axis: usize, at: u32) -> TrackIn {
        TrackIn {
            media: 0,
            start: at,
            end: at + 10,
            spans: Vec::new(),
            seen: Vec::new(),
            embedding: unit(axis),
            quality: 0.9,
            dets: 4,
            weight: 1.0,
            pin: None,
            forbid: Vec::new(),
            previous: None,
        }
    }

    type Cast = (Vec<TrackIn>, Vec<(CharKey, Vec<usize>)>, Vec<Option<CharKey>>);

    /// Two characters of ten faces each, in shots of their own.
    fn cast() -> Cast {
        let mut tracks = Vec::new();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for n in 0..10u32 {
            a.push(tracks.len());
            tracks.push(face(0, n * 100));
            b.push(tracks.len());
            tracks.push(face(1, n * 100 + 50));
        }
        let now = (0..tracks.len()).map(|i| Some(if a.contains(&i) { 0 } else { 1 })).collect();
        (tracks, vec![(0, a), (1, b)], now)
    }

    fn params() -> Params {
        Params { adapt: false, ..Params::default() }
    }

    fn ask(c: &Cast, subjects: &[usize], reads: &[Read]) -> Vec<Judged> {
        let (tracks, members, now) = c;
        let shots: Vec<u32> = tracks.iter().map(|t| t.start).collect();
        let q = Question { tracks, shots: &shots, members, now, subjects, reads };
        let t = Tuning { lift: 0.0, lift_unsure: 0.0, near_bonus: 0.0, ..Tuning::default() };
        judge_tuned(&q, &params(), Some(&Space::Faces), &t)
    }

    fn verdict_of(j: &[Judged], track: usize) -> Verdict {
        j.iter().find(|x| x.track == track).expect("judged").verdict
    }

    /// `c` with one more face on `now`; its index.
    fn with(c: &mut Cast, t: TrackIn, now: Option<CharKey>) -> usize {
        c.0.push(t);
        c.2.push(now);
        c.0.len() - 1
    }

    #[test]
    fn a_face_on_its_own_character_stays_right() {
        assert_eq!(ask(&cast(), &[0], &[])[0].verdict, Verdict::Right);
    }

    #[test]
    fn a_face_on_the_wrong_character_is_said_to_be_elsewhere() {
        let mut c = cast();
        let i = with(&mut c, face(0, 5000), Some(1));
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Elsewhere(0));
    }

    #[test]
    fn a_face_on_no_one_is_found_and_a_stranger_is_not() {
        let mut c = cast();
        let known = with(&mut c, face(1, 5000), None);
        let stranger = with(&mut c, face(5, 6000), None);
        let j = ask(&c, &[known, stranger], &[]);
        assert_eq!(verdict_of(&j, known), Verdict::Newly(1));
        assert_eq!(verdict_of(&j, stranger), Verdict::Nobody);
    }

    #[test]
    fn a_sharper_reading_decides_over_the_stored_face() {
        let mut c = cast();
        let i = with(&mut c, face(5, 5000), None);
        let read = Read { track: i, embedding: unit(0), quality: 0.9 };
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Nobody);
        assert_eq!(ask(&c, &[i], &[read])[0].verdict, Verdict::Newly(0));
    }

    #[test]
    fn a_face_alike_two_characters_is_left_between_them() {
        let mut c = cast();
        let mut t = face(5, 5000);
        t.embedding = vec![0f32; DIM];
        t.embedding[0] = std::f32::consts::FRAC_1_SQRT_2;
        t.embedding[1] = std::f32::consts::FRAC_1_SQRT_2;
        let i = with(&mut c, t, None);
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Between(0, 1));
    }

    #[test]
    fn a_character_ruled_out_is_never_suggested() {
        let mut c = cast();
        let mut t = face(0, 5000);
        t.forbid = vec![0];
        let i = with(&mut c, t, None);
        let j = ask(&c, &[i], &[]);
        assert_eq!(j[0].verdict, Verdict::Nobody);
        assert!(j[0].scores.iter().all(|&(_, k)| k != 0));
    }

    #[test]
    fn a_face_doesnt_vouch_for_itself_or_its_shot() {
        // Two stray faces in one shot, among six of someone else: held out,
        // what is left doesn't look like them.
        let mut tracks = vec![face(5, 100), face(5, 100)];
        tracks.extend((0..6).map(|n| face(0, 1000 + n * 100)));
        let members = vec![(0, (0..tracks.len()).collect::<Vec<_>>())];
        let now = vec![Some(0); tracks.len()];
        assert_eq!(ask(&(tracks, members, now), &[0], &[])[0].verdict, Verdict::Doubt);
    }

    #[test]
    fn one_stray_shot_is_no_support() {
        let mut c = cast();
        let stray = with(&mut c, face(5, 7000), Some(0));
        c.1[0].1.push(stray);
        let i = with(&mut c, face(5, 8000), None);
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Nobody);
    }

    #[test]
    fn a_small_group_cant_keep_a_face() {
        let mut c = cast();
        let a = with(&mut c, face(5, 7000), Some(2));
        let b = with(&mut c, face(5, 8000), Some(2));
        c.1.push((2, vec![a, b]));
        assert_eq!(ask(&c, &[b], &[])[0].verdict, Verdict::Thin);
    }

    #[test]
    fn a_rare_look_counts_as_much_as_a_common_one() {
        // Ten shots of one look, two of a disguise: neither averages the other away.
        let mut c = cast();
        for at in [7000, 7100] {
            let i = with(&mut c, face(4, at), Some(0));
            c.1[0].1.push(i);
        }
        let i = with(&mut c, face(4, 9000), None);
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Newly(0));
    }

    #[test]
    fn a_reading_much_worse_than_the_face_is_no_reading() {
        let mut c = cast();
        let i = with(&mut c, face(0, 5000), None);
        let read = Read { track: i, embedding: unit(5), quality: 0.05 };
        assert_eq!(ask(&c, &[i], &[read])[0].verdict, Verdict::Newly(0));
    }

    #[test]
    fn the_scene_makes_a_guess_never_a_clear_answer() {
        let (tracks, members, now) = {
            let mut c = cast();
            let mut t = face(5, 860);
            t.embedding = vec![0f32; DIM];
            t.embedding[0] = 0.44;
            t.embedding[5] = (1.0f32 - 0.44 * 0.44).sqrt();
            t.quality = 0.2;
            with(&mut c, t, None);
            c
        };
        let i = tracks.len() - 1;
        let shots: Vec<u32> = tracks.iter().map(|t| t.start).collect();
        let q = Question { tracks: &tracks, shots: &shots, members: &members, now: &now, subjects: &[i], reads: &[] };
        let t = Tuning { lift: 0.0, lift_unsure: 0.0, near_bonus: 0.2, near: 480, ..Tuning::default() };
        assert_eq!(judge_tuned(&q, &params(), Some(&Space::Faces), &t)[0].verdict, Verdict::Maybe(0));
    }

    #[test]
    fn the_scene_doesnt_help_a_sharp_face_that_looks_like_no_one() {
        let mut c = cast();
        let mut t = face(5, 860);
        t.embedding = vec![0f32; DIM];
        t.embedding[0] = 0.25;
        t.embedding[5] = (1.0f32 - 0.25 * 0.25).sqrt();
        let i = with(&mut c, t, None);
        let (tracks, members, now) = &c;
        let shots: Vec<u32> = tracks.iter().map(|t| t.start).collect();
        let q = Question { tracks, shots: &shots, members, now, subjects: &[i], reads: &[] };
        let t = Tuning { lift: 0.0, lift_unsure: 0.0, near_bonus: 0.2, near: 480, ..Tuning::default() };
        assert_eq!(judge_tuned(&q, &params(), Some(&Space::Faces), &t)[0].verdict, Verdict::Nobody);
    }

    #[test]
    fn someone_on_screen_at_the_same_time_gets_no_help_from_the_scene() {
        let mut c = cast();
        let mut t = face(0, 0);
        t.end = 40;
        let i = with(&mut c, t, None);
        let j = ask(&c, &[i], &[]);
        assert_eq!(j[0].verdict, Verdict::Maybe(0));
    }

    #[test]
    fn someone_clearly_seen_twice_at_once_is_put_to_the_person() {
        let mut c = cast();
        // In the same frames as the first character's first face.
        let i = with(&mut c, face(0, 0), None);
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Maybe(0));
    }

    #[test]
    fn a_vague_likeness_to_someone_on_screen_stays_no_one() {
        let mut c = cast();
        let mut t = face(5, 0);
        t.embedding = vec![0f32; DIM];
        t.embedding[0] = 0.32;
        t.embedding[5] = (1.0f32 - 0.32 * 0.32).sqrt();
        let i = with(&mut c, t, None);
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Nobody);
    }

    #[test]
    fn a_face_placed_on_a_character_counts_as_them_on_screen() {
        let mut c = cast();
        // On the first character without defining them, in frames of its own.
        with(&mut c, face(0, 5000), Some(0));
        let i = with(&mut c, face(0, 5000), None);
        // Put to the person at most, never placed.
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Maybe(0));
    }

    #[test]
    fn two_faces_of_one_frame_dont_both_go_to_one_character() {
        let mut c = cast();
        let a = with(&mut c, face(0, 5000), None);
        let b = with(&mut c, face(0, 5000), None);
        let j = ask(&c, &[a, b], &[]);
        assert_eq!(j.iter().filter(|x| x.verdict == Verdict::Newly(0)).count(), 1);
    }

    #[test]
    fn the_users_word_is_at_most_doubted() {
        let mut c = cast();
        let mut t = face(0, 5000);
        t.pin = Some(1);
        let i = with(&mut c, t, Some(1));
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Doubt);
    }

    #[test]
    fn a_person_with_two_looks_finds_themselves_in_either() {
        let (a, b, c) = (unit(0), unit(1), unit(2));
        assert_eq!(likeness(&[a.clone(), b.clone()], &[a.clone(), b.clone()]), 1.0);
        assert!(likeness(&[a.clone(), b], std::slice::from_ref(&a)) >= 0.5);
        assert_eq!(likeness(&[a], &[c]), 0.0);
        assert_eq!(likeness(&[], &[unit(0)]), 0.0);
    }

    #[test]
    fn frames_together_counts_each_frame_once() {
        let mut x = face(0, 100);
        x.seen = vec![100, 103, 106];
        let mut y = face(1, 100);
        y.seen = vec![103, 106, 109];
        let mut z = face(1, 100);
        z.seen = vec![103];
        let tracks = [x, y, z];
        assert_eq!(frames_together(&tracks, &[0], &[1, 2]), 2);
        assert_eq!(frames_together(&tracks, &[1], &[]), 0);
    }

    #[test]
    fn nothing_to_compare_with_is_no_verdict() {
        assert!(ask(&(Vec::new(), Vec::new(), Vec::new()), &[], &[]).is_empty());
    }
}
