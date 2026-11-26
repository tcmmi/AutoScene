//! Synthetic identities: random directions in 512-d with noisy tracks
//! around them, the way face embeddings scatter around a person.

use super::*;

const DIM: usize = 512;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32 + 0.5) / (1u64 << 24) as f32
    }
    fn gauss(&mut self) -> f32 {
        let (u, v) = (self.next(), self.next());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }
    fn unit(&mut self) -> Vec<f32> {
        self.unit_d(DIM)
    }
    fn unit_d(&mut self, dim: usize) -> Vec<f32> {
        let mut v: Vec<f32> = (0..dim).map(|_| self.gauss()).collect();
        proto_normalize(&mut v);
        v
    }
}

fn proto_normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter_mut().for_each(|x| *x /= n);
}

/// A track of identity `center` with cosine ≈ `fidelity` to it.
fn around(rng: &mut Rng, center: &[f32], fidelity: f32) -> Vec<f32> {
    let noise = rng.unit_d(center.len());
    let k = (1.0 - fidelity * fidelity).sqrt();
    let mut v: Vec<f32> = center.iter().zip(&noise).map(|(c, n)| fidelity * c + k * n).collect();
    proto_normalize(&mut v);
    v
}

fn track(media: u32, start: u32, embedding: Vec<f32>, quality: f32) -> TrackIn {
    TrackIn {
        media,
        start,
        end: start + 48,
        spans: vec![],
        // The face pass looks at every third frame of the media.
        seen: (start..start + 48).filter(|f| f % 3 == 0).collect(),
        embedding,
        quality,
        dets: 10,
        weight: 1.0,
        pin: None,
        forbid: vec![],
        previous: None,
    }
}

/// `people` identities, `per` core tracks each, on screen one after another.
fn cast(rng: &mut Rng, people: usize, per: usize) -> (Vec<Vec<f32>>, Vec<TrackIn>, Vec<usize>) {
    let centers: Vec<Vec<f32>> = (0..people).map(|_| rng.unit()).collect();
    let mut tracks = Vec::new();
    let mut truth = Vec::new();
    let mut t = 0;
    for _ in 0..per {
        for (p, c) in centers.iter().enumerate() {
            tracks.push(track(0, t, around(rng, c, 0.75), 0.8));
            truth.push(p);
            t += 100;
        }
    }
    (centers, tracks, truth)
}

/// BCubed F-measure of an assignment against the truth (unassigned = own cluster).
fn bcubed(assigned: &[Option<usize>], truth: &[usize]) -> f32 {
    crate::metrics::bcubed(assigned, truth, &vec![1.0; truth.len()], |_| true).f
}

fn chars(o: &Outcome) -> Vec<Option<usize>> {
    o.tracks.iter().map(|(a, _)| a.character()).collect()
}

#[test]
fn separate_people_become_separate_characters() {
    let mut rng = Rng(7);
    let (_, tracks, truth) = cast(&mut rng, 6, 15);
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 6);
    let f = bcubed(&chars(&o), &truth);
    assert!(f > 0.99, "BCubed F {f}");
}

#[test]
fn anime_params_group_wide_embeddings() {
    // CCIP's 768-d vectors under [`Params::anime`]: two well-separated
    // characters, tracks of each one after another, come out as two.
    let mut rng = Rng(23);
    let a = rng.unit_d(768);
    let b = rng.unit_d(768);
    let mut tracks = Vec::new();
    let mut truth = Vec::new();
    for i in 0..15u32 {
        tracks.push(track(0, i * 100, around(&mut rng, &a, 0.85), 0.8));
        tracks.push(track(0, i * 100 + 50, around(&mut rng, &b, 0.85), 0.8));
        truth.extend([0, 1]);
    }
    let o = identify(&tracks, &Params::anime());
    assert_eq!(o.characters.len(), 2);
    let f = bcubed(&chars(&o), &truth);
    assert!(f > 0.99, "BCubed F {f}");
}

#[test]
fn people_on_screen_together_are_never_merged() {
    let mut rng = Rng(11);
    // Two look-alikes (their centres are 0.8 alike — as alike as twins),
    // in the same shots, and now and then one without the other.
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.8);
    let mut tracks = Vec::new();
    let mut truth = Vec::new();
    for i in 0..12u32 {
        tracks.push(track(0, i * 100, around(&mut rng, &a, 0.8), 0.8));
        tracks.push(track(0, i * 100 + 10, around(&mut rng, &b, 0.8), 0.8));
        truth.extend([0, 1]);
    }
    for i in 0..4u32 {
        tracks.push(track(1, i * 100, around(&mut rng, &a, 0.8), 0.8));
        tracks.push(track(2, i * 100, around(&mut rng, &b, 0.8), 0.8));
        truth.extend([0, 1]);
    }
    let o = identify(&tracks, &Params::default());
    let got = chars(&o);
    for i in 0..12 {
        assert_ne!(got[2 * i], got[2 * i + 1], "pair {i} merged");
    }
    assert_eq!(o.characters.len(), 2);
    assert!(bcubed(&got, &truth) > 0.9);
}

/// A person in two looks (lighting, angle), each a group of its own at
/// first: `n` tracks per look, one after another in media `media`. Returns
/// the tracks' indices per look.
fn two_looks(rng: &mut Rng, tracks: &mut Vec<TrackIn>, person: &[f32], media: u32, n: u32) -> [Vec<usize>; 2] {
    let looks = [around(rng, person, 0.9), around(rng, person, 0.9)];
    let mut out = [Vec::new(), Vec::new()];
    for i in 0..n {
        for (l, look) in looks.iter().enumerate() {
            out[l].push(tracks.len());
            tracks.push(track(media, (2 * i + l as u32) * 100, around(rng, look, 0.8), 0.8));
        }
    }
    out
}

/// The character each of `ids` is in (all the same), or a panic.
fn one_character(o: &Outcome, ids: &[usize]) -> usize {
    let c = o.tracks[ids[0]].0.character().expect("a character");
    for &i in ids {
        assert_eq!(o.tracks[i].0.character(), Some(c), "track {i} is elsewhere");
    }
    c
}

#[test]
fn a_mirror_does_not_split_a_person() {
    let mut rng = Rng(41);
    let person = rng.unit();
    let other = rng.unit();
    let mut tracks = Vec::new();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 10);
    for i in 0..10u32 {
        tracks.push(track(0, 5000 + i * 100, around(&mut rng, &other, 0.8), 0.8));
    }
    // In front of a mirror: one face of each look in the same frames, at
    // two places (the person and their reflection) — and someone else
    // beside them.
    let at = tracks[l1[3]].start;
    let others: Vec<usize> = (20..30).collect();
    for i in [l2[3], others[0]] {
        tracks[i].start = at;
        tracks[i].end = at + 48;
        tracks[i].seen = tracks[l1[3]].seen.clone();
    }
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 2, "one character per person");
    let rest: Vec<usize> = l1.iter().chain(&l2).copied().filter(|&i| i != l1[3] && i != l2[3]).collect();
    let c = one_character(&o, &rest);
    assert_ne!(one_character(&o, &others), c);
    // One of the two faces in the mirror shot is theirs; the other can't be
    // (a character is on screen once per frame).
    let mirror = [o.tracks[l1[3]].0.character(), o.tracks[l2[3]].0.character()];
    assert_eq!(mirror.iter().filter(|&&m| m == Some(c)).count(), 1, "{mirror:?}");
    assert!(mirror.iter().all(|m| m.is_none() || *m == Some(c)));
}

#[test]
fn a_face_the_tracker_found_twice_is_one_person() {
    let mut rng = Rng(43);
    let person = rng.unit();
    let mut tracks = Vec::new();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 10);
    // The tracker lost a face of look 1 for a moment and found it again as
    // another track (of look 2) inside the gap: never two faces at once.
    let (s, t) = (l1[5], l2[5]);
    let at = tracks[s].start;
    tracks[s].end = at + 150;
    tracks[s].spans = vec![(at, at + 150)];
    tracks[s].seen = (at..at + 150).filter(|f| f % 3 == 0 && !(at + 60..at + 90).contains(f)).collect();
    tracks[t].start = at + 60;
    tracks[t].end = at + 90;
    tracks[t].seen = (at + 60..at + 90).filter(|f| f % 3 == 0).collect();
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    one_character(&o, &[l1.clone(), l2.clone()].concat());
}

#[test]
fn a_strangers_face_in_a_group_does_not_split_a_person() {
    let mut rng = Rng(47);
    let person = rng.unit();
    let stranger = rng.unit();
    let mut tracks = Vec::new();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 12);
    let strangers: Vec<usize> = (0..10u32)
        .map(|i| {
            tracks.push(track(1, i * 100, around(&mut rng, &stranger, 0.8), 0.8));
            tracks.len() - 1
        })
        .collect();
    // A blurry face of the stranger that resembles look 1 enough to join
    // it, on screen with a face of look 2.
    let mut mixed: Vec<f32> = stranger.iter().zip(&tracks[l1[0]].embedding).map(|(s, p)| 0.55 * s + 0.85 * p).collect();
    proto_normalize(&mut mixed);
    let stray_at = tracks.len();
    tracks.push(track(0, tracks[l2[7]].start, mixed, 0.5));
    let o = identify(&tracks, &Params::default());
    let c = one_character(&o, &[l1.clone(), l2.clone()].concat());
    assert_ne!(o.tracks[stray_at].0.character(), Some(c), "a face seen with them is not them");
    one_character(&o, &strangers);
    assert_eq!(o.characters.len(), 2);
}

#[test]
fn a_reflection_seen_only_with_its_person_is_no_character() {
    let mut rng = Rng(53);
    let person = rng.unit();
    let other = rng.unit();
    let mut tracks: Vec<TrackIn> = (0..12u32).map(|i| track(0, i * 100, around(&mut rng, &person, 0.8), 0.8)).collect();
    for i in 0..8u32 {
        tracks.push(track(0, 5000 + i * 100, around(&mut rng, &other, 0.8), 0.8));
    }
    // A long scene in front of a mirror: shot after shot their reflection,
    // alike to each other (the same mirror, the same light), with them.
    let mirror = around(&mut rng, &person, 0.85);
    let reflections: Vec<usize> = (0..5)
        .map(|i| {
            let mut t = track(0, tracks[i].start, around(&mut rng, &mirror, 0.9), 0.8);
            t.seen = tracks[i].seen.clone();
            tracks.push(t);
            tracks.len() - 1
        })
        .collect();
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 2, "no character for the reflection");
    one_character(&o, &(0..12).collect::<Vec<_>>());
    // Nobody's, not even a guess: it is most like someone on screen with it.
    assert!(reflections.iter().all(|&i| o.tracks[i].0 == Assign::None), "{:?}", o.tracks);
}

/// `n` tracks of `person`, each seen in the same frames as a track of
/// `beside` (another person) when `with(i)`, else on its own.
fn seen_with(
    rng: &mut Rng,
    tracks: &mut Vec<TrackIn>,
    person: &[f32],
    beside: &[usize],
    with: impl Fn(usize) -> bool,
) -> Vec<usize> {
    (0..beside.len())
        .map(|i| {
            let at = tracks[beside[i]].start;
            let mut t =
                track(tracks[beside[i]].media, if with(i) { at } else { at + 200_000 }, around(rng, person, 0.8), 0.8);
            if with(i) {
                t.seen = tracks[beside[i]].seen.clone();
            }
            tracks.push(t);
            tracks.len() - 1
        })
        .collect()
}

#[test]
fn look_alikes_always_on_screen_together_stay_two() {
    let mut rng = Rng(59);
    // Siblings who look alike (mean faces 0.7 alike), the younger one only
    // ever seen with the older: two faces at every moment, never one face
    // seen twice.
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.7);
    let mut tracks: Vec<TrackIn> = (0..14u32).map(|i| track(0, i * 100, around(&mut rng, &a, 0.8), 0.8)).collect();
    let older: Vec<usize> = (0..14).collect();
    let younger = seen_with(&mut rng, &mut tracks, &b, &older[..8], |_| true);
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 2);
    let (ca, cb) = (one_character(&o, &older), one_character(&o, &younger));
    assert_ne!(ca, cb);
}

#[test]
fn a_merge_despite_faces_seen_together_needs_clearly_one_person() {
    let mut rng = Rng(61);
    // Two people half alike (mean faces 0.5) — their faces alike enough
    // to be grouped (average 0.36) — seen together once.
    let p = rng.unit();
    let q = around(&mut rng, &p, 0.5);
    let mut tracks: Vec<TrackIn> = (0..10u32).map(|i| track(0, i * 100, around(&mut rng, &p, 0.85), 0.8)).collect();
    let first: Vec<usize> = (0..10).collect();
    let second = seen_with(&mut rng, &mut tracks, &q, &first, |i| i == 4);
    for &i in &second {
        tracks[i].embedding = around(&mut rng, &q, 0.85);
    }
    let o = identify(&tracks, &Params::default());
    assert_ne!(one_character(&o, &first[..4]), one_character(&o, &second[..4]), "two people");
}

#[test]
fn a_pinned_face_never_steps_aside() {
    let mut rng = Rng(67);
    let person = rng.unit();
    let mut tracks = Vec::new();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 10);
    // The user named look 1. In front of a mirror, a face of look 1 that is
    // the least typical of all, and one of look 2, at the same moment.
    for &i in &l1 {
        tracks[i].pin = Some(7);
    }
    tracks[l1[3]].embedding = around(&mut rng, &person, 0.6);
    let at = tracks[l1[3]].start;
    tracks[l2[3]].start = at;
    tracks[l2[3]].end = at + 48;
    tracks[l2[3]].seen = tracks[l1[3]].seen.clone();
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    assert_eq!(o.characters[0].key, Some(7));
    assert!(l1.iter().all(|&i| o.tracks[i].0 == Assign::Pinned(0)));
    let rest: Vec<usize> = l2.iter().copied().filter(|&i| i != l2[3]).collect();
    one_character(&o, &rest);
    assert_ne!(o.tracks[l2[3]].0.character(), Some(0));
}

#[test]
fn a_pinned_copy_stays_a_character() {
    let mut rng = Rng(71);
    let person = rng.unit();
    let mut tracks: Vec<TrackIn> = (0..12u32).map(|i| track(0, i * 100, around(&mut rng, &person, 0.8), 0.8)).collect();
    // Their face on a screen, shot after shot with them — and the user made
    // it a character of its own.
    let screen = around(&mut rng, &person, 0.75);
    let copies: Vec<usize> = (0..5)
        .map(|i| {
            let mut t = track(0, tracks[i].start, around(&mut rng, &screen, 0.9), 0.8);
            t.seen = tracks[i].seen.clone();
            t.pin = Some(3);
            tracks.push(t);
            tracks.len() - 1
        })
        .collect();
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 2);
    let c = one_character(&o, &copies);
    assert_eq!(o.characters[c].key, Some(3));
}

#[test]
fn a_face_most_like_someone_beside_it_is_no_guess_for_anyone_else() {
    let mut rng = Rng(73);
    let (_, mut tracks, truth) = cast(&mut rng, 2, 10);
    let p0 = (0..tracks.len()).find(|&i| truth[i] == 0).unwrap();
    // A weak face, most like person 0 but on screen with them, and somewhat
    // like person 1 — a guess for person 1 if it were alone.
    let (c0, c1) =
        (tracks[p0].embedding.clone(), tracks[(0..tracks.len()).find(|&i| truth[i] == 1).unwrap()].embedding.clone());
    let mut e: Vec<f32> = c0.iter().zip(&c1).map(|(x, y)| 0.75 * x + 0.45 * y).collect();
    proto_normalize(&mut e);
    let mut weak = track(0, tracks[p0].start, e.clone(), 0.2);
    weak.seen = tracks[p0].seen.clone();
    let alone = track(5, 0, e, 0.2);
    tracks.extend([weak, alone]);
    let o = identify(&tracks, &Params::default());
    let n = tracks.len();
    assert_eq!(o.tracks[n - 2].0, Assign::None, "beside person 0");
    assert_eq!(o.tracks[n - 1].0.character(), o.tracks[p0].0.character(), "alone, it is person 0");
}

#[test]
fn twins_stay_two_and_a_mirror_splits_no_one_in_a_big_series() {
    // More clear tracks than one clustering level takes, so each episode is
    // grouped on its own first: twins (mean faces 0.75 alike), six tracks of
    // one and three of the other per episode, one of those three on screen
    // with the other twin — and a person with a mirror in one episode.
    let mut rng = Rng(79);
    let mut tracks: Vec<TrackIn> = (0..4100u32).map(|i| track(100 + i % 8, (i / 8) * 100, rng.unit(), 0.9)).collect();
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.75);
    let (mut twin_a, mut twin_b) = (Vec::new(), Vec::new());
    for m in 0..20u32 {
        let first = tracks.len();
        for k in 0..6u32 {
            tracks.push(track(m, 1_000_000 + k * 100, around(&mut rng, &a, 0.8), 0.8));
        }
        twin_a.extend(first..tracks.len());
        let theirs: Vec<usize> = (first..first + 3).collect();
        twin_b.extend(seen_with(&mut rng, &mut tracks, &b, &theirs, |i| i == 0));
    }
    let person = rng.unit();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 50, 10);
    let at = tracks[l1[3]].start;
    tracks[l2[3]].start = at;
    tracks[l2[3]].end = at + 48;
    tracks[l2[3]].seen = tracks[l1[3]].seen.clone();
    let o = identify(&tracks, &Params::default());
    assert_ne!(one_character(&o, &twin_a), one_character(&o, &twin_b), "the twins are two");
    let rest: Vec<usize> = l1.iter().chain(&l2).copied().filter(|&i| i != l1[3] && i != l2[3]).collect();
    one_character(&o, &rest);
}

/// Twins A and B are two characters: B's faces stay off A's (but a stray
/// blurry one that looks more like A than like B), and most of them are
/// one character of their own.
fn twins_are_two(o: &Outcome, a: &[usize], b: &[usize], what: &str) {
    let ca = one_character(o, a);
    let on_a = b.iter().filter(|&&i| o.tracks[i].0.character() == Some(ca)).count();
    assert!(10 * on_a <= b.len(), "{what}: {on_a} of B's {} faces on A", b.len());
    let mut theirs: HashMap<usize, usize> = HashMap::new();
    for &i in b {
        if let Some(c) = o.tracks[i].0.character().filter(|&c| c != ca) {
            *theirs.entry(c).or_default() += 1;
        }
    }
    let most = theirs.values().max().copied().unwrap_or(0);
    assert!(2 * most >= b.len(), "{what}: B's own character holds {most} of {}", b.len());
}

