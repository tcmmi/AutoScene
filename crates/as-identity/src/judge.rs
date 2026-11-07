//! A second look at faces: who each one is, asked of every character at
//! once. Nothing changes by asking: a verdict is a suggestion, and only the
//! person's answer moves a face.
//!
//! A face is compared with what a character looks like *without* the faces
//! around it. Its own shot and the others dealt to the same fold are held
//! out of every character's table of looks, so a face can't vouch for
//! itself, and neither can the rest of the shot it came with. Far-apart
//! prototypes would otherwise let two stray faces of one wrong group confirm
//! each other, so a character also needs faces from more than one shot that
//! resemble this one.
//!
//! The bars are the cast's own, in the space it was grouped in.

use std::collections::{HashMap, HashSet};

use crate::{CharKey, Params, Space, TrackIn, in_space, overlaps, proto, session_dim};

/// Shots are dealt into this many folds; a face is judged against the rest.
const FOLDS: usize = 5;
/// With fewer faces than this left after holding out, a character can't say
/// a face is theirs.
const MIN_ANCHORS: usize = 3;
/// ... or from fewer shots than this; also how many shots of a character
/// must resemble a face for it to be suggested.
const MIN_SHOTS: usize = 2;

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

/// One character as seen from one fold: what is left of it.
struct Held {
    anchors: Vec<usize>,
    protos: Vec<Vec<f32>>,
    thin: bool,
}