#[test]
fn twins_stay_two_when_they_are_seen_more_clearly_together() {
    // Two-shots in good light, the rest of their shots in any light: twins
    // whose faces side by side are clearer than their faces elsewhere.
    for (seed, (centres, alone, together)) in [(0.75f32, 0.6f32, 8usize), (0.8, 0.65, 6)].into_iter().enumerate() {
        let mut rng = Rng(109 + seed as u64);
        let a = rng.unit();
        let b = around(&mut rng, &a, centres);
        let mut tracks: Vec<TrackIn> = (0..20u32).map(|i| track(0, i * 100, around(&mut rng, &a, 0.9), 0.8)).collect();
        let first: Vec<usize> = (0..20).collect();
        let second = seen_with(&mut rng, &mut tracks, &b, &first, |i| i < together);
        for (k, &i) in second.iter().enumerate() {
            tracks[i].embedding = around(&mut rng, &b, if k < together { 0.9 } else { alone });
        }
        let o = identify(&tracks, &Params::default());
        twins_are_two(&o, &first, &second, &format!("twins {seed}"));
    }
}

#[test]
fn twins_on_screen_together_now_and_then_stay_two() {
    // In 4 of the 20 shots of the second twin, the first is there too: too
    // often for one person seen twice.
    let mut rng = Rng(113);
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.75);
    let mut tracks: Vec<TrackIn> = (0..20u32).map(|i| track(0, i * 100, around(&mut rng, &a, 0.8), 0.8)).collect();
    let first: Vec<usize> = (0..20).collect();
    let second = seen_with(&mut rng, &mut tracks, &b, &first, |i| i < 4);
    let o = identify(&tracks, &Params::default());
    twins_are_two(&o, &first, &second, "twins");
}

#[test]
fn a_person_seen_twice_twice_is_still_one() {
    // Two moments in front of a mirror, between the same two looks.
    let mut rng = Rng(127);
    let person = rng.unit();
    let mut tracks = Vec::new();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 20);
    for k in [3, 11] {
        let at = tracks[l1[k]].start;
        tracks[l2[k]].start = at;
        tracks[l2[k]].end = at + 48;
        tracks[l2[k]].seen = tracks[l1[k]].seen.clone();
    }
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    let rest: Vec<usize> =
        l1.iter().chain(&l2).copied().filter(|&i| ![l1[3], l1[11], l2[3], l2[11]].contains(&i)).collect();
    one_character(&o, &rest);
}

#[test]
fn twins_seen_more_clearly_together_stay_two_in_a_big_series() {
    // Each episode on its own first: two clear shots of the second twin
    // beside the first, four blurrier ones alone.
    let mut rng = Rng(131);
    let mut tracks: Vec<TrackIn> = (0..4100u32).map(|i| track(100 + i % 8, (i / 8) * 100, rng.unit(), 0.9)).collect();
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.75);
    let (mut twin_a, mut twin_b) = (Vec::new(), Vec::new());
    for m in 0..20u32 {
        let first = tracks.len();
        for k in 0..6u32 {
            tracks.push(track(m, 1_000_000 + k * 100, around(&mut rng, &a, 0.8), 0.8));
        }
        twin_a.extend(first..tracks.len());
        let theirs: Vec<usize> = (first..first + 6).collect();
        let second = seen_with(&mut rng, &mut tracks, &b, &theirs, |i| i < 2);
        for (k, &i) in second.iter().enumerate() {
            tracks[i].embedding = around(&mut rng, &b, if k < 2 { 0.9 } else { 0.65 });
        }
        twin_b.extend(second);
    }
    let o = identify(&tracks, &Params::default());
    twins_are_two(&o, &twin_a, &twin_b, "twins in a series");
}

#[test]
fn pins_and_forbids_win() {
    let mut rng = Rng(3);
    let (_, mut tracks, truth) = cast(&mut rng, 3, 10);
    // The user named person 0 (key 40) by pinning one of its tracks, and
    // said one of person 0's tracks is not them.
    let p0: Vec<usize> = (0..tracks.len()).filter(|&i| truth[i] == 0).collect();
    tracks[p0[0]].pin = Some(40);
    tracks[p0[1]].forbid = vec![40];
    // And moved a track of person 1 to person 2's character (key 41).
    let p1 = (0..tracks.len()).find(|&i| truth[i] == 1).unwrap();
    let p2 = (0..tracks.len()).find(|&i| truth[i] == 2).unwrap();
    tracks[p1].pin = Some(41);
    tracks[p2].pin = Some(41);
    let o = identify(&tracks, &Params::default());
    let c40 = o.characters.iter().position(|c| c.key == Some(40)).unwrap();
    let c41 = o.characters.iter().position(|c| c.key == Some(41)).unwrap();
    assert_eq!(o.tracks[p0[0]].0, Assign::Pinned(c40));
    assert_ne!(o.tracks[p0[1]].0.character(), Some(c40));
    assert_eq!(o.tracks[p0[2]].0, Assign::Auto(c40));
    assert_eq!(o.tracks[p1].0, Assign::Pinned(c41));
    assert_eq!(o.tracks[p2].0, Assign::Pinned(c41));
}

#[test]
fn characters_keep_their_identity_when_episodes_are_added() {
    let mut rng = Rng(5);
    let (centers, mut tracks, _) = cast(&mut rng, 4, 10);
    let first = identify(&tracks, &Params::default());
    // Remember: keys 100.. in the order of the first result.
    for (t, (a, _)) in tracks.iter_mut().zip(&first.tracks) {
        t.previous = a.character().map(|c| 100 + c as u32);
    }
    // A second episode, people in another order.
    for i in 0..10u32 {
        for (p, c) in centers.iter().enumerate().rev() {
            tracks.push(track(1, i * 100 + p as u32, around(&mut rng, c, 0.75), 0.8));
        }
    }
    let second = identify(&tracks, &Params::default());
    for (i, (a, _)) in first.tracks.iter().enumerate() {
        let before = 100 + a.character().unwrap() as u32;
        let after = second.characters[second.tracks[i].0.character().unwrap()].key;
        assert_eq!(after, Some(before));
    }
    // The new episode's tracks joined them.
    assert!(second.tracks[40..].iter().all(|(a, _)| matches!(a, Assign::Auto(_))));
}

#[test]
fn weak_tracks_join_clearly_or_wait() {
    let mut rng = Rng(9);
    let (centers, mut tracks, _) = cast(&mut rng, 3, 10);
    let core = tracks.len();
    // Low quality (not core): close to person 0; between persons 1 and 2; nobody.
    tracks.push(track(0, 90_000, around(&mut rng, &centers[0], 0.6), 0.2));
    let mid: Vec<f32> = centers[1].iter().zip(&centers[2]).map(|(a, b)| a + b).collect();
    let mut mid = around(&mut rng, &mid, 0.9);
    proto_normalize(&mut mid);
    tracks.push(track(0, 91_000, mid, 0.2));
    tracks.push(track(0, 92_000, rng.unit(), 0.2));
    let o = identify(&tracks, &Params::default());
    let person_char = |p: usize| o.tracks[p].0.character().unwrap();
    assert_eq!(o.tracks[core].0, Assign::Auto(person_char(0)));
    assert!(matches!(o.tracks[core + 1].0, Assign::Unsure(c) if c == person_char(1) || c == person_char(2)));
    assert_eq!(o.tracks[core + 2].0, Assign::None);
}

#[test]
fn groups_of_vague_faces_are_not_characters() {
    let mut rng = Rng(17);
    let (_, mut tracks, _) = cast(&mut rng, 2, 10);
    // Masked or blurry faces: alike only through what they lack (≈ 0.4 to a common direction).
    let blur = rng.unit();
    for i in 0..10u32 {
        tracks.push(track(1, i * 100, around(&mut rng, &blur, 0.62), 0.6));
    }
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 2);
    assert!(o.tracks[20..].iter().all(|(a, _)| a.character().is_none()));
}

#[test]
fn a_face_seen_with_a_character_is_not_them() {
    let mut rng = Rng(13);
    let (centers, mut tracks, _) = cast(&mut rng, 2, 10);
    // A weak track of person 0, on screen at the same time as a person-0 core track.
    let at = tracks[0].start + 5;
    tracks.push(track(0, at, around(&mut rng, &centers[0], 0.7), 0.2));
    let o = identify(&tracks, &Params::default());
    let c0 = o.tracks[0].0.character().unwrap();
    assert_ne!(o.tracks.last().unwrap().0.character(), Some(c0));
}

#[test]
fn a_face_back_after_a_gap_leaves_the_gap_free() {
    let mut rng = Rng(19);
    let (centers, mut tracks, _) = cast(&mut rng, 2, 10);
    // Person 0 left and came back: one track, on screen 0..100 and 201..330.
    let mut long = track(1, 0, around(&mut rng, &centers[0], 0.75), 0.8);
    (long.end, long.spans) = (330, vec![(0, 100), (201, 330)]);
    tracks.push(long);
    // A weak view of them in between.
    tracks.push(track(1, 110, around(&mut rng, &centers[0], 0.7), 0.2));
    let o = identify(&tracks, &Params::default());
    let c0 = o.tracks[0].0.character().unwrap();
    assert_eq!(o.tracks.last().unwrap().0, Assign::Auto(c0));
}

#[test]
fn a_character_is_on_screen_once_per_frame() {
    let mut rng = Rng(29);
    let (centers, mut tracks, _) = cast(&mut rng, 2, 10);
    // Two weak faces like person 0 at the same time: only the closer one is them.
    let n = tracks.len();
    tracks.push(track(1, 0, around(&mut rng, &centers[0], 0.7), 0.2));
    tracks.push(track(1, 10, around(&mut rng, &centers[0], 0.85), 0.2));
    let o = identify(&tracks, &Params::default());
    let c0 = o.tracks[0].0.character().unwrap();
    assert_eq!(o.tracks[n + 1].0, Assign::Auto(c0));
    assert_ne!(o.tracks[n].0.character(), Some(c0));
}

#[test]
fn characters_found_to_be_one_say_so() {
    let mut rng = Rng(83);
    let person = rng.unit();
    let other = rng.unit();
    let mut tracks = Vec::new();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 10);
    for k in 0..6u32 {
        tracks.push(track(1, k * 100, around(&mut rng, &other, 0.8), 0.8));
    }
    // An older grouping made the two looks two characters (keys 100 and
    // 101, the second bigger now), and the other person 102.
    for &i in &l1 {
        tracks[i].previous = Some(100);
    }
    for &i in &l2 {
        tracks[i].previous = Some(101);
        tracks[i].quality = 0.9;
    }
    for t in &mut tracks[20..] {
        t.previous = Some(102);
    }
    let o = identify(&tracks, &Params::default());
    let c = one_character(&o, &[l1.clone(), l2.clone()].concat());
    assert_eq!(o.characters[c].key, Some(101));
    assert_eq!(o.absorbed, [(100, c)], "100 is one with 101 now");
    // Nothing joined: nothing to say.
    let again = identify(&tracks[20..], &Params::default());
    assert!(again.absorbed.is_empty());
    // Placing faces after a decision groups nothing either, whatever
    // characters lost their faces.
    let members = vec![(101, [l1.clone(), l2.clone()].concat()), (102, (20..26).collect())];
    assert!(reassign(&tracks, &members, &Params::default(), None).absorbed.is_empty());
}

#[test]
fn a_reflection_that_was_a_character_is_one_with_its_person() {
    let mut rng = Rng(89);
    let person = rng.unit();
    let mut tracks: Vec<TrackIn> = (0..12u32).map(|i| track(0, i * 100, around(&mut rng, &person, 0.8), 0.8)).collect();
    let mirror = around(&mut rng, &person, 0.85);
    for i in 0..5 {
        let mut t = track(0, tracks[i].start, around(&mut rng, &mirror, 0.9), 0.8);
        t.seen = tracks[i].seen.clone();
        t.previous = Some(9);
        tracks.push(t);
    }
    for t in &mut tracks[..12] {
        t.previous = Some(8);
    }
    let o = identify(&tracks, &Params::default());
    let c = one_character(&o, &(0..12).collect::<Vec<_>>());
    assert_eq!(o.characters[c].key, Some(8));
    assert_eq!(o.absorbed, [(9, c)], "its scenepack goes to them");
}

#[test]
fn small_groups_of_one_person_merge_despite_one_mirror() {
    let mut rng = Rng(97);
    let person = rng.unit();
    let mut tracks = Vec::new();
    // Six faces per look: a tenth of the smaller group is less than one face,
    // yet one face may always step aside.
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 6);
    let at = tracks[l1[2]].start;
    tracks[l2[2]].start = at;
    tracks[l2[2]].end = at + 48;
    tracks[l2[2]].seen = tracks[l1[2]].seen.clone();
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    let rest: Vec<usize> = l1.iter().chain(&l2).copied().filter(|&i| i != l1[2] && i != l2[2]).collect();
    one_character(&o, &rest);
}

#[test]
fn look_alikes_stay_two_when_one_of_them_is_seen_less_clearly() {
    let mut rng = Rng(103);
    // One actor in two roles (mean faces 0.85 alike): one role always clear
    // (its faces 0.81 alike), the other often blurry (0.51); the blurry one
    // on screen with the clear one in 12 of its 20 shots.
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.85);
    let mut tracks: Vec<TrackIn> = (0..20u32).map(|i| track(0, i * 100, around(&mut rng, &a, 0.9), 0.8)).collect();
    let clear: Vec<usize> = (0..20).collect();
    let blurry = seen_with(&mut rng, &mut tracks, &b, &clear, |i| i < 12);
    for &i in &blurry {
        tracks[i].embedding = around(&mut rng, &b, 0.72);
    }
    let o = identify(&tracks, &Params::default());
    let ca = one_character(&o, &clear);
    let theirs = blurry.iter().filter(|&&i| o.tracks[i].0.character().is_some_and(|c| c != ca)).count();
    assert!(theirs >= 15, "the second role keeps its faces: {theirs} of 20");
}

#[test]
fn a_noisy_group_does_not_make_look_alikes_copies() {
    let mut rng = Rng(107);
    // Look-alikes (mean faces 0.85): one seen clearly (faces 0.81 alike),
    // the other blurry (0.51), on screen with the first in 12 of its 13
    // shots. Their faces side by side are as alike as the blurry one's
    // faces are to each other — but far less than the clear one's: two
    // people, whose groups must not merge.
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.85);
    let mut tracks: Vec<TrackIn> = (0..20u32).map(|i| track(0, i * 100, around(&mut rng, &a, 0.9), 0.8)).collect();
    let clear: Vec<usize> = (0..20).collect();
    let blurry = seen_with(&mut rng, &mut tracks, &b, &clear[..13], |i| i < 12);
    for &i in &blurry {
        tracks[i].embedding = around(&mut rng, &b, 0.714);
    }
    let core: Vec<usize> = (0..tracks.len()).collect();
    let groups = cluster(&tracks, &core, &Params::default());
    let group_of = |t: usize| groups.iter().position(|g| g.contains(&t)).unwrap();
    let theirs = group_of(blurry[0]);
    assert!(blurry[..12].iter().all(|&t| group_of(t) == theirs), "their faces seen with the other stay together");
    assert_ne!(theirs, group_of(clear[0]));
    assert!(groups[theirs].len() >= 12);
}

#[test]
fn characters_keep_their_identity_when_they_grow() {
    let mut rng = Rng(31);
    let (centers, mut tracks, _) = cast(&mut rng, 2, 5);
    let first = identify(&tracks, &Params::default());
    for (t, (a, _)) in tracks.iter_mut().zip(&first.tracks) {
        t.previous = a.character().map(|c| 100 + c as u32);
    }
    let key0 = 100 + first.tracks[0].0.character().unwrap() as u32;
    // A small part in episode 1, a big one in episode 2.
    for i in 0..30u32 {
        tracks.push(track(1, i * 100, around(&mut rng, &centers[0], 0.75), 0.8));
    }
    let second = identify(&tracks, &Params::default());
    let c = second.tracks[0].0.character().unwrap();
    assert_eq!(second.characters[c].key, Some(key0));
    assert!(second.tracks[10..].iter().all(|(a, _)| a.character() == Some(c)));
}

#[test]
fn a_person_seen_once_per_episode_is_found_in_a_big_series() {
    // More clear tracks than one clustering level takes: a person with one
    // track per episode is a single track in each episode's first level.
    // Strangers are random directions in the embedding's 512 dimensions
    // (in fewer, chance alone makes them look alike).
    let mut rng = Rng(37);
    let person = rng.unit();
    let mut tracks: Vec<TrackIn> = (0..4100u32).map(|i| track(i % 8, (i / 8) * 100, rng.unit(), 0.9)).collect();
    let first = tracks.len();
    for m in 0..8u32 {
        let noise = rng.unit();
        let mut e: Vec<f32> = person.iter().zip(&noise).map(|(c, n)| 0.9 * c + 0.436 * n).collect();
        proto_normalize(&mut e);
        tracks.push(track(m, 1_000_000, e, 0.6));
    }
    let o = identify(&tracks, &Params::default());
    let c = o.tracks[first].0.character();
    assert!(c.is_some(), "the person is no character");
    assert!(o.tracks[first..].iter().all(|(a, _)| a.character() == c));
}

/// The groups of `tracks` (all of them clear), clustered per media from
/// `single` groups on and in runs of at most `second` groups after that,
/// and the most groups a run took.
fn cluster_capped(tracks: &[TrackIn], single: usize, second: usize) -> (Vec<Vec<usize>>, usize) {
    let core: Vec<usize> = (0..tracks.len()).collect();
    crate::ahc::BIGGEST_RUN.with(|b| b.set(0));
    let groups = crate::ahc::cluster_in(tracks, &core, &Params::default(), crate::ahc::Levels { single, second });
    (groups, crate::ahc::BIGGEST_RUN.with(|b| b.get()))
}

#[test]
fn a_media_longer_than_one_run_takes_is_grouped_in_stretches_of_time() {
    // Six people one after another in one media, 30 tracks each, at most 60
    // groups a run: three stretches of 60 tracks, each with 10 of everyone's,
    // whose groups then meet as episodes do.
    let mut rng = Rng(139);
    let (_, tracks, truth) = cast(&mut rng, 6, 30);
    let (groups, biggest) = cluster_capped(&tracks, 40, 60);
    assert!(biggest <= 60, "a run of {biggest} groups");
    let people: Vec<Vec<usize>> = (0..6).map(|p| (0..tracks.len()).filter(|&i| truth[i] == p).collect()).collect();
    let mut found = groups.clone();
    found.sort();
    assert_eq!(found, people, "everyone is one group");
    // Just as one run would group them.
    let (whole, biggest) = cluster_capped(&tracks, 40, usize::MAX);
    assert_eq!(biggest, tracks.len());
    assert_eq!(whole, groups);
}

#[test]
fn faces_seen_together_on_either_side_of_a_cut_stay_two() {
    // Twins (mean faces 0.8 alike) on screen together in 90 shots of one
    // media, after somebody else's face: at most 60 groups a run makes
    // stretches of 46 tracks, and the first cut falls between the twins of
    // shot 22.
    let mut rng = Rng(149);
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.8);
    let mut tracks = vec![track(0, 0, rng.unit(), 0.8)];
    let (mut twin_a, mut twin_b) = (Vec::new(), Vec::new());
    for i in 0..90u32 {
        twin_a.push(tracks.len());
        tracks.push(track(0, i * 100 + 50, around(&mut rng, &a, 0.8), 0.8));
        twin_b.push(tracks.len());
        tracks.push(track(0, i * 100 + 60, around(&mut rng, &b, 0.8), 0.8));
    }
    let (groups, biggest) = cluster_capped(&tracks, 40, 60);
    assert!(biggest <= 60, "a run of {biggest} groups");
    let group_of = |t: usize| groups.iter().position(|g| g.contains(&t)).unwrap();
    assert_ne!(group_of(twin_a[22]), group_of(twin_b[22]), "the twins of the shot that was cut");
    for g in &groups {
        for (x, &i) in g.iter().enumerate() {
            for &j in &g[x + 1..] {
                assert!(!together(&tracks[i], &tracks[j]), "{i} and {j} are on screen at once");
            }
        }
    }
    assert!(twin_a.iter().all(|&t| group_of(t) == group_of(twin_a[0])), "the first twin is one group");
    assert!(twin_b.iter().all(|&t| group_of(t) == group_of(twin_b[0])), "the second twin is one group");
}

#[test]
fn a_media_too_long_for_one_run_is_grouped_within_the_bound() {
    // More clear tracks in one media than a run takes (a season in one
    // file, a long recording): strangers one after another, and now and
    // then a person, in two stretches of time.
    let mut rng = Rng(151);
    let person = rng.unit();
    let mut tracks = Vec::new();
    let mut theirs = Vec::new();
    for i in 0..6100u32 {
        if i % 200 == 0 {
            theirs.push(tracks.len());
            tracks.push(track(0, i * 100, around(&mut rng, &person, 0.75), 0.8));
        } else {
            tracks.push(track(0, i * 100, rng.unit(), 0.9));
        }
    }
    let core: Vec<usize> = (0..tracks.len()).collect();
    crate::ahc::BIGGEST_RUN.with(|b| b.set(0));
    let groups = cluster(&tracks, &core, &Params::default());
    let biggest = crate::ahc::BIGGEST_RUN.with(|b| b.get());
    assert!(biggest <= 6000, "a run of {biggest} groups");
    let mut theirs_found = groups[0].clone();
    theirs_found.sort_unstable();
    assert_eq!(theirs_found, theirs, "the person is one group");
}

/// What identify made, as `reassign` takes it: each character's members.
fn members_of(o: &Outcome) -> Vec<(CharKey, Vec<usize>)> {
    (0..o.characters.len())
        .map(|ci| {
            let g = (0..o.tracks.len())
                .filter(|&i| matches!(o.tracks[i], (Assign::Pinned(c), _) | (Assign::Auto(c), 1.0) if c == ci))
                .collect();
            (o.characters[ci].key.unwrap_or(ci as CharKey), g)
        })
        .collect()
}

#[test]
fn reassigning_keeps_the_cast_and_places_everyone_else() {
    let mut rng = Rng(21);
    let (centers, mut tracks, truth) = cast(&mut rng, 4, 10);
    // Weak faces of each person, and a stranger seen often (a group of its
    // own, which only clustering may turn into a character).
    let mut weak = Vec::new();
    for (p, c) in centers.iter().enumerate() {
        for k in 0..3u32 {
            weak.push((tracks.len(), p));
            tracks.push(track(1, 10_000 + (p as u32 * 3 + k) * 100, around(&mut rng, c, 0.75), 0.2));
        }
    }
    let stranger = rng.unit();
    let strangers: Vec<usize> = (0..6u32)
        .map(|k| {
            tracks.push(track(2, k * 100, around(&mut rng, &stranger, 0.8), 0.2));
            tracks.len() - 1
        })
        .collect();
    let o = identify(&tracks[..truth.len()], &Params::default());
    let members = members_of(&o);
    let r = reassign(&tracks, &members, &Params::default(), None);

    assert_eq!(r.characters.len(), 4, "no character made");
    for (ci, (key, g)) in members.iter().enumerate() {
        assert_eq!(r.characters[ci].key, Some(*key));
        assert!(g.iter().all(|&i| r.tracks[i] == (Assign::Auto(ci), 1.0)), "members stay");
    }
    // Each weak face joins its person (characters keep their order).
    let person = |p: usize| o.tracks[truth.iter().position(|&t| t == p).unwrap()].0.character().unwrap();
    for &(i, p) in &weak {
        assert_eq!(r.tracks[i].0, Assign::Auto(person(p)), "weak face {i}");
    }
    assert!(strangers.iter().all(|&i| r.tracks[i].0 == Assign::None));
}

#[test]
fn not_them_on_a_member_takes_its_look_alikes_along() {
    let mut rng = Rng(23);
    let a = rng.unit();
    let b = rng.unit();
    // Character 7 is person a — and, by mistake, one face of person b.
    let mut tracks: Vec<TrackIn> = (0..6u32).map(|k| track(0, k * 100, around(&mut rng, &a, 0.8), 0.9)).collect();
    tracks.push(track(0, 600, around(&mut rng, &b, 0.9), 0.9));
    let wrong = tracks.len() - 1;
    // More faces of person b, nobody's yet.
    let alikes: Vec<usize> = (0..4u32)
        .map(|k| {
            tracks.push(track(1, k * 100, around(&mut rng, &b, 0.9), 0.3));
            tracks.len() - 1
        })
        .collect();
    let members = vec![(7, (0..=wrong).collect::<Vec<_>>())];
    let p = Params::default();
    let before = reassign(&tracks, &members, &p, None);
    assert!(alikes.iter().all(|&i| before.tracks[i].0 == Assign::Auto(0)), "they follow the wrong face");

    tracks[wrong].forbid = vec![7];
    let after = reassign(&tracks, &members, &p, None);
    assert_eq!(after.tracks[wrong].0, Assign::None);
    assert!(alikes.iter().all(|&i| after.tracks[i].0.character().is_none()), "look-alikes leave with it");
    assert!((0..wrong).all(|i| after.tracks[i] == (Assign::Auto(0), 1.0)));
}

#[test]
fn reassigning_never_moves_a_pin_or_breaks_a_forbid() {
    let mut rng = Rng(29);
    let (_, mut tracks, truth) = cast(&mut rng, 3, 8);
    let o = identify(&tracks, &Params::default());
    let members = members_of(&o);
    // A face of person 0 the user put on person 1's character, and one they
    // said isn't person 0's character.
    let p0: Vec<usize> = (0..tracks.len()).filter(|&i| truth[i] == 0).collect();
    let c0 = o.tracks[p0[2]].0.character().unwrap();
    let c1 = o.tracks[(0..tracks.len()).find(|&i| truth[i] == 1).unwrap()].0.character().unwrap();
    let (k0, k1) = (members[c0].0, members[c1].0);
    tracks[p0[0]].pin = Some(k1);
    tracks[p0[1]].forbid = vec![k0];
    let mut members = members.clone();
    for (key, g) in &mut members {
        g.retain(|&i| i != p0[0] && i != p0[1]);
        if *key == k1 {
            g.push(p0[0]);
        }
    }
    let r = reassign(&tracks, &members, &Params::default(), None);
    let at = |k: CharKey| r.characters.iter().position(|c| c.key == Some(k)).unwrap();
    assert_eq!(r.tracks[p0[0]].0, Assign::Pinned(at(k1)));
    assert_ne!(r.tracks[p0[1]].0.character(), Some(at(k0)));
    assert!(!matches!(r.tracks[p0[1]].0, Assign::Unsure(c) if c == at(k0)));
}

/// `people` characters drawn in one style: every face is `style` alike to
/// a look they all share, two of them on screen at once in every scene.
fn drawn(rng: &mut Rng, people: usize, scenes: u32, style: f32) -> (Vec<TrackIn>, Vec<usize>) {
    let shared = rng.unit();
    let centers: Vec<Vec<f32>> = (0..people).map(|_| around(rng, &shared, style)).collect();
    let mut tracks = Vec::new();
    let mut truth = Vec::new();
    for k in 0..scenes {
        for p in [k as usize % people, (k as usize + 1) % people] {
            tracks.push(track(0, k * 100, around(rng, &centers[p], 0.85), 0.8));
            truth.push(p);
        }
    }
    (tracks, truth)
}

#[test]
fn characters_drawn_in_one_style_stay_apart() {
    let mut rng = Rng(23);
    // Two characters' faces 0.35 alike on average: actors' bars would
    // merge them; the faces seen side by side tell how alike strangers look.
    let (tracks, truth) = drawn(&mut rng, 6, 90, 0.7);
    let alike = strangers(&tracks, &Params::default()).unwrap();
    assert!((0.25..0.45).contains(&alike), "strangers {alike}");
    let fixed = Params { calibrate_from: usize::MAX, ..Params::default() };
    let before = bcubed(&chars(&identify(&tracks, &fixed)), &truth);
    let o = identify(&tracks, &Params::default());
    let f = bcubed(&chars(&o), &truth);
    assert_eq!(o.characters.len(), 6);
    assert!(f > 0.97 && before < 0.8, "fitted F {f}, actors' bars F {before}");
    // A decision keeps to the same bars.
    let members: Vec<(CharKey, Vec<usize>)> = (0..6)
        .map(|c| (c as CharKey, (0..tracks.len()).filter(|&i| o.tracks[i].0 == Assign::Auto(c)).collect()))
        .collect();
    assert!(bcubed(&chars(&reassign(&tracks, &members, &Params::default(), None)), &truth) > 0.97);
}

#[test]
fn actors_keep_the_bars_they_were_set_for() {
    let mut rng = Rng(29);
    // Random people: strangers about 0 alike.
    let (tracks, _) = drawn(&mut rng, 6, 90, 0.0);
    let p = Params::default();
    assert!(strangers(&tracks, &p).unwrap().abs() < 0.05);
    let f = p.fitted(&tracks);
    assert_eq!(
        (f.link, f.assign, f.unsure, f.margin, f.copy_link),
        (p.link, p.assign, p.unsure, p.margin, p.copy_link)
    );
    // Too few faces seen at once to tell: as set.
    let (few, _) = drawn(&mut rng, 6, 20, 0.7);
    assert!(strangers(&few, &p).is_none());
    assert_eq!(p.fitted(&few).link, p.link);
}

#[test]
fn footage_said_to_be_animation_is_fitted_however_little_strangers_look_alike() {
    let mut rng = Rng(31);
    // A style whose strangers are ~0.1 alike: in the ramp, half moved.
    let (tracks, _) = drawn(&mut rng, 6, 90, 0.45);
    let p = Params::default();
    let alike = strangers(&tracks, &p).unwrap();
    assert!((0.06..0.14).contains(&alike), "strangers {alike}");
    let auto = p.fitted(&tracks);
    let said = Params { animated: true, ..p }.fitted(&tracks);
    assert!(p.link < auto.link && auto.link < said.link, "{} {} {}", p.link, auto.link, said.link);
    assert_eq!(said.link, p.for_strangers(alike).link);
    // Too few faces seen at once to tell: as set, animation or not.
    let (few, _) = drawn(&mut rng, 6, 20, 0.45);
    assert_eq!(Params { animated: true, ..p }.fitted(&few).link, p.link);
}

// -- the face space fitted to the footage (adapt) ----------------------------

/// A cartoon: `scenes` scenes of two of `people` characters drawn in one
/// style (strangers about `style` alike), every face clear.
fn cartoon(seed: u64, people: usize, scenes: u32, style: f32) -> (Vec<TrackIn>, Vec<usize>) {
    drawn(&mut Rng(seed), people, scenes, style)
}

#[test]
fn scatters_count_every_pair_of_faces() {
    use faer::Mat;
    let mut rng = Rng(41);
    let (n, dim, groups) = (40, 6, 7);
    let x = Mat::<f64>::from_fn(n, dim, |_, _| rng.gauss() as f64);
    let group: Vec<usize> = (0..n).map(|i| (i * 5 + i / 3) % groups).collect();
    // Seen together: each pair of groups once, both ways.
    let mut adjacent = vec![Vec::new(); groups];
    for (a, b) in [(0, 1), (0, 3), (2, 5), (4, 6), (1, 6)] {
        adjacent[a].push(b);
        adjacent[b].push(a);
    }
    let (cs, cd, same, other) = adapt::scatters(&x, &group, &adjacent);
    let (mut bs, mut bd) = (vec![0f64; dim * dim], vec![0f64; dim * dim]);
    let (mut ns, mut nd) = (0.0, 0.0);
    for i in 0..n {
        for j in i + 1..n {
            let (into, count) = if group[i] == group[j] {
                (&mut bs, &mut ns)
            } else if adjacent[group[i]].contains(&group[j]) {
                (&mut bd, &mut nd)
            } else {
                continue;
            };
            *count += 1.0;
            for r in 0..dim {
                for c in 0..dim {
                    into[r * dim + c] += (x[(i, r)] - x[(j, r)]) * (x[(i, c)] - x[(j, c)]);
                }
            }
        }
    }
    assert_eq!((same, other), (ns, nd));
    for r in 0..dim {
        for c in 0..dim {
            assert!((cs[(r, c)] - bs[r * dim + c] / ns).abs() < 1e-10, "C_S {r} {c}");
            assert!((cd[(r, c)] - bd[r * dim + c] / nd).abs() < 1e-10, "C_D {r} {c}");
        }
    }
}

#[test]
fn quantiles_interpolate_like_numpy() {
    let s = [0.1, 0.2, 0.4, 0.8];
    assert!((adapt::quantile(&s, 0.95) - 0.74).abs() < 1e-12);
    assert!((adapt::quantile(&s, 0.5) - 0.3).abs() < 1e-12);
    assert_eq!(adapt::quantile(&[0.7], 0.95), 0.7);
    assert!((adapt::quantile(&[0.2, 0.6], 0.25) - 0.3).abs() < 1e-12);
}

#[test]
fn whitening_keeps_only_the_directions_the_faces_show() {
    use faer::Mat;
    let mut rng = Rng(43);
    let dim = 10;
    // A full-rank spread of one person; three directions in which people differ.
    let a = Mat::<f64>::from_fn(dim, dim, |_, _| rng.gauss() as f64);
    let b = Mat::<f64>::from_fn(dim, 3, |_, _| rng.gauss() as f64);
    let cs = &a * a.transpose();
    let cd = &b * b.transpose();
    let p = adapt::whitening(cs.clone(), &cd).unwrap();
    assert_eq!(p.ncols(), 3);
    // Pᵀ (C_S + ridge) P = I, largest spread of different people first.
    let trace: f64 = (0..dim).map(|i| cs[(i, i)]).sum();
    let ridged =
        Mat::<f64>::from_fn(dim, dim, |r, c| cs[(r, c)] + if r == c { 0.05 * trace / dim as f64 } else { 0.0 });
    let id = p.transpose() * &ridged * &p;
    let spread = p.transpose() * &cd * &p;
    for r in 0..3 {
        for c in 0..3 {
            let want = if r == c { 1.0 } else { 0.0 };
            assert!((id[(r, c)] - want).abs() < 1e-9, "{r} {c}: {}", id[(r, c)]);
        }
    }
    assert!(spread[(0, 0)] >= spread[(1, 1)] && spread[(1, 1)] >= spread[(2, 2)]);
}

#[test]
fn live_action_and_too_few_faces_seen_together_keep_arcfaces_space() {
    let p = Params::default();
    // Actors: strangers about 0 alike.
    let (actors, _) = cartoon(47, 6, 250, 0.0);
    assert!(fit(&actors, &p).is_none());
    let o = identify(&actors, &p);
    assert_eq!(o.space, Space::Faces);
    let today = identify(&actors, &Params { adapt: false, ..p });
    assert_eq!(o.tracks, today.tracks);
    // A cartoon with fewer than 200 pairs of faces seen at once.
    let (few, _) = cartoon(53, 6, MIN_STRANGERS as u32 - 1, 0.7);
    assert!(fit(&few, &p).is_none());
    assert_eq!(identify(&few, &p).space, Space::Faces);
    // Enough of them.
    let (film, truth) = cartoon(53, 6, 250, 0.7);
    let (f, st) = fit(&film, &p).expect("a cartoon gets its own space");
    assert_eq!((st.strangers, st.share), (250, 1.0));
    assert!(f.k >= 1 && f.k <= 64 && (f.weight - 0.35).abs() < 1e-6);
    let o = identify(&film, &p);
    assert_eq!(o.space, Space::Fitted(f));
    assert_eq!(o.characters.len(), 6);
    assert!(bcubed(&chars(&o), &truth) > 0.97);
}

#[test]
fn footage_said_to_be_animation_gets_its_space_however_little_alike() {
    let p = Params::default();
    // Strangers about 0.09 alike: in the ramp.
    let (tracks, _) = cartoon(59, 6, 250, 0.35);
    let (_, st) = fit(&tracks, &p).unwrap();
    assert!(st.share > 0.0 && st.share < 1.0, "share {}", st.share);
    let (f, st) = fit(&tracks, &Params { animated: true, ..p }).unwrap();
    assert_eq!(st.share, 1.0);
    assert!((f.weight - 0.35).abs() < 1e-6);
}

#[test]
fn the_bars_follow_how_alike_strangers_still_look() {
    let p = Params::default();
    let (tracks, _) = cartoon(61, 6, 250, 0.7);
    let (f, st) = fit(&tracks, &p).unwrap();
    let o = (st.share * ((st.tail - p.link) / (1.0 - p.link)).max(0.0)).clamp(0.0, 0.5);
    let oc = (2.5 * o).min(0.5);
    assert!((f.link - (p.link + o * (1.0 - p.link))).abs() < 1e-6);
    assert!((f.copy_link - (p.copy_link + oc * (1.0 - p.copy_link))).abs() < 1e-6);
    assert!((f.copy_alike - (p.copy_alike + oc * (1.0 - p.copy_alike))).abs() < 1e-6);
    // assign, margin and min_cohesion stay; unsure goes with link.
    let q = f.params(&p);
    assert_eq!((q.assign, q.margin, q.min_cohesion, q.unsure), (p.assign, p.margin, p.min_cohesion, f.link));
}

#[test]
fn the_fit_is_the_same_in_any_order() {
    let p = Params::default();
    let (tracks, _) = cartoon(67, 6, 250, 0.7);
    let (f, _) = fit(&tracks, &p).unwrap();
    let mut order: Vec<usize> = (0..tracks.len()).collect();
    let mut rng = Rng(71);
    for i in (1..order.len()).rev() {
        order.swap(i, (rng.next() * (i + 1) as f32) as usize % (i + 1));
    }
    let shuffled: Vec<TrackIn> = order.iter().map(|&i| tracks[i].clone()).collect();
    let (g, _) = fit(&shuffled, &p).unwrap();
    assert_eq!(f.k, g.k);
    assert!((f.link - g.link).abs() < 1e-5 && (f.copy_link - g.copy_link).abs() < 1e-5);
    let dot = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
    for (a, b) in [(0, 1), (2, 40), (7, 300), (100, 499)] {
        let (za, zb) = (f.embed(&tracks[a].embedding), f.embed(&tracks[b].embedding));
        let (ya, yb) = (g.embed(&tracks[a].embedding), g.embed(&tracks[b].embedding));
        assert!((dot(&za, &zb) - dot(&ya, &yb)).abs() < 1e-5, "{a} {b}");
    }
}