/// Who each of `q.subjects` is. Faces are compared in `space` (`None`:
/// unknown, fitted again as [`crate::identify`] does); nothing is decided.
pub fn judge(q: &Question, p: &Params, space: Option<&Space>) -> Vec<Judged> {
    let Some(dim) = session_dim(q.tracks) else { return Vec::new() };
    let usable = |i: usize| q.tracks.get(i).is_some_and(|t| t.embedding.len() == dim);
    let (faces, p, space) = in_space(q.tracks, p, space);
    let tracks: &[TrackIn] = &faces;

    // Sharper readings, in the same space as everything else.
    let mut read: HashMap<usize, Vec<f32>> = HashMap::new();
    for r in q.reads {
        if usable(r.track) && r.embedding.len() == dim && r.embedding.iter().all(|x| x.is_finite()) {
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

    // The shots in order, dealt in turn: a character's shots spread evenly.
    let shot = |i: usize| (tracks[i].media, q.shots[i]);
    let mut all: Vec<(u32, u32)> =
        chars.iter().flat_map(|(_, g)| g.iter()).chain(q.subjects).map(|&i| shot(i)).collect();
    all.sort_unstable();
    all.dedup();
    let fold_of: HashMap<(u32, u32), usize> = all.iter().enumerate().map(|(n, s)| (*s, n % FOLDS)).collect();

    let folds: HashSet<usize> = q.subjects.iter().filter(|&&i| usable(i)).map(|&i| fold_of[&shot(i)]).collect();
    let table: HashMap<usize, Vec<Held>> = folds
        .into_iter()
        .map(|f| {
            let held = chars
                .iter()
                .map(|(_, g)| {
                    let anchors: Vec<usize> = g.iter().copied().filter(|&i| fold_of[&shot(i)] != f).collect();
                    let looks: Vec<(&[f32], f32)> = anchors
                        .iter()
                        .map(|&i| (tracks[i].embedding.as_slice(), tracks[i].quality.max(0.05)))
                        .collect();
                    let protos = proto::prototypes(&looks, p.prototypes, p.table_dedup_cos);
                    let shots: HashSet<(u32, u32)> = anchors.iter().map(|&i| shot(i)).collect();
                    let thin = anchors.len() < MIN_ANCHORS || shots.len() < MIN_SHOTS;
                    Held { anchors, protos, thin }
                })
                .collect();
            (f, held)
        })
        .collect();

    // Who is on screen when: every face of a character counts.
    type Screen = HashMap<u32, Vec<(usize, Vec<(u32, u32)>)>>;
    let mut screen: Vec<Screen> = vec![HashMap::new(); chars.len()];
    for (ci, (_, g)) in chars.iter().enumerate() {
        for &i in g {
            screen[ci].entry(tracks[i].media).or_default().push((i, tracks[i].presence()));
        }
    }

    let embedding = |i: usize| read.get(&i).map_or(tracks[i].embedding.as_slice(), Vec::as_slice);
    let mut asked: Vec<(usize, Vec<(f32, usize)>)> = q
        .subjects
        .iter()
        .filter(|&&i| usable(i))
        .map(|&i| {
            let held = &table[&fold_of[&shot(i)]];
            let e = embedding(i);
            let mut scores: Vec<(f32, usize)> = (0..chars.len())
                .filter(|&ci| !held[ci].protos.is_empty() && !tracks[i].forbid.contains(&chars[ci].0))
                .map(|ci| (proto::similarity(e, &held[ci].protos), ci))
                .collect();
            scores.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            (i, scores)
        })
        .collect();
    let top = |s: &[(f32, usize)]| s.first().map_or(f32::NEG_INFINITY, |x| x.0);
    asked.sort_by(|a, b| top(&b.1).total_cmp(&top(&a.1)).then(a.0.cmp(&b.0)));

    let key = |ci: usize| chars[ci].0;
    let mut out = Vec::with_capacity(asked.len());
    for (i, scores) in asked {
        let t = &tracks[i];
        let held = &table[&fold_of[&shot(i)]];
        let e = embedding(i);
        let here = t.presence();
        // As in `place`: a character on screen in these frames can't be this
        // face too. Its own spans don't count against it.
        let taken =
            |ci: usize| screen[ci].get(&t.media).is_some_and(|v| v.iter().any(|(j, s)| *j != i && overlaps(&here, s)));
        // Whether more than one shot of a character looks like this face.
        let backed = |ci: usize| {
            let alike = held[ci].anchors.iter().filter(|&&j| proto::dot(e, &tracks[j].embedding) >= p.unsure);
            alike.map(|&j| shot(j)).collect::<HashSet<_>>().len() >= MIN_SHOTS
        };
        let clear =
            |s: f32, ci: usize, next: f32| s >= p.assign && s - next >= p.margin && !held[ci].thin && backed(ci);
        let close = |s: f32, ci: usize| s >= p.unsure && backed(ci);

        let beside = scores.first().is_some_and(|&(_, ci)| taken(ci));
        let free: Vec<(f32, usize)> = scores.iter().copied().filter(|&(_, ci)| !taken(ci)).collect();
        let second = free.get(1).copied();
        let next = second.map_or(-1.0, |x| x.0);
        // The runner-up, when it is as good a candidate as the first.
        let rival = |s1: f32| second.filter(|&(s2, c2)| close(s2, c2) && s1 - s2 < p.margin).map(|x| x.1);
        let own = q.now[i].and_then(|k| index.get(&k).copied());

        let verdict = match (own, free.first().copied()) {
            (Some(_), None) => Verdict::Doubt,
            (None, None) => Verdict::Nobody,
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
                if held[o].thin {
                    if c1 != o && clear(s1, c1, next) { Verdict::Elsewhere(key(c1)) } else { Verdict::Thin }
                } else if c1 == o {
                    match rival(s1) {
                        _ if s1 < p.unsure => Verdict::Doubt,
                        Some(c2) => Verdict::Between(key(o), key(c2)),
                        None => Verdict::Right,
                    }
                } else if clear(s1, c1, next) {
                    Verdict::Elsewhere(key(c1))
                } else if own_score >= p.unsure && close(s1, c1) {
                    Verdict::Between(key(o), key(c1))
                } else if own_score >= p.unsure {
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
        judge(&q, &params(), Some(&Space::Faces))
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
    fn a_character_on_screen_beside_the_face_cant_be_it() {
        let mut c = cast();
        // In the same frames as the first character's first face.
        let i = with(&mut c, face(0, 0), None);
        assert_eq!(ask(&c, &[i], &[])[0].verdict, Verdict::Nobody);
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
    fn nothing_to_compare_with_is_no_verdict() {
        assert!(ask(&(Vec::new(), Vec::new(), Vec::new()), &[], &[]).is_empty());
    }
}