#[test]
fn an_unreadable_face_spoils_nothing() {
    let p = Params::default();
    let (mut tracks, _) = cartoon(73, 6, 250, 0.7);
    let (f, _) = fit(&tracks, &p).unwrap();
    let mut broken = tracks[3].clone();
    broken.embedding[17] = f32::NAN;
    tracks.push(broken);
    let (g, _) = fit(&tracks, &p).unwrap();
    assert_eq!(f, g);
    let o = identify(&tracks, &p);
    assert_eq!(o.tracks.last().unwrap().0, Assign::None);
    assert!(o.characters.iter().all(|c| c.centroid.iter().all(|x| x.is_finite())));
}

#[test]
fn the_users_decisions_dont_shape_the_space() {
    let p = Params::default();
    let (tracks, _) = cartoon(79, 6, 250, 0.7);
    let (f, _) = fit(&tracks, &p).unwrap();
    let mut decided = tracks.clone();
    for (i, t) in decided.iter_mut().enumerate().take(60) {
        match i % 3 {
            0 => t.pin = Some(i as CharKey % 4),
            1 => t.forbid = vec![2],
            _ => t.quality = 0.5,
        }
        t.previous = Some(1);
    }
    let (g, _) = fit(&decided, &p).unwrap();
    assert_eq!(f, g);
}

#[test]
fn a_stored_space_comes_back_as_it_was() {
    let (tracks, _) = cartoon(83, 6, 250, 0.7);
    let (f, _) = fit(&tracks, &Params::default()).unwrap();
    let s = Space::Fitted(f);
    assert_eq!(Space::from_bytes(&s.to_bytes()), Some(s.clone()));
    assert_eq!(Space::from_bytes(&Space::Faces.to_bytes()), Some(Space::Faces));
    let b = s.to_bytes();
    assert_eq!(Space::from_bytes(&b[..b.len() - 1]), None);
    assert_eq!(Space::from_bytes(&[b.as_slice(), &[0]].concat()), None, "a byte too many");
    assert_eq!(Space::from_bytes(b"something else"), None);
    // A weight no fit makes (it would turn every face into NaN).
    let mut w = b.clone();
    w[16..20].copy_from_slice(&1.25f32.to_le_bytes());
    assert_eq!(Space::from_bytes(&w), None);
    // A size that doesn't fit in memory.
    let mut huge = b[..16].to_vec();
    huge[8..16].copy_from_slice(&[0xff; 8]);
    assert_eq!(Space::from_bytes(&huge), None);
}

#[test]
fn decisions_place_faces_in_the_space_the_cast_was_grouped_in() {
    let p = Params::default();
    let (mut tracks, truth) = cartoon(89, 6, 250, 0.7);
    let o = identify(&tracks, &p);
    let Space::Fitted(f) = o.space.clone() else { panic!("a cartoon gets its own space") };
    // A face the user says is no face leaves; the others stay where they were.
    let gone = (0..tracks.len()).find(|&i| matches!(o.tracks[i].0, Assign::Auto(_))).unwrap();
    let mut members: Vec<(CharKey, Vec<usize>)> = (0..o.characters.len())
        .map(|c| (c as CharKey, (0..tracks.len()).filter(|&i| i != gone && o.tracks[i].0 == Assign::Auto(c)).collect()))
        .collect();
    let pinned = members[0].1[0];
    tracks[pinned].pin = Some(0);
    tracks[gone].embedding = vec![f32::NAN; tracks[gone].embedding.len()];
    let r = reassign(&tracks, &members, &p, Some(&o.space));
    assert_eq!(r.space, Space::Fitted(f));
    assert_eq!(r.tracks[pinned].0, Assign::Pinned(0));
    let (before, after) = (chars(&o), chars(&r));
    let moved = (0..tracks.len()).filter(|&i| i != gone && before[i] != after[i]).count();
    assert!(moved <= 2, "{moved} faces moved");
    assert!(bcubed(&after, &truth) > 0.97);
    // A face the user said isn't character 1 never goes there, in this space too.
    let target = members[1].1.remove(0);
    tracks[target].forbid = vec![1];
    let r = reassign(&tracks, &members, &p, Some(&o.space));
    assert_ne!(r.tracks[target].0.character().and_then(|c| r.characters[c].key), Some(1));
}

// -- the anime preset ---------------------------------------------------------

#[test]
fn the_anime_preset_carries_ccips_bars() {
    let p = Params::anime();
    assert_eq!((p.link, p.copy_link), (0.45, 0.70));
    assert_eq!((p.core_quality, p.strangers_alone, p.strangers_style), (0.20, 0.02, 0.25));
    assert_eq!((p.table_dedup_cos, p.merge_top, p.cl_cap), (0.88, 4, 0.25));
    assert!(!p.animated && p.adapt);
    // The rest stays the live-action pass's.
    let d = Params::default();
    assert_eq!(
        (p.assign, p.margin, p.unsure, p.prototypes, p.min_cohesion, p.copy_share),
        (d.assign, d.margin, d.unsure, d.prototypes, d.min_cohesion, d.copy_share)
    );
}

#[test]
fn the_fitted_spaces_share_in_the_mix_is_capped() {
    let p = Params::default();
    let (tracks, _) = cartoon(97, 6, 250, 0.7);
    // Uncapped (1.0), as ever: the fitted space's share is the mix's.
    let (full, _) = fit(&tracks, &Params { animated: true, ..p }).unwrap();
    assert!((full.weight - 0.35).abs() < 1e-6);
    let (capped, _) = fit(&tracks, &Params { animated: true, cl_cap: 0.25, ..p }).unwrap();
    assert!((capped.weight - 0.25).abs() < 1e-6);
    // Same axes, only their share.
    assert_eq!(capped.projection, full.projection);
}

#[test]
fn faces_of_another_embedders_dimension_compare_to_no_one() {
    let mut rng = Rng(101);
    let (centers, mut tracks, _) = cast(&mut rng, 3, 10);
    // A track of another embedder's length among the pass's: it can't
    // compare to anyone and shapes nothing.
    let mut foreign = track(0, 9000, around(&mut rng, &centers[0], 0.8), 0.9);
    foreign.embedding = vec![0.5; 4];
    tracks.push(foreign);
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.tracks.last().unwrap().0, Assign::None);
    assert!(o.characters.iter().all(|c| c.centroid.len() == DIM));
    // Most tracks of another length: the session is theirs — the odd one out.
    for t in &mut tracks {
        t.embedding = vec![8f32.sqrt().recip(); 8];
    }
    tracks[0].embedding = vec![0.01; DIM];
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.tracks[0].0, Assign::None);
    assert!(o.tracks[1..].iter().all(|(a, _)| a.character().is_some()));
    // Placing faces after a decision leaves it out too.
    let members = vec![(5, (1..tracks.len()).collect())];
    assert_eq!(reassign(&tracks, &members, &Params::default(), None).tracks[0].0, Assign::None);
}

#[test]
fn stranger_pairs_weigh_by_the_less_seen_face() {
    let mut rng = Rng(103);
    let p = Params::default();
    // One pair of clear tracks on screen at a time, never two pairs at
    // once: 45 of strangers (~0 alike), 55 of look-alikes (~0.9) nobody
    // sees much of.
    let mut tracks = Vec::new();
    for i in 0..100u32 {
        let a = rng.unit();
        let (cos, w) = if i < 45 { (0.02, 1.0) } else { (0.9, 0.01) };
        let mut ta = track(0, i * 100, a.clone(), 0.8);
        let mut tb = track(0, i * 100, around(&mut rng, &a, cos), 0.8);
        ta.weight = w;
        tb.weight = w;
        tracks.extend([ta, tb]);
    }
    // The little-seen pairs barely count: the median pair is a stranger's.
    assert!(strangers(&tracks, &p).unwrap() < 0.4, "weighed");
    // Every pair the same: the median pair is a look-alike's.
    for t in &mut tracks {
        t.weight = 1.0;
    }
    assert!(strangers(&tracks, &p).unwrap() > 0.6, "unweighed");
}

#[test]
fn a_merge_never_sends_more_faces_aside_than_merge_top() {
    let mut rng = Rng(107);
    let person = rng.unit();
    let mut tracks = Vec::new();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 20);
    // The same person seen twice at three moments: the merge asks three
    // faces to step aside — copy_share of twenty allows it, a top of two
    // doesn't.
    for k in [3, 5, 7] {
        let at = tracks[l1[k]].start;
        tracks[l2[k]].start = at;
        tracks[l2[k]].end = at + 48;
        tracks[l2[k]].seen = tracks[l1[k]].seen.clone();
    }
    assert_eq!(identify(&tracks, &Params::default()).characters.len(), 1);
    let capped = Params { merge_top: 2, ..Params::default() };
    assert_eq!(identify(&tracks, &capped).characters.len(), 2);
}

#[test]
fn a_look_too_alike_gets_no_prototype_of_its_own() {
    let mut rng = Rng(109);
    let a = rng.unit();
    let b = around(&mut rng, &a, 0.93);
    // Members ~0.93 alike: a second centre at the live bar (0.98), not at
    // the anime one (0.88) — the same look there.
    let members: Vec<(&[f32], f32)> = [&a, &b, &a, &b].iter().map(|e| (e.as_slice(), 1.0)).collect();
    assert_eq!(proto::prototypes(&members, 12, 0.98).len(), 2);
    assert_eq!(proto::prototypes(&members, 12, 0.88).len(), 1);
}

/// Every track remembers the character `o` put it on.
fn remember(tracks: &mut [TrackIn], o: &Outcome) {
    for (t, (to, _)) in tracks.iter_mut().zip(&o.tracks) {
        t.previous = to.character().and_then(|c| o.characters[c].key);
    }
}

/// Whether the tracks `a` and the tracks `b` are two characters (each of them one).
fn are_two(o: &Outcome, a: &[usize], b: &[usize]) -> bool {
    one_character(o, a) != one_character(o, b)
}

/// Two people who look nothing alike, `per` tracks each, never in a frame
/// together. Returns the tracks and each one's indices.
fn two_people(rng: &mut Rng, per: [u32; 2]) -> (Vec<TrackIn>, [Vec<usize>; 2]) {
    let centres = [rng.unit(), rng.unit()];
    let mut tracks = Vec::new();
    let mut ids = [Vec::new(), Vec::new()];
    for i in 0..per[0].max(per[1]) {
        for s in 0..2 {
            if i < per[s] {
                ids[s].push(tracks.len());
                tracks.push(track(0, (2 * i + s as u32) * 100, around(rng, &centres[s], 0.85), 0.8));
            }
        }
    }
    (tracks, ids)
}

#[test]
fn a_merge_by_hand_of_two_unlike_characters_holds() {
    // The grouping made two characters (1 and 2) of faces far too unlike to
    // join; the user merged 2 into 1, as the cast stores it: 2's faces are
    // pinned to 1, 1's own faces were 1 and are not pinned.
    for per in [[30, 30], [40, 8], [8, 40]] {
        let mut rng = Rng(44);
        let (mut tracks, [a, b]) = two_people(&mut rng, per);
        // Apart without the merge.
        assert!(are_two(&identify(&tracks, &Params::default()), &a, &b));
        a.iter().for_each(|&i| tracks[i].previous = Some(1));
        b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
        // Six faces of a third person were 1 too (an older grouping's
        // mistake) and group apart now: 1's own faces are most of what it
        // held, not all of it.
        let third = rng.unit();
        let first_stray = tracks.len();
        for i in 0..6u32 {
            tracks.push(TrackIn {
                previous: Some(1),
                ..track(0, 100_000 + i * 100, around(&mut rng, &third, 0.85), 0.8)
            });
        }
        let strays: Vec<usize> = (first_stray..tracks.len()).collect();
        let o = identify(&tracks, &Params::default());
        assert_eq!(o.characters.len(), 2, "{per:?}");
        assert!(!are_two(&o, &a, &b));
        assert!(are_two(&o, &a, &strays));
        assert_eq!(o.characters[one_character(&o, &a)].key, Some(1));
        // Also when new episodes have added many more faces of the first
        // one, and when the cast is grouped once more after that.
        let first_new = tracks.len();
        for i in 0..120u32 {
            let like = tracks[a[i as usize % a.len()]].embedding.clone();
            tracks.push(track(1, i * 100, around(&mut rng, &like, 0.95), 0.8));
        }
        let all_a: Vec<usize> = a.iter().copied().chain(first_new..tracks.len()).collect();
        let o = identify(&tracks, &Params::default());
        assert!(!are_two(&o, &all_a, &b), "{per:?}");
        remember(&mut tracks, &o);
        let o = identify(&tracks, &Params::default());
        assert!(!are_two(&o, &all_a, &b), "{per:?}");
        assert_eq!(o.characters[one_character(&o, &b)].key, Some(1));
    }
}

#[test]
fn a_named_character_lets_go_of_faces_that_are_not_most_of_it() {
    // An older grouping put two people on one character (7), which the user
    // named: five faces of the bigger one are pinned. A better grouping
    // parts them: the smaller one's faces were 7, but are not most of what 7
    // held, and go on as a character of their own.
    let mut rng = Rng(45);
    let (mut tracks, [a, b]) = two_people(&mut rng, [40, 25]);
    tracks.iter_mut().for_each(|t| t.previous = Some(7));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(7));
    let o = identify(&tracks, &Params::default());
    assert!(are_two(&o, &a, &b));
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(7));
    assert_ne!(o.characters[one_character(&o, &b)].key, Some(7));
}

#[test]
fn a_group_that_was_mostly_another_character_does_not_follow_a_pin() {
    // Character 3 is three faces the user put together (pinned); four more
    // faces were placed with it and now group with the 40 faces of
    // character 5, of whom they are. They are all 3 held besides its pins,
    // but the group was 5 far more than 3: it stays 5.
    let mut rng = Rng(46);
    let (mut tracks, [a, b]) = two_people(&mut rng, [3, 44]);
    a.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(3), Some(3)));
    b.iter().for_each(|&i| tracks[i].previous = Some(5));
    b[..4].iter().for_each(|&i| tracks[i].previous = Some(3));
    let o = identify(&tracks, &Params::default());
    assert!(are_two(&o, &a, &b));
    assert_eq!(o.characters[one_character(&o, &b)].key, Some(5));
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(3));
}

#[test]
fn one_face_moved_to_another_character_moves_alone() {
    // Two characters (1 and 2), nobody named. The user moves one face of 2
    // to 1: it groups with 2's faces, and that group is then pinned to 1.
    // 1's own faces keep their character and take the moved face along; 2's
    // faces stay 2. (Before, the pinned face carried 2's whole group to 1,
    // and 1's own faces went on as a new character.) It stays so when the
    // cast is grouped again and again.
    for per in [[40, 40], [60, 30], [30, 60]] {
        let mut rng = Rng(47);
        let (mut tracks, [a, b]) = two_people(&mut rng, per);
        a.iter().for_each(|&i| tracks[i].previous = Some(1));
        b.iter().for_each(|&i| tracks[i].previous = Some(2));
        (tracks[b[0]].pin, tracks[b[0]].previous) = (Some(1), Some(1));
        for _ in 0..3 {
            let o = identify(&tracks, &Params::default());
            assert_eq!(o.characters.len(), 2, "{per:?}");
            assert!(are_two(&o, &a, &b[1..]), "{per:?}");
            assert_eq!(o.tracks[b[0]].0.character(), Some(one_character(&o, &a)), "{per:?}");
            assert_eq!(o.characters[one_character(&o, &a)].key, Some(1));
            assert_eq!(o.characters[one_character(&o, &b[1..])].key, Some(2));
            remember(&mut tracks, &o);
        }
    }
    // The same with new faces of the second one in the run: they are 2's.
    let mut rng = Rng(48);
    let (mut tracks, [a, b]) = two_people(&mut rng, [40, 40]);
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    b.iter().for_each(|&i| tracks[i].previous = Some(2));
    (tracks[b[0]].pin, tracks[b[0]].previous) = (Some(1), Some(1));
    let first_new = tracks.len();
    for i in 0..30u32 {
        let like = tracks[b[1 + i as usize % (b.len() - 1)]].embedding.clone();
        tracks.push(track(1, i * 100, around(&mut rng, &like, 0.95), 0.8));
    }
    let all_b: Vec<usize> = b[1..].iter().copied().chain(first_new..tracks.len()).collect();
    let o = identify(&tracks, &Params::default());
    assert!(are_two(&o, &a, &all_b));
    assert_eq!(o.characters[one_character(&o, &all_b)].key, Some(2));
}

#[test]
fn faces_moved_to_a_named_character_do_not_bring_their_own_along() {
    // 1 is named (five of its faces are pinned); the user moves six faces
    // of 2 to it. However the pinned faces group, 1 keeps its own faces and
    // the six, and 2 keeps the rest of its own.
    for moved in [1usize, 3, 6, 10] {
        let mut rng = Rng(49);
        let (mut tracks, [a, b]) = two_people(&mut rng, [40, 40]);
        a.iter().for_each(|&i| tracks[i].previous = Some(1));
        b.iter().for_each(|&i| tracks[i].previous = Some(2));
        a[..5].iter().for_each(|&i| tracks[i].pin = Some(1));
        b[..moved].iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
        for _ in 0..2 {
            let o = identify(&tracks, &Params::default());
            let one = one_character(&o, &a);
            assert!(are_two(&o, &a, &b[moved..]), "{moved}");
            assert!(b[..moved].iter().all(|&i| o.tracks[i].0.character() == Some(one)), "{moved}");
            assert_eq!(o.characters[one].key, Some(1));
            assert_eq!(o.characters[one_character(&o, &b[moved..])].key, Some(2));
            remember(&mut tracks, &o);
        }
    }
}

#[test]
fn a_stray_face_or_two_are_not_a_characters_own_faces() {
    // Character 3 is three faces the user put together (pinned). One more
    // face was once placed with it; it groups with the 40 faces of someone
    // who has no character yet. That face is all 3 held besides its pins,
    // but one face is no character's own faces: the 40 are a new character.
    let mut rng = Rng(50);
    let (mut tracks, [a, b]) = two_people(&mut rng, [3, 41]);
    a.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(3), Some(3)));
    tracks[b[0]].previous = Some(3);
    let o = identify(&tracks, &Params::default());
    assert!(are_two(&o, &a, &b));
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(3));
    assert_ne!(o.characters[one_character(&o, &b)].key, Some(3));
}

#[test]
fn what_a_character_held_is_weighed_by_its_clear_faces() {
    // 2 was merged into 1 by hand. 1 also held a crowd of faces too unclear
    // to shape a group, and a dozen of a third person's that group apart:
    // its own faces are most of what it held by what the clear faces say
    // (their quality), though not by the number of tracks.
    let mut rng = Rng(51);
    let (mut tracks, [a, b]) = two_people(&mut rng, [30, 30]);
    a.iter().for_each(|&i| (tracks[i].quality, tracks[i].previous) = (0.9, Some(1)));
    b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let third = rng.unit();
    for i in 0..40u32 {
        tracks.push(TrackIn { previous: Some(1), ..track(0, 100_000 + i * 100, around(&mut rng, &third, 0.85), 0.5) });
    }
    let own = rng.unit();
    for i in 0..300u32 {
        tracks.push(TrackIn { previous: Some(1), ..track(0, 200_000 + i * 100, around(&mut rng, &own, 0.2), 0.1) });
    }
    let o = identify(&tracks, &Params::default());
    assert!(!are_two(&o, &a, &b));
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(1));
}

/// `n` more tracks of the person the tracks `like` are of, in media `media`.
fn more_of(rng: &mut Rng, tracks: &mut Vec<TrackIn>, like: &[usize], media: u32, n: u32) -> Vec<usize> {
    let first = tracks.len();
    for i in 0..n {
        let face = tracks[like[i as usize % like.len()]].embedding.clone();
        tracks.push(track(media, i * 100, around(rng, &face, 0.95), 0.8));
    }
    (first..tracks.len()).collect()
}

#[test]
fn a_merge_holds_when_the_merged_in_one_outgrows_the_character() {
    // 2 (30 faces) was merged into 1 (40 faces) by hand: 2's faces are
    // pinned to 1, and so are five of 1's own, as a merge pins them. Every
    // episode then brings three times as many faces of the merged-in person
    // as of the other: soon most of what 1 "held" is the merged-in one's.
    // Both are held by pins, whatever grows.
    let mut rng = Rng(52);
    let (mut tracks, [a, b]) = two_people(&mut rng, [40, 30]);
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(1));
    b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let (mut all_a, mut all_b) = (a.clone(), b.clone());
    for episode in 1..4 {
        all_a.extend(more_of(&mut rng, &mut tracks, &a, episode, 20));
        all_b.extend(more_of(&mut rng, &mut tracks, &b, episode, 60));
        let o = identify(&tracks, &Params::default());
        assert_eq!(o.characters.len(), 1, "episode {episode}");
        assert!(!are_two(&o, &all_a, &all_b), "episode {episode}");
        assert_eq!(o.characters[one_character(&o, &all_a)].key, Some(1));
        remember(&mut tracks, &o);
    }
}

#[test]
fn faces_pinned_to_one_character_are_one_character_however_unlike() {
    // Nothing but pins: thirty faces each of two people, all pinned to 4.
    let mut rng = Rng(53);
    let (mut tracks, [a, b]) = two_people(&mut rng, [30, 30]);
    tracks.iter_mut().for_each(|t| t.pin = Some(4));
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    assert!(!are_two(&o, &a, &b));
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(4));
    // New faces of either join it, as they could not join one group of both.
    let new_a = more_of(&mut rng, &mut tracks, &a, 1, 20);
    let new_b = more_of(&mut rng, &mut tracks, &b, 2, 20);
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    assert!(!are_two(&o, &new_a, &new_b));
    assert_eq!(o.tracks[new_a[0]].0.character(), o.tracks[a[0]].0.character());
}

#[test]
fn characters_that_turn_out_to_be_one_person_still_become_one() {
    // 2 was merged into 1 by hand (forty faces pinned to 1). A third
    // character, 3, is that same person and groups with them now: twenty
    // faces, fewer than the pins. It becomes 1 as well.
    let mut rng = Rng(54);
    let (mut tracks, [a, b]) = two_people(&mut rng, [30, 40]);
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(1));
    b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let c = more_of(&mut rng, &mut tracks, &b, 1, 20);
    c.iter().for_each(|&i| tracks[i].previous = Some(3));
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    assert!(!are_two(&o, &b, &c));
    // A named character with few faces of its own that a bigger, unnamed
    // one of the same person groups with keeps its name over all of them:
    // the group is the named one's own.
    let mut rng = Rng(55);
    let mut tracks: Vec<TrackIn> = Vec::new();
    let one = rng.unit();
    for i in 0..15u32 {
        tracks.push(TrackIn { previous: Some(7), ..track(0, i * 100, around(&mut rng, &one, 0.85), 0.8) });
    }
    (0..5).for_each(|i| tracks[i].pin = Some(7));
    for i in 0..40u32 {
        tracks.push(TrackIn { previous: Some(9), ..track(1, i * 100, around(&mut rng, &one, 0.85), 0.8) });
    }
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    assert_eq!(o.characters[0].key, Some(7));
}

#[test]
fn a_character_made_of_a_few_faces_of_another_does_not_take_the_rest() {
    // The user made a new character (9) of three faces the grouping had put
    // on 2. They group with 2's forty faces again; 2 keeps its own.
    let mut rng = Rng(56);
    let (mut tracks, [a, b]) = two_people(&mut rng, [30, 43]);
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    b.iter().for_each(|&i| tracks[i].previous = Some(2));
    b[..3].iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(9), Some(9)));
    let o = identify(&tracks, &Params::default());
    assert!(are_two(&o, &b[..3], &b[3..]));
    assert_eq!(o.characters[one_character(&o, &b[3..])].key, Some(2));
    assert_eq!(o.characters[one_character(&o, &b[..3])].key, Some(9));
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(1));
}

/// Two selves of one person, the way a hero and the everyday self look to a
/// face model: `per[s]` tracks around each of two centres that are `alike`
/// to each other, each self's faces `fidelity[s]` like its centre, never in
/// a frame together. Returns the tracks and each self's indices.
fn selves(rng: &mut Rng, per: [u32; 2], alike: f32, fidelity: [f32; 2]) -> (Vec<TrackIn>, [Vec<usize>; 2]) {
    let a = rng.unit();
    let centres = [around(rng, &a, alike), a];
    let mut tracks = Vec::new();
    let mut ids = [Vec::new(), Vec::new()];
    for i in 0..per[0].max(per[1]) {
        for s in 0..2 {
            if i < per[s] {
                ids[s].push(tracks.len());
                tracks.push(track(0, (2 * i + s as u32) * 100, around(rng, &centres[s], fidelity[s]), 0.8));
            }
        }
    }
    (tracks, ids)
}

/// 40 tracks a self, each self's faces about 0.72 alike.
fn two_selves(rng: &mut Rng, alike: f32) -> (Vec<TrackIn>, [Vec<usize>; 2]) {
    selves(rng, [40, 40], alike, [0.85, 0.85])
}

/// The grouping with the rule that keeps two tight groups apart at `tight`.
fn tightly(tight: f32) -> Params {
    Params { tight, ..Params::default() }
}

/// The rule as the space fitted to animation switches it on.
fn as_animation() -> Params {
    tightly(Params::default().tight_animated)
}

#[test]
fn two_tight_groups_with_a_gap_between_them_stay_two() {
    // Each self's faces are about 0.72 alike, the two selves 0.43 (0.6 of
    // that): above the link bar, and never on screen together.
    let mut rng = Rng(31);
    let (tracks, [a, b]) = two_selves(&mut rng, 0.6);
    // Without the rule they are one character ...
    let plain = identify(&tracks, &Params::default());
    assert_eq!(plain.characters.len(), 1);
    assert!(!are_two(&plain, &a, &b));
    // ... with it two, each whole.
    let o = identify(&tracks, &as_animation());
    assert_eq!(o.characters.len(), 2);
    assert!(are_two(&o, &a, &b));
    // A share under what they are alike lets them join again.
    assert_eq!(identify(&tracks, &tightly(0.50)).characters.len(), 1);
}

#[test]
fn two_looks_nearly_as_alike_as_each_is_in_itself_still_join() {
    // A character in two lights: 0.8 of their own cohesion alike, above the
    // share the space fitted to animation asks for.
    let mut rng = Rng(32);
    let (tracks, [a, b]) = two_selves(&mut rng, 0.8);
    let o = identify(&tracks, &as_animation());
    assert_eq!(o.characters.len(), 1);
    assert!(!are_two(&o, &a, &b));
}

#[test]
fn the_less_coherent_group_sets_the_bar() {
    // One self tight (its faces 0.90 alike), the other loose (0.56); the two
    // 0.43 alike: over 0.70 of the loose one's cohesion, under 0.70 of the
    // tight one's. A loose group has no gap around it to speak of: they join.
    let mut rng = Rng(35);
    let (tracks, [a, b]) = selves(&mut rng, [40, 40], 0.6, [0.95, 0.75]);
    let o = identify(&tracks, &as_animation());
    assert!(!are_two(&o, &a, &b));
}

#[test]
fn groups_too_small_to_know_their_cohesion_join_as_before() {
    // 12 tracks a self: fewer than `tight_tracks`, so the rule keeps out.
    let mut rng = Rng(33);
    let p = as_animation();
    let (tracks, [a, b]) = selves(&mut rng, [12, 12], 0.6, [0.85, 0.85]);
    assert!(a.len() < p.tight_tracks);
    assert!(!are_two(&identify(&tracks, &p), &a, &b));
    // From as many tracks as they have, it holds them apart.
    assert!(are_two(&identify(&tracks, &Params { tight_tracks: 12, ..p }), &a, &b));
    // One large group and one small one: the small one is not judged.
    let (tracks, [a, b]) = selves(&mut rng, [40, 12], 0.6, [0.85, 0.85]);
    assert!(!are_two(&identify(&tracks, &p), &a, &b));
}

#[test]
fn a_merge_by_hand_of_two_selves_holds() {
    // The grouping made them two characters (1 and 2); the user merged 2
    // into 1, as the cast stores it: 2's faces are pinned to 1, 1's own
    // faces were 1 and are not pinned. The rule still holds the two groups
    // apart; 1's own faces keep their character (`keep_own_faces`). (The
    // merged self is the bigger one: what 1 held is counted without the
    // pins, or its own faces would be under half of it.)
    let mut rng = Rng(34);
    let (mut tracks, [a, b]) = selves(&mut rng, [40, 60], 0.6, [0.85, 0.85]);
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let o = identify(&tracks, &as_animation());
    assert_eq!(o.characters.len(), 1);
    assert!(!are_two(&o, &a, &b));
    // Also when new episodes have added three times as many faces of the
    // first self as there were (few of the group's faces were the
    // character): most of what the character held is in the group.
    let more: Vec<TrackIn> = (0..120u32)
        .map(|i| {
            let like = tracks[a[i as usize % a.len()]].embedding.clone();
            track(1, i * 100, around(&mut rng, &like, 0.95), 0.8)
        })
        .collect();
    let first_new = tracks.len();
    tracks.extend(more);
    let all_a: Vec<usize> = a.iter().copied().chain(first_new..tracks.len()).collect();
    let o = identify(&tracks, &as_animation());
    assert!(!are_two(&o, &all_a, &b));
}

#[test]
fn a_merge_of_two_selves_holds_when_the_merged_in_self_outgrows_the_other() {
    // The second self (60 faces) was merged into the first (40) by hand, and
    // five of the first self's own faces were pinned with the merge. The
    // two are about as alike as the link bar asks: the five must not start
    // among the sixty, or the first self's faces are held by nothing once
    // later episodes bring far more of the merged-in self.
    let mut rng = Rng(57);
    let (mut tracks, [a, b]) = selves(&mut rng, [40, 60], 0.6, [0.85, 0.85]);
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(1));
    b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let (mut all_a, mut all_b) = (a.clone(), b.clone());
    for episode in 1..4 {
        all_a.extend(more_of(&mut rng, &mut tracks, &a, episode, 10));
        all_b.extend(more_of(&mut rng, &mut tracks, &b, episode, 60));
        let o = identify(&tracks, &as_animation());
        assert_eq!(o.characters.len(), 1, "episode {episode}");
        assert!(!are_two(&o, &all_a, &all_b), "episode {episode}");
        assert_eq!(o.characters[one_character(&o, &all_a)].key, Some(1));
        remember(&mut tracks, &o);
    }
}

#[test]
fn a_named_character_lets_the_other_self_go_and_keeps_its_name() {
    // An older grouping made the two selves one character (7) and the user
    // named it: five faces of the bigger self are pinned, every face was 7.
    // The rule parts them; the name stays with the self it was pinned to,
    // the other self goes on as a character of its own.
    let mut rng = Rng(36);
    let (mut tracks, [a, b]) = selves(&mut rng, [60, 40], 0.6, [0.85, 0.85]);
    tracks.iter_mut().for_each(|t| t.previous = Some(7));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(7));
    let o = identify(&tracks, &as_animation());
    assert_eq!(o.characters.len(), 2);
    assert!(are_two(&o, &a, &b));
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(7));
    assert_ne!(o.characters[one_character(&o, &b)].key, Some(7));
    // Had the five pinned faces been of the smaller self, the bigger one
    // would be most of what 7 held: it keeps its character, and the two
    // stay one rather than leave the name with the smaller self alone.
    a[..5].iter().for_each(|&i| tracks[i].pin = None);
    b[..5].iter().for_each(|&i| tracks[i].pin = Some(7));
    let o = identify(&tracks, &as_animation());
    assert_eq!(o.characters.len(), 1);
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(7));
    assert!(!are_two(&o, &a, &b));
}

#[test]
fn a_named_character_does_not_take_the_other_self_back() {
    // The grouping keeps the two selves apart (7 and 8); the user named 7.
    // The other self was never 7: the rule still holds it apart.
    let mut rng = Rng(37);
    let (mut tracks, [a, b]) = two_selves(&mut rng, 0.6);
    a.iter().for_each(|&i| tracks[i].previous = Some(7));
    b.iter().for_each(|&i| tracks[i].previous = Some(8));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(7));
    let o = identify(&tracks, &as_animation());
    assert!(are_two(&o, &a, &b));
    // Nor because one stray face of the other self was once placed with 7.
    tracks[b[0]].previous = Some(7);
    let o = identify(&tracks, &as_animation());
    assert!(are_two(&o, &a, &b[1..]));
    // Nor because the user pinned one face of the other self there: that
    // face stays where it was put, the rest is still a character of its own.
    tracks[b[0]].pin = Some(7);
    let o = identify(&tracks, &as_animation());
    assert_eq!(o.tracks[b[0]].0.character(), Some(one_character(&o, &a)));
    assert!(are_two(&o, &a, &b[1..]));
}

#[test]
fn a_merged_character_keeps_its_faces_though_some_of_them_went_elsewhere() {
    // The second self (60 faces) was merged into character 1 by hand. Of the
    // 40 faces that were 1 before, 12 are someone else's (an older grouping
    // put them there) and group apart now; new episodes made the first
    // self's group mostly new faces (28 of 148 were 1). It still holds
    // most of what 1 held besides the pins, and keeps its character.
    let mut rng = Rng(39);
    let (mut tracks, [a, b]) = selves(&mut rng, [28, 60], 0.6, [0.85, 0.85]);
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let other = rng.unit();
    let first_stray = tracks.len();
    for i in 0..12u32 {
        tracks.push(TrackIn { previous: Some(1), ..track(0, 100_000 + i * 100, around(&mut rng, &other, 0.85), 0.8) });
    }
    let strays: Vec<usize> = (first_stray..tracks.len()).collect();
    let first_new = tracks.len();
    for i in 0..120u32 {
        let like = tracks[a[i as usize % a.len()]].embedding.clone();
        tracks.push(track(1, i * 100, around(&mut rng, &like, 0.95), 0.8));
    }
    let all_a: Vec<usize> = a.iter().copied().chain(first_new..tracks.len()).collect();
    let o = identify(&tracks, &as_animation());
    assert!(!are_two(&o, &all_a, &b));
    assert!(are_two(&o, &strays, &b));
}

#[test]
fn one_face_of_the_other_self_moved_by_hand_moves_alone() {
    // A fresh cast: the selves are characters 7 and 8, nobody is named. The
    // user moves one face of the second self to 7. With no other pin of 7,
    // that face groups with its own self, whose group is then pinned to 7;
    // the first self's faces follow the pin, and what the pinned group
    // holds of 8 goes on as 8: the two stay two, the moved face is with 7.
    let mut rng = Rng(42);
    let (mut tracks, [a, b]) = two_selves(&mut rng, 0.6);
    a.iter().for_each(|&i| tracks[i].previous = Some(7));
    b.iter().for_each(|&i| tracks[i].previous = Some(8));
    (tracks[b[0]].pin, tracks[b[0]].previous) = (Some(7), Some(7));
    let o = identify(&tracks, &as_animation());
    assert!(are_two(&o, &a, &b[1..]));
    assert_eq!(o.tracks[b[0]].0.character(), Some(one_character(&o, &a)));
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(7));
    assert_eq!(o.characters[one_character(&o, &b[1..])].key, Some(8));
}

#[test]
fn the_share_the_fitted_space_asks_for_parts_selves_a_little_under_it() {
    // Two selves 0.66 of their own cohesion alike: under the 0.70 of the
    // space fitted to animation, over 0.62.
    let mut rng = Rng(43);
    let (tracks, [a, b]) = two_selves(&mut rng, 0.66);
    assert!(are_two(&identify(&tracks, &as_animation()), &a, &b));
    assert!(!are_two(&identify(&tracks, &tightly(0.62)), &a, &b));
}

#[test]
fn two_tight_groups_stay_two_when_a_big_cast_is_grouped_per_media() {
    // Both selves in each of two episodes, the clustering forced to run per
    // media first and then across them.
    let mut rng = Rng(38);
    let (mut tracks, [a, b]) = two_selves(&mut rng, 0.6);
    a.iter().chain(&b).skip(1).step_by(2).for_each(|&i| tracks[i].media = 1);
    let core: Vec<usize> = (0..tracks.len()).collect();
    let levels = crate::ahc::Levels { single: 10, second: 1000 };
    let whole = |g: &[Vec<usize>], ids: &[usize]| g.iter().any(|x| ids.iter().all(|i| x.contains(i)));
    let groups = crate::ahc::cluster_in(&tracks, &core, &as_animation(), levels);
    assert!(whole(&groups, &a) && whole(&groups, &b));
    assert!(!groups.iter().any(|g| g.contains(&a[0]) && g.contains(&b[0])));
    // (Without the rule the same run makes one group of them.)
    let groups = crate::ahc::cluster_in(&tracks, &core, &Params::default(), levels);
    assert!(groups.iter().any(|g| g.contains(&a[0]) && g.contains(&b[0])));
}

#[test]
fn only_the_space_fitted_to_animation_holds_tight_groups_apart() {
    let fit = Fit {
        mean: vec![0.0; 4],
        projection: vec![1.0, 0.0, 0.0, 0.0],
        k: 1,
        weight: 0.35,
        link: 0.4,
        copy_link: 0.7,
        copy_alike: 0.6,
    };
    let p = Params::default();
    // Off by default, and in a space fitted to footage nobody called animation.
    assert_eq!(p.tight, 0.0);
    assert_eq!(fit.params(&p).tight, 0.0);
    // On where the user did.
    let animated = fit.params(&Params { animated: true, ..p });
    assert_eq!(animated.tight, p.tight_animated);
    assert!(animated.tight > 0.0 && animated.tight < 1.0);
    // Not for the anime embedder's footage (it is not `animated`), and not
    // for Fast Scan's faces, for which nothing was measured.
    assert_eq!(fit.params(&Params::anime()).tight, 0.0);
    assert_eq!(fit.params(&Params { animated: true, ..Params::sface() }).tight, 0.0);
}

/// B (`per[1]` faces, 0.85 like their centre) and A (`per[0]` faces, `fid`
/// like theirs, the first five `typical`), the centres `alike`; B's faces
/// come first in track order when `b_first`. Never in a frame together.
fn typical_pair(
    rng: &mut Rng,
    per: [u32; 2],
    alike: f32,
    fid: f32,
    typical: f32,
    b_first: bool,
) -> (Vec<TrackIn>, [Vec<usize>; 2]) {
    let b_centre = rng.unit();
    let a_centre = around(rng, &b_centre, alike);
    let mut tracks = Vec::new();
    let mut ids = [Vec::new(), Vec::new()];
    for s in if b_first { [1usize, 0] } else { [0, 1] } {
        for i in 0..per[s] {
            let f = if s == 1 {
                0.85
            } else if i < 5 {
                typical
            } else {
                fid
            };
            let centre = if s == 1 { &b_centre } else { &a_centre };
            ids[s].push(tracks.len());
            tracks.push(track(0, (s as u32 * 1000 + i) * 100, around(rng, centre, f), 0.8));
        }
    }
    (tracks, ids)
}

#[test]
fn a_merge_holds_when_the_pinned_faces_of_the_one_that_stays_are_its_most_typical() {
    // One person in two looks, about 0.24 alike on average: under the link
    // bar, so the grouping leaves them two characters and the user merges 2
    // into 1. The merge pins five typical faces of 1 (`as-project`
    // `anchors`: the sharpest of its most typical faces): closer to the
    // person than the others, so more like the other look too (0.32, over
    // the link bar), and they start among the merged-in faces. Later
    // episodes bring more of the merged-in look, until 1's own faces are not
    // most of what it held: the pins hold them (`held_by_pins`).
    let mut rng = Rng(1040);
    let (mut tracks, [a, b]) = typical_pair(&mut rng, [40, 60], 0.40, 0.70, 0.95, false);
    assert!(are_two(&identify(&tracks, &Params::default()), &a, &b), "two characters without the merge");
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(1));
    b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let (mut all_a, mut all_b) = (a.clone(), b.clone());
    for episode in 1..4 {
        // (Each look in a media of its own: no two new faces in one frame.)
        all_a.extend(more_of(&mut rng, &mut tracks, &a, episode, 10));
        all_b.extend(more_of(&mut rng, &mut tracks, &b, 10 + episode, 60));
        let o = identify(&tracks, &Params::default());
        assert_eq!(o.characters.len(), 1, "episode {episode}");
        assert!(!are_two(&o, &all_a, &all_b), "episode {episode}");
        remember(&mut tracks, &o);
    }
}

#[test]
fn a_merge_of_two_selves_holds_whatever_the_order_of_the_tracks() {
    // The two selves 0.66 of their own cohesion alike (the rule holds them
    // apart), the five pinned faces of the one that stays typical ones, and
    // the merged-in self's faces first in track order (its episode was
    // analysed first).
    for b_first in [false, true] {
        let mut rng = Rng(12_066);
        let (mut tracks, [a, b]) = typical_pair(&mut rng, [40, 60], 0.66, 0.80, 0.95, b_first);
        assert!(are_two(&identify(&tracks, &as_animation()), &a, &b), "two characters without the merge");
        a.iter().for_each(|&i| tracks[i].previous = Some(1));
        a[..5].iter().for_each(|&i| tracks[i].pin = Some(1));
        b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
        let (mut all_a, mut all_b) = (a.clone(), b.clone());
        for episode in 1..4 {
            all_a.extend(more_of(&mut rng, &mut tracks, &a, episode, 10));
            all_b.extend(more_of(&mut rng, &mut tracks, &b, 10 + episode, 60));
            let o = identify(&tracks, &as_animation());
            assert_eq!(o.characters.len(), 1, "merged-in first: {b_first}, episode {episode}");
            assert!(!are_two(&o, &all_a, &all_b), "merged-in first: {b_first}, episode {episode}");
            remember(&mut tracks, &o);
        }
    }
}

#[test]
fn faces_put_on_characters_by_hand_do_not_take_the_room_of_a_run() {
    // One person in two episodes, and sixty faces of sixty other people
    // that the user collected in one character (extras, to hide them), or
    // put ten each on six characters. The clustering runs per media first
    // (a big project; here forced by small levels): the person's two groups
    // must still meet across the episodes, and every pinned face is still
    // there.
    for one_character in [true, false] {
        let mut rng = Rng(77);
        let person = rng.unit();
        let mut tracks = Vec::new();
        for m in 0..2u32 {
            for i in 0..12u32 {
                tracks.push(track(m, i * 100, around(&mut rng, &person, 0.85), 0.8));
            }
        }
        let x: Vec<usize> = (0..tracks.len()).collect();
        for i in 0..60u32 {
            let mut t = track(i % 2, 10_000 + i * 100, rng.unit(), 0.8);
            t.pin = Some(if one_character { 9 } else { 100 + i % 6 });
            tracks.push(t);
        }
        let core: Vec<usize> = (0..tracks.len()).collect();
        let levels = crate::ahc::Levels { single: 10, second: 40 };
        let groups = crate::ahc::cluster_in(&tracks, &core, &Params::default(), levels);
        assert!(
            groups.iter().any(|g| x.iter().all(|i| g.contains(i))),
            "one group of the person across both episodes (one character: {one_character})"
        );
        let pinned: usize = groups.iter().filter(|g| tracks[g[0]].pin.is_some()).map(Vec::len).sum();
        assert_eq!(pinned, 60);
    }
}

#[test]
fn a_look_pinned_after_many_odd_faces_still_takes_its_new_faces() {
    // Character 1 holds 300 faces of 300 people, pinned to it one by one
    // (more than `MAX_ALONE`: the last ones make room, and the first go to
    // its odd faces), all earlier in the tracks' order than the thirty faces
    // of a character merged into it. The merged-in faces still start as a
    // group of their own, not among the odd faces (which are like nobody):
    // new faces of that person join the character.
    let mut rng = Rng(79);
    let mut tracks = Vec::new();
    for i in 0..300u32 {
        tracks.push(TrackIn { pin: Some(1), previous: Some(1), ..track(0, i * 100, rng.unit(), 0.8) });
    }
    let person = rng.unit();
    let merged: Vec<usize> = (0..30u32)
        .map(|i| {
            tracks.push(TrackIn {
                pin: Some(1),
                previous: Some(1),
                ..track(1, i * 100, around(&mut rng, &person, 0.85), 0.8)
            });
            tracks.len() - 1
        })
        .collect();
    let new = more_of(&mut rng, &mut tracks, &merged, 2, 20);
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    assert_eq!(o.characters[one_character(&o, &new)].key, Some(1));
    assert_eq!(one_character(&o, &new), one_character(&o, &merged));
}

#[test]
fn someone_seen_beside_a_characters_own_faces_does_not_come_to_it_through_its_other_look() {
    // Character 1 has pinned faces in two looks that are nothing alike (two
    // people merged by hand): fifteen of the second, all pinned, and
    // seventeen of the first, all pinned or only five of them (the others
    // were 1 before). Someone else, character 5, resembles the second look
    // and is on screen with a face of the first in every shot. Their twelve
    // faces group with the second look's; they are not 1, whose own faces
    // they were seen beside, and stay 5.
    for all_pinned in [true, false] {
        let mut rng = Rng(80);
        let (mut tracks, [first, second]) = two_people(&mut rng, [17, 15]);
        first.iter().for_each(|&i| tracks[i].previous = Some(1));
        first[..if all_pinned { 17 } else { 5 }].iter().for_each(|&i| tracks[i].pin = Some(1));
        second.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
        let mut look = vec![0f32; tracks[0].embedding.len()];
        second.iter().for_each(|&i| look.iter_mut().zip(&tracks[i].embedding).for_each(|(s, x)| *s += x));
        proto_normalize(&mut look);
        let other = around(&mut rng, &look, 0.8);
        let others: Vec<usize> = (0..12)
            .map(|i| {
                let beside = tracks[first[5 + i]].start;
                tracks.push(TrackIn { previous: Some(5), ..track(0, beside, around(&mut rng, &other, 0.85), 0.8) });
                tracks.len() - 1
            })
            .collect();
        let o = identify(&tracks, &Params::default());
        let c = one_character(&o, &[first.clone(), second.clone()].concat());
        assert_eq!(o.characters[c].key, Some(1), "all pinned: {all_pinned}");
        assert_eq!(o.characters[one_character(&o, &others)].key, Some(5), "all pinned: {all_pinned}");
        assert_eq!(o.characters.len(), 2, "all pinned: {all_pinned}");
    }
}

#[test]
fn new_faces_of_two_merged_people_seen_together_are_both_the_character() {
    // Two people merged by hand into 1 (all faces of the one pinned, five of
    // the other), who are on screen together in the next episode: a new face
    // of each in every shot. Both are 1: only a face seen beside the
    // character's own faces is held off, not one seen beside another new face.
    let mut rng = Rng(81);
    let (mut tracks, [a, b]) = two_people(&mut rng, [30, 30]);
    a.iter().for_each(|&i| tracks[i].previous = Some(1));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(1));
    b.iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let new_a = more_of(&mut rng, &mut tracks, &a, 1, 20);
    let new_b = more_of(&mut rng, &mut tracks, &b, 1, 20);
    assert!(new_a.iter().zip(&new_b).all(|(&x, &y)| together(&tracks[x], &tracks[y])));
    for round in 0..2 {
        let o = identify(&tracks, &Params::default());
        assert_eq!(o.characters.len(), 1, "round {round}");
        assert_eq!(one_character(&o, &new_a), one_character(&o, &new_b), "round {round}");
        remember(&mut tracks, &o);
    }
}

#[test]
fn a_character_made_of_a_few_faces_of_someone_inside_another_takes_that_persons_other_faces() {
    // Character 2 holds two people (an older grouping's mistake): forty
    // faces of one and ten of another. The user makes a new character (9) of
    // three of the ten. The ten group together, apart from the forty: they
    // are all 9, and 2 keeps the forty.
    let mut rng = Rng(2000);
    let (mut tracks, [x, y]) = two_people(&mut rng, [40, 10]);
    tracks.iter_mut().for_each(|t| t.previous = Some(2));
    y[..3].iter().for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(9), Some(9)));
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters[one_character(&o, &x)].key, Some(2));
    assert_eq!(o.characters[one_character(&o, &y)].key, Some(9));
}

#[test]
fn a_pinned_face_never_steps_aside_where_two_tight_groups_stay_two() {
    // `a_pinned_face_never_steps_aside`, in the space fitted to animation:
    // the odd pinned face starts apart from the other pinned faces of its
    // character there, and the face in the mirror beside it must still not
    // end on the character.
    let mut rng = Rng(67);
    let person = rng.unit();
    let mut tracks = Vec::new();
    let [l1, l2] = two_looks(&mut rng, &mut tracks, &person, 0, 10);
    for &i in &l1 {
        tracks[i].pin = Some(7);
    }
    tracks[l1[3]].embedding = around(&mut rng, &person, 0.6);
    let at = tracks[l1[3]].start;
    tracks[l2[3]].start = at;
    tracks[l2[3]].end = at + 48;
    tracks[l2[3]].seen = tracks[l1[3]].seen.clone();
    let o = identify(&tracks, &as_animation());
    let c = one_character(&o, &l1);
    assert_ne!(o.tracks[l2[3]].0.character(), Some(c), "two faces of one character in one frame");
}

/// The mean face of `ids` (unit length).
fn mean_face(tracks: &[TrackIn], ids: &[usize]) -> Vec<f32> {
    let mut sum = vec![0f32; tracks[ids[0]].embedding.len()];
    ids.iter().for_each(|&i| sum.iter_mut().zip(&tracks[i].embedding).for_each(|(s, x)| *s += x));
    proto_normalize(&mut sum);
    sum
}

#[test]
fn a_face_that_shares_no_frame_with_the_characters_own_is_not_beside_them() {
    // Two people merged by hand into 1, every face pinned. New faces of the
    // second, in the first one's media and at the very time of that one's
    // faces, but found in other frames (a track that fills the gaps of
    // another): never in a frame together, so nothing holds them off.
    let mut rng = Rng(82);
    let (mut tracks, [a, b]) = two_people(&mut rng, [15, 15]);
    tracks.iter_mut().for_each(|t| (t.pin, t.previous) = (Some(1), Some(1)));
    let new: Vec<usize> = (0..12)
        .map(|i| {
            let start = tracks[a[i]].start;
            let face = around(&mut rng, &tracks[b[i]].embedding, 0.95);
            let seen = (start..start + 48).filter(|f| f % 3 == 1).collect();
            tracks.push(TrackIn { seen, ..track(0, start, face, 0.8) });
            tracks.len() - 1
        })
        .collect();
    assert!(new.iter().zip(&a).all(|(&x, &y)| !together(&tracks[x], &tracks[y])));
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    assert_eq!(one_character(&o, &new), one_character(&o, &b));
}

#[test]
fn two_people_pinned_to_one_character_who_are_on_screen_together_are_one_character() {
    // Every face of two people pinned to 4, each face of the one in the
    // frames of a face of the other: pinned faces are the character's, beside
    // whomever they were seen.
    let mut rng = Rng(83);
    let (mut tracks, [a, b]) = two_people(&mut rng, [20, 20]);
    for (&x, &y) in a.iter().zip(&b) {
        (tracks[y].start, tracks[y].end, tracks[y].seen) = (tracks[x].start, tracks[x].end, tracks[x].seen.clone());
    }
    tracks.iter_mut().for_each(|t| t.pin = Some(4));
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1);
    assert_eq!(o.characters[one_character(&o, &[a, b].concat())].key, Some(4));
}

#[test]
fn pinned_faces_that_resemble_no_part_of_a_character_hold_none() {
    // Character 3 is twenty faces of twenty people, pinned to it one by one,
    // and three people it held besides (ten, ten and eight faces: none is
    // most of it). The pinned faces resemble none of them, however little
    // they resemble each other: the three go on as characters of their own.
    let mut rng = Rng(84);
    let (mut tracks, [x, y]) = two_people(&mut rng, [10, 10]);
    let third = rng.unit();
    let z: Vec<usize> = (0..8u32)
        .map(|i| {
            tracks.push(track(2, i * 100, around(&mut rng, &third, 0.85), 0.8));
            tracks.len() - 1
        })
        .collect();
    tracks.iter_mut().for_each(|t| t.previous = Some(3));
    let odd: Vec<usize> = (0..20u32)
        .map(|i| {
            tracks.push(TrackIn { pin: Some(3), previous: Some(3), ..track(1, i * 100, rng.unit(), 0.8) });
            tracks.len() - 1
        })
        .collect();
    let o = identify(&tracks, &Params::default());
    let c = one_character(&o, &odd);
    assert_eq!(o.characters[c].key, Some(3));
    assert_eq!(o.characters.len(), 4);
    for part in [&x, &y, &z] {
        assert_ne!(one_character(&o, part), c);
    }
}

#[test]
fn one_pinned_face_that_resembles_a_part_does_not_hold_it() {
    // Character 1: forty faces of one person, five of them pinned (a name),
    // and ten faces of a second person it held besides (a wrong join that
    // the grouping now parts). One more face pinned to 1 resembles the
    // second person and was seen beside one of their faces, so it is no
    // group with them. A single such pin does not hold the part: the ten go
    // on as a new character.
    let mut rng = Rng(85);
    let (mut tracks, [x, y]) = two_people(&mut rng, [40, 10]);
    tracks.iter_mut().for_each(|t| t.previous = Some(1));
    x[..5].iter().for_each(|&i| tracks[i].pin = Some(1));
    let face = around(&mut rng, &mean_face(&tracks, &y), 0.6);
    tracks.push(TrackIn { pin: Some(1), previous: Some(1), ..track(0, tracks[y[0]].start, face, 0.8) });
    let pinned = tracks.len() - 1;
    let o = identify(&tracks, &Params::default());
    let c = one_character(&o, &x);
    assert_eq!(o.characters[c].key, Some(1));
    assert_eq!(o.tracks[pinned].0.character(), Some(c));
    assert_ne!(one_character(&o, &y), c);
}

#[test]
fn one_face_of_someone_inside_another_character_moved_to_a_third_moves_alone() {
    // Character 2 holds two people, forty faces and ten. The user moves one
    // of the ten to character 1, which has thirty faces of its own (a third
    // person). It moves alone, as a face of a whole character does: only a
    // character made of nothing but the picked faces takes the others of
    // their person along.
    let mut rng = Rng(86);
    let (mut tracks, [x, y]) = two_people(&mut rng, [40, 10]);
    tracks.iter_mut().for_each(|t| t.previous = Some(2));
    let third = rng.unit();
    let a: Vec<usize> = (0..30u32)
        .map(|i| {
            tracks.push(TrackIn { previous: Some(1), ..track(1, i * 100, around(&mut rng, &third, 0.85), 0.8) });
            tracks.len() - 1
        })
        .collect();
    (tracks[y[0]].pin, tracks[y[0]].previous) = (Some(1), Some(1));
    let o = identify(&tracks, &Params::default());
    let c = one_character(&o, &a);
    assert_eq!(o.characters[c].key, Some(1));
    assert_eq!(o.tracks[y[0]].0.character(), Some(c));
    assert!(y[1..].iter().all(|&i| o.tracks[i].0.character() != Some(c)));
    assert_eq!(o.characters[one_character(&o, &x)].key, Some(2));
}

/// `n` faces around `centre`, one after another from `from` on in media `media`.
fn faces_of(rng: &mut Rng, tracks: &mut Vec<TrackIn>, centre: &[f32], media: u32, from: u32, n: u32) -> Vec<usize> {
    let first = tracks.len();
    for i in 0..n {
        tracks.push(track(media, from + i * 100, around(rng, centre, 0.85), 0.8));
    }
    (first..tracks.len()).collect()
}

#[test]
fn a_named_character_that_also_holds_a_merge_lets_the_other_self_go() {
    // As `a_named_character_lets_the_other_self_go_and_keeps_its_name`: an
    // older grouping made the two selves one character (7), named: five
    // faces of the bigger self are pinned. Here 7 also holds someone else,
    // merged into it by hand (eighty faces pinned, unlike both selves). The
    // rule parts the selves all the same: the pins of the bigger self are in
    // their own self's group, not in other company, and do not hold the
    // other self (`held_by_pins` goes by the group a pin left the clustering
    // in, not by all that is pinned to the character).
    let mut rng = Rng(20_000);
    let one = rng.unit();
    let other = around(&mut rng, &one, 0.6);
    let third = rng.unit();
    let mut tracks = Vec::new();
    let a = faces_of(&mut rng, &mut tracks, &one, 0, 0, 60);
    let b = faces_of(&mut rng, &mut tracks, &other, 0, 100_000, 40);
    let c = faces_of(&mut rng, &mut tracks, &third, 0, 200_000, 80);
    tracks.iter_mut().for_each(|t| t.previous = Some(7));
    a[..5].iter().for_each(|&i| tracks[i].pin = Some(7));
    c.iter().for_each(|&i| tracks[i].pin = Some(7));
    let o = identify(&tracks, &as_animation());
    assert!(!are_two(&o, &a, &c), "the merge holds");
    assert_eq!(o.characters[one_character(&o, &a)].key, Some(7));
    assert!(are_two(&o, &a, &b), "the other self goes on as a character of its own");
}

#[test]
fn someone_beside_a_character_in_a_few_of_their_shots_stays_whole() {
    // 7 is named and holds two unlike looks (three faces pinned of the
    // first, two of the second). Someone else (character 8, twenty faces)
    // resembles the second look and is on screen with a face of the first
    // look in three of their shots. Those three say who the other seventeen
    // are not: all twenty stay 8, not three of them.
    let mut rng = Rng(22_000);
    let (mut tracks, [l1, l2]) = two_people(&mut rng, [25, 20]);
    tracks.iter_mut().for_each(|t| t.previous = Some(7));
    l1[..3].iter().for_each(|&i| tracks[i].pin = Some(7));
    l2[..2].iter().for_each(|&i| tracks[i].pin = Some(7));
    let other = around(&mut rng, &mean_face(&tracks, &l2), 0.6);
    let first = tracks.len();
    for k in 0..20usize {
        let start = if k < 3 { tracks[l1[k]].start } else { 500_000 + k as u32 * 100 };
        tracks.push(TrackIn { previous: Some(8), ..track(0, start, around(&mut rng, &other, 0.85), 0.8) });
    }
    let y: Vec<usize> = (first..tracks.len()).collect();
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters[one_character(&o, &l1)].key, Some(7));
    assert_eq!(o.characters[one_character(&o, &y)].key, Some(8));
}

#[test]
fn looks_merged_into_a_character_full_of_start_groups_take_their_new_faces() {
    // Character 1 has forty pairs of pinned faces (each pair unlike the
    // others) and twenty single ones. Two looks are merged into it later in
    // the tracks' order (forty pinned faces each); the next episode brings
    // thirty new faces of each. They are 1's: looks never make room, and
    // each starts as a group of its own.
    let mut rng = Rng(23_000);
    let mut tracks = Vec::new();
    let mut at = 0u32;
    for pair in 0..60u32 {
        let c = rng.unit();
        for _ in 0..(if pair < 40 { 2 } else { 1 }) {
            tracks.push(TrackIn { pin: Some(1), previous: Some(1), ..track(0, at, around(&mut rng, &c, 0.97), 0.8) });
            at += 100;
        }
    }
    let (b_centre, c_centre) = (rng.unit(), rng.unit());
    let b = faces_of(&mut rng, &mut tracks, &b_centre, 0, 1_000_000, 40);
    let c = faces_of(&mut rng, &mut tracks, &c_centre, 0, 2_000_000, 40);
    b.iter().chain(&c).for_each(|&i| (tracks[i].pin, tracks[i].previous) = (Some(1), Some(1)));
    let new_b = faces_of(&mut rng, &mut tracks, &b_centre, 1, 0, 30);
    let new_c = faces_of(&mut rng, &mut tracks, &c_centre, 1, 100_000, 30);
    let o = identify(&tracks, &Params::default());
    let one = one_character(&o, &b);
    assert_eq!(o.characters[one].key, Some(1));
    assert_eq!(one_character(&o, &new_b), one, "the first look's new faces");
    assert_eq!(one_character(&o, &new_c), one, "the second look's new faces");
}

/// Two unlike looks of character 1, fifteen faces each, every face pinned,
/// a thousand frames apart (the second look half a thousand after the
/// first). Returns the tracks and each look's indices.
fn two_pinned_looks(rng: &mut Rng) -> (Vec<TrackIn>, [Vec<usize>; 2]) {
    let centres = [rng.unit(), rng.unit()];
    let mut tracks = Vec::new();
    let mut ids = [Vec::new(), Vec::new()];
    for i in 0..15u32 {
        for s in 0..2 {
            ids[s].push(tracks.len());
            let face = around(rng, &centres[s], 0.85);
            tracks.push(TrackIn {
                pin: Some(1),
                previous: Some(1),
                ..track(0, 100 + i * 1000 + s as u32 * 500, face, 0.8)
            });
        }
    }
    (tracks, ids)
}

#[test]
fn beside_is_a_shared_frame_whichever_face_starts_first_and_however_long_the_other_lasts() {
    // Someone else (character 5) resembles the second look of 1 and is in
    // frames of the first look's faces: starting after the face beside them,
    // before it, or late in a face that lasts far longer than the others.
    // Each time they stay 5.
    for kind in 0..3 {
        let mut rng = Rng(87);
        let (mut tracks, [first, second]) = two_pinned_looks(&mut rng);
        if kind == 2 {
            for &i in &first {
                let t = &mut tracks[i];
                t.end = t.start + 348;
                t.seen = (t.start..t.end).filter(|f| f % 3 == 0).collect();
            }
        }
        let other = around(&mut rng, &mean_face(&tracks, &second), 0.8);
        let others: Vec<usize> = (0..12)
            .map(|i| {
                let beside = tracks[first[i]].start;
                let start = [beside + 24, beside - 24, beside + 300][kind];
                tracks.push(TrackIn { previous: Some(5), ..track(0, start, around(&mut rng, &other, 0.85), 0.8) });
                tracks.len() - 1
            })
            .collect();
        assert!(others.iter().zip(&first).all(|(&x, &y)| together(&tracks[x], &tracks[y])), "kind {kind}");
        let o = identify(&tracks, &Params::default());
        let c = one_character(&o, &[first.clone(), second.clone()].concat());
        assert_eq!(o.characters[c].key, Some(1), "kind {kind}");
        assert_eq!(o.characters[one_character(&o, &others)].key, Some(5), "kind {kind}");
    }
}

#[test]
fn what_leaves_two_of_a_characters_groups_stays_two_groups() {
    // Two people, characters 5 and 6: the one resembles the second look of
    // 1 and is in the frames of the first look's faces, the other resembles
    // the first look and is in the frames of the second's. Both leave 1's
    // groups, each as what it was.
    let mut rng = Rng(88);
    let (mut tracks, [first, second]) = two_pinned_looks(&mut rng);
    let mut beside = |tracks: &mut Vec<TrackIn>, like: &[usize], with: &[usize], key: CharKey| -> Vec<usize> {
        let other = around(&mut rng, &mean_face(tracks, like), 0.8);
        (0..12)
            .map(|i| {
                let start = tracks[with[i]].start;
                tracks.push(TrackIn { previous: Some(key), ..track(0, start, around(&mut rng, &other, 0.85), 0.8) });
                tracks.len() - 1
            })
            .collect()
    };
    let five = beside(&mut tracks, &second, &first, 5);
    let six = beside(&mut tracks, &first, &second, 6);
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters[one_character(&o, &[first, second].concat())].key, Some(1));
    assert_eq!(o.characters[one_character(&o, &five)].key, Some(5));
    assert_eq!(o.characters[one_character(&o, &six)].key, Some(6));
    assert_eq!(o.characters.len(), 3);
}

/// A face of quality 1 in media 0 from `start` on (48 frames), with the pin
/// and the character before as the cast left them.
fn left_as(start: u32, embedding: Vec<f32>, pin: Option<CharKey>, previous: Option<CharKey>) -> TrackIn {
    TrackIn { pin, previous, ..track(0, start, embedding, 1.0) }
}

/// A unit face in eight dimensions, `cos` like the first axis and the rest along `axis`.
fn leaning(cos: f32, axis: usize) -> Vec<f32> {
    let mut e = vec![0.0; 8];
    e[0] = cos;
    e[axis] += (1.0 - cos * cos).max(0.0).sqrt();
    e
}

/// Whether the faces pinned to character 1 in `pinned` hold the part `g`
/// (`company`: per track, its group in the clustering).
fn holds(tracks: &[TrackIn], g: &[usize], pinned: &[usize], company: &[usize]) -> bool {
    let around = Beside::new(tracks, std::iter::empty());
    held_by_pins(tracks, g, 0, &pins_and_their_company(tracks, pinned, 1, company), &around, &[], &Params::default())
}

#[test]
fn pins_each_alone_in_their_company_hold_a_part_they_resemble() {
    // A part of character 1 (two faces) and two faces pinned to 1, each 0.5
    // like the part and each the only face of its group in the clustering.
    // A face alone is like no one in its company, so both vote: the pins
    // hold the part.
    let tracks = [
        left_as(0, leaning(1.0, 1), None, Some(1)),
        left_as(100, leaning(1.0, 1), None, Some(1)),
        left_as(200, leaning(0.5, 2), Some(1), Some(1)),
        left_as(300, leaning(0.5, 3), Some(1), Some(1)),
    ];
    assert!(holds(&tracks, &[0, 1], &[2, 3], &[0, 0, 1, 2]));
}

#[test]
fn a_pin_is_not_its_own_company() {
    // Two faces pinned to 1, each 0.45 like a part of 1 and each in a group
    // of the clustering with one face unlike it (0). They are more like the
    // part than like the rest of their company, so they hold it. (Counted in
    // its own company, a pin would be 0.5 like it.)
    let tracks = [
        left_as(0, leaning(1.0, 1), None, Some(1)),
        left_as(100, leaning(1.0, 1), None, Some(1)),
        left_as(200, leaning(0.45, 2), Some(1), Some(1)),
        left_as(300, leaning(0.45, 3), Some(1), Some(1)),
        left_as(400, leaning(0.0, 4), None, Some(1)),
        left_as(500, leaning(0.0, 5), None, Some(1)),
    ];
    assert!(holds(&tracks, &[0, 1], &[2, 3, 4, 5], &[0, 0, 1, 2, 1, 2]));
}

#[test]
fn only_faces_pinned_to_the_character_vote_to_hold_a_part() {
    // The group pinned to 1 holds a face pinned to it and a face it held
    // before, both 0.5 like a part of 1 and each alone in its company. The
    // face that is not pinned has no vote, and one vote does not hold.
    let tracks = [
        left_as(0, leaning(1.0, 1), None, Some(1)),
        left_as(100, leaning(1.0, 1), None, Some(1)),
        left_as(200, leaning(0.5, 2), Some(1), Some(1)),
        left_as(300, leaning(0.5, 3), None, Some(1)),
    ];
    assert!(!holds(&tracks, &[0, 1], &[2, 3], &[0, 0, 1, 2]));
}

#[test]
fn a_character_that_held_a_face_besides_its_pins_was_not_made_by_hand() {
    // Character 1 held one face besides its pins, elsewhere. A group pinned
    // to it is one pinned face and three faces of character 2, which holds
    // ten more elsewhere. Not made by hand, character 1 weighs all of 2's
    // faces against its pin: they outweigh it and leave.
    let face = || leaning(1.0, 1);
    let mut tracks = vec![left_as(0, face(), None, Some(1)), left_as(100, face(), Some(1), Some(1))];
    tracks.extend((0..13).map(|i| left_as(200 + 100 * i, face(), None, Some(2))));
    let core: Vec<usize> = (0..tracks.len()).collect();
    let out = pins_together(&tracks, &core, vec![vec![0], vec![1, 2, 3, 4], (5..15).collect()]);
    assert!(out.contains(&vec![1]), "{out:?}");
    assert!(out.contains(&vec![2, 3, 4]), "{out:?}");
}

#[test]
fn a_face_put_on_a_character_by_hand_never_leaves_with_the_person_it_was() {
    // Character 1 is pinned in two groups: its own face with a pin, and a
    // face the user moved to it from character 2 with a face 2 held (not
    // pinned), seen in a frame with 1's own face. The face of 2 leaves; the
    // face the user moved stays with character 1.
    let face = || leaning(1.0, 1);
    let tracks = [
        left_as(0, face(), None, Some(1)),
        left_as(1000, face(), Some(1), Some(1)),
        left_as(2000, face(), Some(1), Some(2)),
        left_as(0, face(), None, Some(2)),
    ];
    let out = pins_together(&tracks, &[0, 1, 2, 3], vec![vec![0, 1], vec![2, 3]]);
    assert_eq!(out, [vec![0, 1, 2], vec![3]]);
}

#[test]
fn a_pin_seen_beside_a_part_does_not_vote_for_it() {
    // Two faces pinned to 1, each 0.5 like a part of 1 and alone in its
    // company, but one of them is seen in a frame with a face of the part:
    // it is not of the part's person. One vote does not hold.
    let tracks = [
        left_as(0, leaning(1.0, 1), None, Some(1)),
        left_as(100, leaning(1.0, 1), None, Some(1)),
        left_as(200, leaning(0.5, 2), Some(1), Some(1)),
        left_as(110, leaning(0.5, 3), Some(1), Some(1)),
    ];
    assert!(!holds(&tracks, &[0, 1], &[2, 3], &[0, 0, 1, 2]));
}

#[test]
fn the_staying_persons_unplaced_faces_beside_the_merged_in_one_stay() {
    // Two people on screen together (B's first twenty faces in the frames
    // of A's faces), merged by hand: B's faces pinned to 1, five of A's
    // pinned. Ten more faces of A, in shots where B was found too, were no
    // character before (left unsure, or on another). They are A's: they
    // belong on 1, not on a character of their own. (Review 6.)
    for prev in [None, Some(5)] {
        let mut rng = Rng(7701);
        let (ca, cb) = (rng.unit(), rng.unit());
        let mut tracks = Vec::new();
        for i in 0..30u32 {
            tracks.push(TrackIn { previous: Some(1), ..track(0, i * 1000, around(&mut rng, &ca, 0.85), 0.8) });
        }
        for i in 0..30u32 {
            let start = if i < 20 { i * 1000 } else { 100_000 + i * 1000 };
            tracks.push(TrackIn {
                pin: Some(1),
                previous: Some(1),
                ..track(0, start, around(&mut rng, &cb, 0.85), 0.8)
            });
        }
        (0..5).for_each(|i| tracks[i].pin = Some(1));
        let x: Vec<usize> = (20..30u32)
            .map(|i| {
                tracks
                    .push(TrackIn { previous: prev, ..track(0, 100_000 + i * 1000, around(&mut rng, &ca, 0.85), 0.8) });
                tracks.len() - 1
            })
            .collect();
        let o = identify(&tracks, &Params::default());
        let on_1 =
            x.iter().filter(|&&i| o.tracks[i].0.character().is_some_and(|c| o.characters[c].key == Some(1))).count();
        assert_eq!(on_1, 10, "previous {prev:?}: {} characters", o.characters.len());
    }
}

#[test]
fn forty_looks_of_one_character_take_their_new_faces() {
    // Forty looks of one character (five pinned faces each, as after
    // several merges); a new episode brings ten faces of every look. Every
    // look is a start group, so all new faces are the character's.
    // (Review 6.)
    let mut rng = Rng(31377);
    let centres: Vec<Vec<f32>> = (0..40).map(|_| rng.unit()).collect();
    let mut tracks = Vec::new();
    for ep in 0..5u32 {
        for (l, c) in centres.iter().enumerate() {
            tracks.push(TrackIn {
                pin: Some(1),
                previous: Some(1),
                ..track(ep, l as u32 * 100, around(&mut rng, c, 0.85), 0.8)
            });
        }
    }
    let first_new = tracks.len();
    for (l, c) in centres.iter().enumerate() {
        for i in 0..10u32 {
            tracks.push(track(100, (l as u32 * 10 + i) * 100, around(&mut rng, c, 0.85), 0.8));
        }
    }
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1, "new faces of {} looks are other characters", o.characters.len() - 1);
    assert!((first_new..tracks.len()).all(|i| o.tracks[i].0.character().is_some()));
}

#[test]
fn pins_do_not_hold_a_look_alike_seen_beside_the_character() {
    // B (60 faces) merged into A (40): B's faces pinned to 1, and A's five
    // most typical faces, which start among B's (as like them as the bar
    // asks). Someone who resembles A (12 faces) was on 1 before, wrongly,
    // in frames where A's faces were not; the grouping parts them from A.
    // They must not come back to 1 through A's pins beside A's own faces:
    // A's faces stay on 1, the look-alike's do not. (Review 6.)
    let cases = [(0.40f32, 0.70f32, 0.6f32), (0.50, 0.75, 0.8)];
    for ((alike, fid, l_alike), beside_was) in cases.into_iter().flat_map(|c| [(c, None), (c, Some(1))]) {
        let mut rng = Rng(1307 + (alike * 100.0) as u64);
        let b_c = rng.unit();
        let a_c = around(&mut rng, &b_c, alike);
        let l_c = around(&mut rng, &a_c, l_alike);
        let mut tracks = Vec::new();
        for i in 0..40u32 {
            let f = if i < 5 { 0.95 } else { fid };
            let pin = (i < 5).then_some(1);
            // A's faces in the look-alike's frames were not on 1 (one character per frame), or
            // were, as no grouping leaves it.
            let previous = if (5..17).contains(&i) { beside_was } else { Some(1) };
            tracks.push(TrackIn { pin, previous, ..track(0, i * 1000, around(&mut rng, &a_c, f), 0.8) });
        }
        for i in 0..60u32 {
            tracks.push(TrackIn {
                pin: Some(1),
                previous: Some(1),
                ..track(1, i * 1000, around(&mut rng, &b_c, 0.85), 0.8)
            });
        }
        let l: Vec<usize> = (0..12u32)
            .map(|i| {
                let beside = tracks[5 + i as usize].start;
                tracks.push(TrackIn { previous: Some(1), ..track(0, beside, around(&mut rng, &l_c, 0.85), 0.8) });
                tracks.len() - 1
            })
            .collect();
        let o = identify(&tracks, &Params::default());
        let on = |i: usize| o.tracks[i].0.character().is_some_and(|c| o.characters[c].key == Some(1));
        let a_on = (0..40).filter(|&i| on(i)).count();
        let l_on = l.iter().filter(|&&i| on(i)).count();
        assert_eq!(
            (a_on, l_on),
            (40, 0),
            "alike {alike}, beside {beside_was:?}: A's faces on 1, the look-alike's on 1"
        );
    }
}

#[test]
fn faces_beside_a_part_that_are_like_no_pin_say_nothing_about_it() {
    // A part of character 1 (two faces, 0.5 like its pins) and two pins,
    // each alone in its company: they vote for it. Beside the part's faces:
    // strangers (like no pin) and, in one frame, a face 0.9 like the pins.
    // That face says the pins are someone beside the part, however many
    // strangers are there too: not held. Without it the strangers alone say
    // nothing: held.
    let part = [left_as(0, leaning(0.5, 1), None, Some(1)), left_as(100, leaning(0.5, 1), None, Some(1))];
    let pins = [left_as(1000, leaning(1.0, 1), Some(1), Some(1)), left_as(2000, leaning(1.0, 1), Some(1), Some(1))];
    let strangers = (3..7).map(|axis| left_as(if axis % 2 == 0 { 0 } else { 100 }, leaning(0.0, axis), None, None));
    let mut tracks: Vec<TrackIn> = part.into_iter().chain(pins).chain(strangers).collect();
    let held = |tracks: &[TrackIn]| {
        let company: Vec<usize> = (0..tracks.len()).collect();
        let around = Beside::new(tracks, (0..tracks.len()).map(|i| (i, usize::from(i >= 2))));
        let votes = pins_and_their_company(tracks, &[2, 3], 1, &company);
        held_by_pins(tracks, &[0, 1], 0, &votes, &around, &[], &Params::default())
    };
    assert!(held(&tracks), "strangers beside the part");
    tracks.push(left_as(0, leaning(0.9, 2), None, None));
    assert!(!held(&tracks), "a face like the pins beside the part");
}

#[test]
fn one_shared_frame_between_two_looks_lets_no_look_alike_in() {
    // Character 1 is pinned in two unlike looks; one face of each look is
    // in one frame (a screen, a poster, a face tracked wrong once). Someone
    // like the second look (character 5) is seen in frames of the first.
    // One frame does not make the two looks two people who share the
    // screen: the look-alike stays 5. (Review 7.)
    let mut rng = Rng(87);
    let (mut tracks, [first, second]) = two_pinned_looks(&mut rng);
    let at = tracks[first[14]].start;
    let seen = tracks[first[14]].seen.clone();
    (tracks[second[14]].start, tracks[second[14]].end, tracks[second[14]].seen) = (at, at + 48, seen);
    let other = around(&mut rng, &mean_face(&tracks, &second), 0.8);
    let others: Vec<usize> = (0..12)
        .map(|i| {
            let b = tracks[first[i]].start;
            tracks.push(TrackIn { previous: Some(5), ..track(0, b + 24, around(&mut rng, &other, 0.85), 0.8) });
            tracks.len() - 1
        })
        .collect();
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters[one_character(&o, &others)].key, Some(5));
}

/// B (60 faces, pinned to 1) merged into A (40 faces, the five most typical
/// pinned to 1: they start among B's); and someone like A (12 faces), on 1
/// before where A was not on screen. With `media` 2 the look-alike is in no
/// frame with anyone of 1; with 1, beside B's faces. Returns the tracks, A
/// and the look-alike.
fn a_merge_and_a_look_alike_never_beside_its_own(
    rng: &mut Rng,
    media: u32,
    alike: f32,
    l_alike: f32,
) -> (Vec<TrackIn>, Vec<usize>, Vec<usize>) {
    let b_c = rng.unit();
    let a_c = around(rng, &b_c, alike);
    let l_c = around(rng, &a_c, l_alike);
    let mut tracks = Vec::new();
    for i in 0..40u32 {
        let pin = (i < 5).then_some(1);
        let f = if i < 5 { 0.95 } else { 0.70 };
        tracks.push(TrackIn { pin, previous: Some(1), ..track(0, i * 1000, around(rng, &a_c, f), 0.8) });
    }
    for i in 0..60u32 {
        tracks.push(TrackIn { pin: Some(1), previous: Some(1), ..track(1, i * 1000, around(rng, &b_c, 0.85), 0.8) });
    }
    let l: Vec<usize> = (0..12u32)
        .map(|i| {
            tracks.push(TrackIn { previous: Some(1), ..track(media, i * 1000, around(rng, &l_c, 0.85), 0.8) });
            tracks.len() - 1
        })
        .collect();
    (tracks, (0..40).collect(), l)
}

#[test]
fn pins_do_not_hold_a_look_alike_that_was_never_beside_the_character() {
    // The pins are less like the look-alike than a face must be to be
    // placed on a character: A stays on 1, the look-alike goes on as a
    // character of its own. (Review 7.)
    for media in [2u32, 1] {
        let mut rng = Rng(19140);
        let (tracks, a, l) = a_merge_and_a_look_alike_never_beside_its_own(&mut rng, media, 0.40, 0.45);
        let o = identify(&tracks, &Params::default());
        assert_eq!(o.characters[one_character(&o, &a)].key, Some(1));
        assert!(are_two(&o, &a, &l), "media {media}");
    }
}

#[test]
fn pins_hold_the_part_of_the_character_they_are_most_like() {
    // As above, with the anime bars (a face is placed as soon as it is over
    // the link bar), and a look-alike the pins are about that like: they
    // vote for both parts, but A's faces are more like them, and too unlike
    // the look-alike to be one person with it. A stays on 1, the look-alike
    // does not.
    for media in [2u32, 1] {
        let mut rng = Rng(19141);
        let (tracks, a, l) = a_merge_and_a_look_alike_never_beside_its_own(&mut rng, media, 0.55, 0.60);
        let o = identify(&tracks, &Params::anime());
        let on = |i: usize| o.tracks[i].0.character().is_some_and(|c| o.characters[c].key == Some(1));
        let (a_on, l_on) = (a.iter().filter(|&&i| on(i)).count(), l.iter().filter(|&&i| on(i)).count());
        assert_eq!((a_on, l_on), (40, 0), "media {media}");
    }
}

#[test]
fn pins_hold_the_part_of_the_character_they_are_most_like_also_where_its_faces_are_unclear() {
    // As above (anime bars), but A is mostly seen from afar: besides the
    // pins, 13 clear faces too vague to group and 30 unclear ones (no
    // clustering takes them), typical of A. Only the unclear ones show that
    // the look-alike is not the pins' person: its vague faces are a part of
    // A too, unclear or not.
    let (clear, far, unclear) = (13, 0.4, 30);
    for media in [2u32, 1] {
        let mut rng = Rng(19141);
        let b_c = rng.unit();
        let a_c = around(&mut rng, &b_c, 0.55);
        let l_c = around(&mut rng, &a_c, 0.60);
        let mut tracks = Vec::new();
        for i in 0..5u32 {
            tracks.push(TrackIn {
                pin: Some(1),
                previous: Some(1),
                ..track(0, i * 1000, around(&mut rng, &a_c, 0.95), 0.8)
            });
        }
        for i in 5..5 + clear {
            tracks.push(TrackIn { previous: Some(1), ..track(0, i * 1000, around(&mut rng, &a_c, far), 0.8) });
        }
        for i in 5 + clear..5 + clear + unclear {
            tracks.push(TrackIn { previous: Some(1), ..track(0, i * 1000, around(&mut rng, &a_c, 0.90), 0.1) });
        }
        let a: Vec<usize> = (0..tracks.len()).collect();
        for i in 0..60u32 {
            tracks.push(TrackIn {
                pin: Some(1),
                previous: Some(1),
                ..track(1, i * 1000, around(&mut rng, &b_c, 0.85), 0.8)
            });
        }
        let l: Vec<usize> = (0..12u32)
            .map(|i| {
                tracks.push(TrackIn { previous: Some(1), ..track(media, i * 1000, around(&mut rng, &l_c, 0.85), 0.8) });
                tracks.len() - 1
            })
            .collect();
        let o = identify(&tracks, &Params::anime());
        let on = |i: usize| o.tracks[i].0.character().is_some_and(|c| o.characters[c].key == Some(1));
        let (a_on, l_on) = (a.iter().filter(|&&i| on(i)).count(), l.iter().filter(|&&i| on(i)).count());
        assert_eq!(l_on, 0, "media {media}");
        assert!(a_on >= 5 + unclear as usize, "media {media}: {a_on} of A's faces on 1");
    }
}

#[test]
fn a_bin_of_extras_seen_twice_does_not_take_the_room_of_a_run() {
    // One person in two episodes, and sixty extras, each seen twice, that
    // the user collected in one character: sixty looks pinned to it. The
    // clustering runs per media first (a big project; here forced by small
    // levels): the person's two groups must still meet across the episodes.
    let mut rng = Rng(78);
    let person = rng.unit();
    let mut tracks = Vec::new();
    for m in 0..2u32 {
        for i in 0..12u32 {
            tracks.push(track(m, i * 100, around(&mut rng, &person, 0.85), 0.8));
        }
    }
    let x: Vec<usize> = (0..tracks.len()).collect();
    for i in 0..60u32 {
        let extra = rng.unit();
        for m in 0..2u32 {
            tracks.push(TrackIn { pin: Some(9), ..track(m, 10_000 + i * 100, around(&mut rng, &extra, 0.95), 0.8) });
        }
    }
    let core: Vec<usize> = (0..tracks.len()).collect();
    let levels = crate::ahc::Levels { single: 10, second: 40 };
    let groups = crate::ahc::cluster_in(&tracks, &core, &Params::default(), levels);
    assert!(groups.iter().any(|g| x.iter().all(|i| g.contains(i))), "one group of the person across both episodes");
    let pinned: usize = groups.iter().filter(|g| tracks[g[0]].pin.is_some()).map(Vec::len).sum();
    assert_eq!(pinned, 120);
}

#[test]
fn two_hundred_looks_of_one_character_whose_first_faces_come_first_take_their_new_faces() {
    // Two hundred looks of one character (five pinned faces each, one per
    // episode: the first face of every look comes before the second of
    // any), and a new episode with ten faces of every look. All are the
    // character's: the first faces wait for their second ones.
    let mut rng = Rng(31378);
    let centres: Vec<Vec<f32>> = (0..200).map(|_| rng.unit()).collect();
    let mut tracks = Vec::new();
    for ep in 0..5u32 {
        for (l, c) in centres.iter().enumerate() {
            tracks.push(TrackIn {
                pin: Some(1),
                previous: Some(1),
                ..track(ep, l as u32 * 100, around(&mut rng, c, 0.85), 0.8)
            });
        }
    }
    let first_new = tracks.len();
    for (l, c) in centres.iter().enumerate() {
        for i in 0..10u32 {
            tracks.push(track(100, (l as u32 * 10 + i) * 100, around(&mut rng, c, 0.85), 0.8));
        }
    }
    let o = identify(&tracks, &Params::default());
    assert_eq!(o.characters.len(), 1, "new faces of {} looks are other characters", o.characters.len() - 1);
    assert!((first_new..tracks.len()).all(|i| o.tracks[i].0.character().is_some()));
}

/// Character 1 pinned in two unlike looks of `per` faces each (the second after the first), someone like
/// the second look (character 5) beside twelve faces of the first, and the faces of the second look in
/// `share` that are moved into frames of the first look's faces `with` (each to the frame of the one at
/// the same place; a face that is longer than one frame of the first look reaches several). Returns
/// whether the look-alike stays 5.
fn look_alike_stays(per: u32, share: &[(usize, usize, u32)]) -> bool {
    look_alike_stays_beside([per, per], share)
}

/// `look_alike_stays` with looks of `per[0]` and `per[1]` faces.
fn look_alike_stays_beside(per: [u32; 2], share: &[(usize, usize, u32)]) -> bool {
    let mut rng = Rng(88);
    let centres = [rng.unit(), rng.unit()];
    let mut tracks = Vec::new();
    let mut looks = [Vec::new(), Vec::new()];
    for i in 0..per[0].max(per[1]) {
        for s in (0..2).filter(|&s| i < per[s]) {
            looks[s].push(tracks.len());
            let t = track(0, s as u32 * 100_000 + i * 1000, around(&mut rng, &centres[s], 0.85), 0.8);
            tracks.push(TrackIn { pin: Some(1), previous: Some(1), ..t });
        }
    }
    for &(of_second, at_first, long) in share {
        let at = tracks[looks[0][at_first]].start;
        let t = &mut tracks[looks[1][of_second]];
        (t.start, t.end) = (at, at + long);
        t.seen = (at..at + long).filter(|f| f % 3 == 0).collect();
    }
    let other = around(&mut rng, &mean_face(&tracks, &looks[1]), 0.8);
    let others: Vec<usize> = (0..12)
        .map(|i| {
            let b = tracks[looks[0][i]].start;
            tracks.push(TrackIn { previous: Some(5), ..track(0, b + 24, around(&mut rng, &other, 0.85), 0.8) });
            tracks.len() - 1
        })
        .collect();
    let o = identify(&tracks, &Params::default());
    o.characters[one_character(&o, &others)].key == Some(5)
}

#[test]
fn two_looks_share_the_screen_only_when_each_has_faces_beside_the_other() {
    // One face of the second look, long (on a screen through a scene),
    // shares frames with three faces of the first: three on one side, one
    // on the other. Not two people who share the screen: the look-alike
    // stays 5. Three faces of each beside three of the other are.
    assert!(look_alike_stays(15, &[(14, 12, 2500)]), "one long face beside three");
    assert!(!look_alike_stays(15, &[(12, 12, 48), (13, 13, 48), (14, 14, 48)]), "three beside three");
}

#[test]
fn big_looks_share_the_screen_only_in_a_tenth_of_their_faces() {
    // Two looks of forty faces each: two faces of each beside two of the
    // other are not a tenth of either, so not two people who share the
    // screen; four are.
    assert!(look_alike_stays(40, &[(38, 38, 48), (39, 39, 48)]), "two of forty");
    assert!(!look_alike_stays(40, &[(36, 36, 48), (37, 37, 48), (38, 38, 48), (39, 39, 48)]), "four of forty");
}

#[test]
fn two_looks_share_the_screen_in_two_faces_each_up_to_twenty() {
    // Up to twenty faces a tenth is two or less: two faces of each look
    // beside two of the other are two people who share the screen. Of
    // twenty-five, a tenth rounds up to three.
    let two = [(13, 13, 48), (14, 14, 48)];
    assert!(!look_alike_stays(15, &two), "two of fifteen");
    assert!(look_alike_stays(25, &[(23, 23, 48), (24, 24, 48)]), "two of twenty-five");
    assert!(!look_alike_stays(25, &[(22, 22, 48), (23, 23, 48), (24, 24, 48)]), "three of twenty-five");
}

#[test]
fn the_tenth_is_of_the_smaller_look() {
    // Looks of forty and fifteen faces: two of each beside two of the other
    // are a tenth of the smaller one, so two people who share the screen.
    assert!(!look_alike_stays_beside([40, 15], &[(13, 38, 48), (14, 39, 48)]));
}

#[test]
fn a_merged_pair_takes_in_a_double_of_the_one_that_stays_only_when_they_share_the_screen_enough() {
    // B (40 faces, all pinned) merged into A (60, five pinned); C is a
    // double of A (twelve faces, character 7, split off by an older
    // grouping), one of its faces beside B. With A and B in four shots
    // together (a tenth of B's forty) they are two people who share the
    // screen, and C joins the character. In three, they are not: C's face
    // beside B could be a look-alike's, and C stays 7 (the price of not
    // letting a screen or a poster count; review 8).
    let c_is = |shared: usize| {
        let mut rng = Rng(8140);
        let (ca, cb) = (rng.unit(), rng.unit());
        let mut tracks = Vec::new();
        for i in 0..60u32 {
            let (pin, f) = if i < 5 { (Some(1), 0.92) } else { (None, 0.85) };
            tracks.push(TrackIn { pin, previous: Some(1), ..track(0, i * 1000, around(&mut rng, &ca, f), 0.8) });
        }
        let b: Vec<usize> = (0..40u32)
            .map(|i| {
                let t = track(0, 300_000 + i * 1000, around(&mut rng, &cb, 0.85), 0.8);
                tracks.push(TrackIn { pin: Some(1), previous: Some(1), ..t });
                tracks.len() - 1
            })
            .collect();
        let put = |tracks: &mut Vec<TrackIn>, i: usize, j: usize| {
            let (s, e, seen) = (tracks[j].start, tracks[j].end, tracks[j].seen.clone());
            (tracks[i].start, tracks[i].end, tracks[i].seen) = (s, e, seen);
        };
        for (i, &bi) in b.iter().enumerate().take(shared) {
            put(&mut tracks, bi, 10 + i);
        }
        let c: Vec<usize> = (0..12u32)
            .map(|i| {
                let t = track(0, 600_000 + i * 1000, around(&mut rng, &ca, 0.85), 0.8);
                tracks.push(TrackIn { previous: Some(7), ..t });
                tracks.len() - 1
            })
            .collect();
        put(&mut tracks, c[0], b[39]);
        let o = identify(&tracks, &Params::default());
        o.characters[one_character(&o, &c)].key
    };
    assert_eq!(c_is(4), Some(1), "four shots together");
    assert_eq!(c_is(3), Some(7), "three");
}

#[test]
fn pins_hold_a_part_only_as_like_it_as_a_face_placed_on_a_character() {
    // Two pins alone in their companies, 0.4 like a part of 1: over the
    // link bar (0.30), under the bar a face is placed at (0.45). Not held.
    let tracks = [
        left_as(0, leaning(0.4, 1), None, Some(1)),
        left_as(100, leaning(0.4, 1), None, Some(1)),
        left_as(200, leaning(1.0, 2), Some(1), Some(1)),
        left_as(300, leaning(1.0, 3), Some(1), Some(1)),
    ];
    assert!(!holds(&tracks, &[0, 1], &[2, 3], &[0, 0, 1, 2]));
    // 0.47 like it: over `assign`, so held; but not where the link bar is
    // higher (0.5), as in a fitted space.
    let tracks = [
        left_as(0, leaning(0.47, 1), None, Some(1)),
        left_as(100, leaning(0.47, 1), None, Some(1)),
        left_as(200, leaning(1.0, 2), Some(1), Some(1)),
        left_as(300, leaning(1.0, 3), Some(1), Some(1)),
    ];
    let held = |p: &Params| {
        let around = Beside::new(&tracks, std::iter::empty());
        let votes = pins_and_their_company(&tracks, &[2, 3], 1, &[0, 0, 1, 2]);
        held_by_pins(&tracks, &[0, 1], 0, &votes, &around, &[], p)
    };
    assert!(held(&Params::default()));
    assert!(!held(&Params { link: 0.5, ..Params::default() }));
}

#[test]
fn a_part_of_the_same_person_as_one_more_like_the_pins_is_held_too() {
    // A part of 1 (two faces, 0.5 like its pins) and another part of it
    // more like the pins. If the two are alike as one person's parts are
    // (0.92), the pins hold both; if not (0.28), the other part is the
    // pins' person, and this one someone else.
    let pins = [left_as(1000, leaning(1.0, 3), Some(1), Some(1)), left_as(2000, leaning(1.0, 3), Some(1), Some(1))];
    let held = |other: Vec<f32>| {
        let mut tracks = vec![left_as(0, leaning(0.5, 1), None, Some(1)), left_as(100, leaning(0.5, 1), None, Some(1))];
        tracks.extend(pins.iter().cloned());
        tracks.extend([left_as(3000, other.clone(), None, Some(1)), left_as(3100, other, None, Some(1))]);
        let company: Vec<usize> = (0..tracks.len()).collect();
        let around = Beside::new(&tracks, std::iter::empty());
        let votes = pins_and_their_company(&tracks, &[2, 3], 1, &company);
        held_by_pins(&tracks, &[0, 1], 0, &votes, &around, &[&[4, 5]], &Params::default())
    };
    assert!(held(leaning(0.8, 1)), "a part of the same person");
    assert!(!held(leaning(0.55, 2)), "a part of someone else");
    // 0.38 like the part: over the link bar (0.30), so one person with it,
    // though under the bar a face is placed at (0.45).
    let b = 0.08 / 0.75f32.sqrt();
    let c = (1.0 - 0.36 - b * b).sqrt();
    assert!(held(vec![0.6, b, c, 0.0, 0.0, 0.0, 0.0, 0.0]), "a part as like it as the link bar asks");
}
