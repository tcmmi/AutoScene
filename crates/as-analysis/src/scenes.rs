//! Scenes: which shots of an episode belong together, from what lies between
//! them and from how alike they are (their pictures, the people in them,
//! their colours). The method and its numbers are in `bench/scenes`; the
//! bench (`as-project`'s `scenes_bench`) scores this very code.
//!
//! Every gap between two shots gets a score, 0..1, for a new scene there;
//! scenes come from a threshold on it, so a higher threshold only joins
//! scenes and never reshuffles them.

use crate::FrameSignals;

/// Frames of a shot whose colour signatures stand for it.
pub const KEYS: u32 = 5;
/// Frames of a shot whose pictures stand for it.
pub const PICTURES: u32 = 3;
/// The most shots a link between two alike shots may reach over.
pub const REACH: usize = 24;

/// What a stretch of a media is, as far as scenes go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    Shot,
    Dissolve,
    /// Black, or a fade to or from it.
    Dark,
    /// Anything else (a flash inside a shot).
    Other,
}

/// What lies between two shots that follow each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Between {
    Cut,
    /// A dissolve.
    Blend,
    /// Black, or a fade to or from it.
    Dark,
}

#[derive(Clone, Debug, Default)]
pub struct Shot {
    pub start: u32,
    pub end: u32,
    /// Frames whose colour signatures stand for the shot.
    pub keys: Vec<u32>,
    /// The characters the cast shows in it, sorted.
    pub cast: Vec<usize>,
}

impl Shot {
    /// The shot of frames `[start, end)`, nobody known in it yet.
    pub fn new(start: u32, end: u32) -> Self {
        let len = end - start;
        let n = len.min(KEYS);
        let keys = (0..n).map(|k| start + (2 * k + 1) * len / (2 * n)).collect();
        Self { start, end, keys, cast: Vec::new() }
    }
}

/// The frames of shot `[start, end)` whose pictures stand for it: at 1/6,
/// 1/2 and 5/6 of it (fewer, when it has fewer frames).
pub fn picture_frames(start: u32, end: u32) -> Vec<u32> {
    let len = end - start;
    let mut frames: Vec<u32> = (0..PICTURES).map(|k| start + (2 * k + 1) * len / (2 * PICTURES)).collect();
    frames.dedup();
    frames
}

/// The shots of a media and what lies between each two, from its pieces in
/// order.
pub fn shots(pieces: impl IntoIterator<Item = (u32, u32, Piece)>) -> (Vec<Shot>, Vec<Between>) {
    let mut shots: Vec<Shot> = Vec::new();
    let mut between = Vec::new();
    let mut gap = Between::Cut;
    for (start, end, piece) in pieces {
        match piece {
            Piece::Shot => {
                if !shots.is_empty() {
                    between.push(gap);
                }
                gap = Between::Cut;
                shots.push(Shot::new(start, end));
            }
            Piece::Dissolve if gap == Between::Cut => gap = Between::Blend,
            Piece::Dark => gap = Between::Dark,
            Piece::Dissolve | Piece::Other => {}
        }
    }
    (shots, between)
}

/// The shot a frame lies in.
pub fn shot_at(shots: &[Shot], frame: u32) -> Option<usize> {
    let i = shots.partition_point(|s| s.start <= frame).checked_sub(1)?;
    (frame < shots[i].end).then_some(i)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// How many shots on a link between two alike shots may reach.
    pub window: usize,
    /// What each shot in between leaves of a link.
    pub decay: f32,
    /// The colour distance at which two shots have nothing in common.
    pub far: f32,
    /// What black, or a fade, leaves of a link across it; and a dissolve.
    pub dark: f32,
    pub blend: f32,
    /// The weights of the colours and of the cast (a picture embedding
    /// brings its own).
    pub colour: f32,
    pub cast: f32,
    /// A piece of a scene shorter than this many seconds joins its
    /// neighbour more readily (0: no such rule), by this power of its share.
    pub short: f32,
    pub curve: f32,
}

impl Params {
    /// What a project stores without any pass of its own: colours and cast.
    pub const DEFAULT: Self =
        Self { window: 8, decay: 0.9, far: 0.2, dark: 0.5, blend: 0.8, colour: 0.7, cast: 0.3, short: 0.0, curve: 1.0 };
}

/// How scenes are read for a kind of footage: the settings, the weight of
/// the shots' pictures, and the score from which a gap is a new scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipe {
    pub params: Params,
    pub picture: f32,
    pub threshold: f32,
}

impl Recipe {
    /// Real life (`bench/models/DECISIONS.md`, row 8): the pictures and the
    /// cast; pieces under 40 s join first.
    pub const LIVE: Self = Self {
        params: Params { colour: 0.0, cast: 0.25, short: 40.0, ..Params::DEFAULT },
        picture: 1.0,
        threshold: 0.675,
    };
    /// Drawn and rendered footage (row 9, provisional): the colours beside
    /// the pictures; pieces under 60 s join first.
    pub const DRAWN: Self = Self {
        params: Params { colour: 1.0, cast: 0.25, short: 60.0, ..Params::DEFAULT },
        picture: 1.0,
        threshold: 0.6,
    };

    /// Whether the frames' colour signatures are read at all.
    pub fn reads_colours(&self) -> bool {
        self.params.colour > 0.0
    }
}

/// Colour distance of two shots, 0..1: that of their closest pair of frames,
/// so that a shot which moves is still like the shot it ends on.
pub fn distance(a: &Shot, b: &Shot, sig: &[FrameSignals]) -> f32 {
    let mut best = 1f32;
    for &x in &a.keys {
        for &y in &b.keys {
            best = best.min(sig[x as usize].color_distance(&sig[y as usize]));
        }
    }
    best
}

/// The share of the characters of two shots that both have; `None` when
/// either shows nobody the cast knows.
pub fn shared_cast(a: &[usize], b: &[usize]) -> Option<f32> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let both = a.iter().filter(|c| b.contains(c)).count();
    Some(both as f32 / (a.len() + b.len() - both) as f32)
}

/// How alike the pictures of two shots are: the cosine of their closest
/// pair of frames. `None` when a shot has no vector.
pub fn closest(a: &[Vec<f32>], b: &[Vec<f32>]) -> Option<f32> {
    a.iter()
        .flat_map(|x| b.iter().map(move |y| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>()))
        .max_by(f32::total_cmp)
}

/// A picture embedding's say on every pair of shots within [`REACH`].
pub struct Cue {
    pub name: String,
    pub weight: f32,
    /// The cosine two shots far apart have: nothing in common.
    pub floor: f32,
    /// By [`Pairs::at`]; NaN where a shot has no vector.
    pub cos: Vec<f32>,
}

/// What is known of every pair of shots within [`REACH`] of each other.
pub struct Pairs {
    /// Colour distance; NaN where the colours were not read.
    pub colour: Vec<f32>,
    /// Shared cast; NaN where a shot shows nobody the cast knows.
    pub cast: Vec<f32>,
    pub cues: Vec<Cue>,
}

impl Pairs {
    pub fn at(i: usize, j: usize) -> usize {
        i * REACH + (j - i - 1)
    }

    fn fill(n: usize, of: impl Fn(usize, usize) -> Option<f32>) -> Vec<f32> {
        let mut v = vec![f32::NAN; n * REACH];
        for i in 0..n {
            for j in i + 1..n.min(i + 1 + REACH) {
                v[Self::at(i, j)] = of(i, j).unwrap_or(f32::NAN);
            }
        }
        v
    }

    /// The colours (of the frames' signatures `sig`, when given) and the cast
    /// of every pair of shots.
    pub fn new(shots: &[Shot], sig: Option<&[FrameSignals]>) -> Self {
        let n = shots.len();
        Self {
            colour: Self::fill(n, |i, j| sig.map(|sig| distance(&shots[i], &shots[j], sig))),
            cast: Self::fill(n, |i, j| shared_cast(&shots[i].cast, &shots[j].cast)),
            cues: Vec::new(),
        }
    }

    /// Adds a picture embedding: per shot, the unit vectors of its frames.
    pub fn add(&mut self, name: &str, weight: f32, vectors: &[Vec<Vec<f32>>]) {
        let n = vectors.len();
        // What shots have in common that have nothing in common: the median
        // over pairs at least 100 shots apart.
        let mut apart: Vec<f32> = (0..n)
            .flat_map(|i| (i + 100..n).step_by(100).filter_map(move |j| closest(&vectors[i], &vectors[j])))
            .collect();
        apart.sort_by(f32::total_cmp);
        let floor = apart.get(apart.len() / 2).copied().unwrap_or(0.0);
        let cos = Self::fill(n, |i, j| closest(&vectors[i], &vectors[j]));
        self.cues.push(Cue { name: name.to_owned(), weight, floor, cos });
    }

    /// What each cue says of shots `i < j`, 0..1, with its weight: the
    /// colours, the cast, then each embedding. A cue that knows nothing of
    /// the pair is left out.
    pub fn says(&self, i: usize, j: usize, p: &Params) -> impl Iterator<Item = (f32, f32)> {
        let k = Self::at(i, j);
        let colour = (1.0 - self.colour[k] / p.far).clamp(0.0, 1.0);
        let cues = self.cues.iter().map(move |c| (((c.cos[k] - c.floor) / (1.0 - c.floor)).clamp(0.0, 1.0), c.weight));
        [(colour, p.colour), (self.cast[k], p.cast)].into_iter().chain(cues).filter(|(v, _)| !v.is_nan())
    }

    /// How alike shots `i < j` are, 0..1: the weighted mean of the cues.
    pub fn alike(&self, i: usize, j: usize, p: &Params) -> f32 {
        let (sum, weight) = self.says(i, j, p).fold((0.0, 0.0), |(s, w), (v, x)| (s + v * x, w + x));
        if weight > 0.0 { sum / weight } else { 0.0 }
    }
}

/// One score per gap between two shots, 0..1: how much speaks for a new scene
/// there. Two alike shots hold everything between them together (a shot
/// between two alike shots belongs with them); the link weakens with each
/// shot it reaches over and with a fade or a dissolve on its way. Scenes come
/// from a threshold, so a higher one only joins scenes.
pub fn boundary_scores(pairs: &Pairs, between: &[Between], p: &Params) -> Vec<f32> {
    let n = between.len() + 1;
    let mut link = vec![0f32; between.len()];
    for i in 0..n {
        let mut keep = 1f32;
        for j in i + 1..n.min(i + 1 + p.window) {
            keep *= match between[j - 1] {
                Between::Cut => 1.0,
                Between::Blend => p.blend,
                Between::Dark => p.dark,
            };
            if j > i + 1 {
                keep *= p.decay;
            }
            let s = pairs.alike(i, j, p) * keep;
            for l in &mut link[i..j] {
                *l = l.max(s);
            }
        }
    }
    link.into_iter().map(|l| 1.0 - l).collect()
}

/// Joins the shots into ever longer pieces, the weakest boundary first, and
/// gives each gap the score at which it went. A boundary beside a piece
/// shorter than [`Params::short`] counts for less (a cutaway, the first shot
/// of a scene), and for more again once that piece has grown. The scores
/// rise from join to join, so a threshold on them still only joins scenes.
pub fn settle(base: &[f32], secs: &[f32], p: &Params) -> Vec<f32> {
    let n = base.len();
    if p.short <= 0.0 {
        return base.to_vec();
    }
    // The gaps still open, as a list: `before[g]` and `after[g]` are the
    // open gaps around gap g (n: none), so a piece is the shots between two.
    let mut before: Vec<usize> = (0..n).map(|g| g.checked_sub(1).unwrap_or(n)).collect();
    let mut after: Vec<usize> = (0..n).map(|g| if g + 1 < n { g + 1 } else { n }).collect();
    let mut open = vec![true; n];
    let upto: Vec<f32> = std::iter::once(0.0)
        .chain(secs.iter().scan(0.0, |sum, s| {
            *sum += s;
            Some(*sum)
        }))
        .collect();
    let cost = |g: usize, before: &[usize], after: &[usize]| {
        let left = upto[g + 1] - if before[g] == n { 0.0 } else { upto[before[g] + 1] };
        let right = if after[g] == n { upto[n + 1] } else { upto[after[g] + 1] } - upto[g + 1];
        base[g] * (left.min(right) / p.short).min(1.0).powf(p.curve)
    };
    let mut out = vec![0f32; n];
    let mut level = 0f32;
    for _ in 0..n {
        let (g, c) = (0..n)
            .filter(|&g| open[g])
            .map(|g| (g, cost(g, &before, &after)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .expect("an open gap");
        level = level.max(c);
        out[g] = level;
        open[g] = false;
        if before[g] != n {
            after[before[g]] = after[g];
        }
        if after[g] != n {
            before[after[g]] = before[g];
        }
    }
    out
}

/// The scene of each shot, for a cut at every gap marked.
pub fn scene_ids(cuts: &[bool]) -> Vec<usize> {
    let mut ids = vec![0; cuts.len() + 1];
    for (g, &cut) in cuts.iter().enumerate() {
        ids[g + 1] = ids[g] + usize::from(cut);
    }
    ids
}

/// The scene of each shot, counted from 0 in the media's order.
///
/// `secs` is each shot's length in seconds, `pictures` the unit vectors of
/// each shot's frames ([`picture_frames`]), `sig` the frames' signatures
/// (read only when the recipe [reads colours](Recipe::reads_colours)).
pub fn scenes(
    shots: &[Shot],
    between: &[Between],
    secs: &[f32],
    sig: Option<&[FrameSignals]>,
    pictures: &[Vec<Vec<f32>>],
    recipe: &Recipe,
) -> Vec<usize> {
    if shots.is_empty() {
        return Vec::new();
    }
    let mut pairs = Pairs::new(shots, sig.filter(|_| recipe.reads_colours()));
    pairs.add("pictures", recipe.picture, pictures);
    let scores = settle(&boundary_scores(&pairs, between, &recipe.params), secs, &recipe.params);
    let cuts: Vec<bool> = scores.iter().map(|&s| s > recipe.threshold).collect();
    scene_ids(&cuts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals;

    /// Shots of ten frames each; `look[i]` picks the colours, `cast[i]` the people.
    fn film(look: &[u8], cast: &[&[usize]]) -> (Vec<Shot>, Vec<FrameSignals>) {
        let mut sig = Vec::new();
        let shots = look
            .iter()
            .zip(cast)
            .enumerate()
            .map(|(i, (&l, c))| {
                let mut colors = [0u8; signals::COLORS];
                for ch in 0..3 {
                    colors[ch * signals::BINS + l as usize] = 255;
                }
                sig.extend(std::iter::repeat_n(FrameSignals { colors, ..Default::default() }, 10));
                let start = i as u32 * 10;
                Shot { start, end: start + 10, keys: vec![start + 5], cast: c.to_vec() }
            })
            .collect();
        (shots, sig)
    }

    #[test]
    fn a_shot_between_two_alike_shots_belongs_with_them() {
        // A B A B | C D C: the answers and the cutaways stay in their scenes.
        let (shots, sig) = film(&[0, 5, 0, 5, 9, 12, 9], &[&[1], &[2], &[1], &[2], &[3], &[4], &[3]]);
        let between = vec![Between::Cut; 6];
        let s = boundary_scores(&Pairs::new(&shots, Some(&sig)), &between, &Params::DEFAULT);
        assert!(s[3] > 0.8, "the two scenes share nothing: {s:?}");
        for g in [0, 1, 2, 4, 5] {
            assert!(s[g] < 0.2, "gap {g} lies between two alike shots: {s:?}");
        }
        // No reach: every change of picture and people is a boundary.
        let alone =
            boundary_scores(&Pairs::new(&shots, Some(&sig)), &between, &Params { window: 1, ..Params::DEFAULT });
        assert!(alone.iter().all(|&x| x > 0.8), "{alone:?}");
    }

    #[test]
    fn a_fade_weakens_the_link_across_it() {
        let (shots, sig) = film(&[0, 0, 0, 0], &[&[], &[], &[], &[]]);
        let plain = boundary_scores(&Pairs::new(&shots, Some(&sig)), &[Between::Cut; 3], &Params::DEFAULT);
        let faded = boundary_scores(
            &Pairs::new(&shots, Some(&sig)),
            &[Between::Cut, Between::Dark, Between::Cut],
            &Params::DEFAULT,
        );
        assert!(faded[1] > plain[1] + 0.3, "{plain:?} {faded:?}");
        assert_eq!((faded[0], faded[2]), (plain[0], plain[2]), "the gaps beside it keep their own links");
    }

    #[test]
    fn a_short_piece_joins_a_neighbour() {
        let p = Params { short: 30.0, ..Params::DEFAULT };
        // A cutaway of 3 s between two long shots, unlike both: it joins one
        // of them, and the boundary that is left stands as it was.
        assert_eq!(settle(&[0.8, 0.8], &[60.0, 3.0, 60.0], &p), [0.8 * 3.0 / 30.0, 0.8]);
        // Long pieces on both sides: nothing changes.
        assert_eq!(settle(&[0.7, 0.4], &[60.0, 45.0, 90.0], &p), [0.7, 0.4]);
        // Without the rule the scores are the links' own.
        assert_eq!(settle(&[0.8, 0.8], &[60.0, 3.0, 60.0], &Params::DEFAULT), [0.8, 0.8]);
        // Three short shots in a row grow into one piece of 15 s. It joins
        // one neighbour at half the boundary's score; what is left stands in
        // full, and the scores never fall from join to join.
        let s = settle(&[0.9, 0.5, 0.5, 0.9], &[60.0, 5.0, 5.0, 5.0, 60.0], &p);
        assert!(s[1] < 0.1 && s[2] < 0.1, "the short shots join each other first: {s:?}");
        assert_eq!((s[0], s[3]), (0.45, 0.9));
    }

    #[test]
    fn a_higher_threshold_only_joins_scenes() {
        // Scores as the join of short pieces leaves them, at thresholds
        // between each two of them.
        let p = Params { short: 30.0, ..Params::DEFAULT };
        let s = settle(&[0.9, 0.5, 0.2, 0.7, 0.9, 0.3], &[60.0, 5.0, 20.0, 5.0, 40.0, 3.0, 50.0], &p);
        let cuts = |t: f32| s.iter().map(|&x| x > t).collect::<Vec<_>>();
        let mut levels = s.clone();
        levels.sort_by(f32::total_cmp);
        for pair in levels.windows(2) {
            let (fine, coarse) = (scene_ids(&cuts(pair[0] - 0.001)), scene_ids(&cuts(pair[1] - 0.001)));
            for a in 0..fine.len() {
                for b in 0..fine.len() {
                    assert!(fine[a] != fine[b] || coarse[a] == coarse[b], "shots {a} and {b} were parted: {s:?}");
                }
            }
        }
    }

    #[test]
    fn pieces_become_shots_and_what_lies_between() {
        let (shots, between) = shots([
            (0, 10, Piece::Shot),
            (10, 14, Piece::Dissolve),
            (14, 30, Piece::Shot),
            (30, 31, Piece::Other),
            (31, 40, Piece::Shot),
            (40, 44, Piece::Dissolve),
            (44, 50, Piece::Dark),
            (50, 60, Piece::Shot),
        ]);
        assert_eq!(shots.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(), [(0, 10), (14, 30), (31, 40), (50, 60)]);
        assert_eq!(between, [Between::Blend, Between::Cut, Between::Dark]);
        assert_eq!(shot_at(&shots, 14), Some(1));
        assert_eq!(shot_at(&shots, 12), None);
        assert_eq!(picture_frames(0, 60), [10, 30, 50]);
        assert_eq!(picture_frames(7, 8), [7], "a shot of one frame has one picture");
    }

    #[test]
    fn scenes_follow_the_pictures() {
        // Two places, three shots each, 20 s a shot: the pictures of a place
        // point one way, those of the other place another.
        let shots: Vec<Shot> = (0..6).map(|i| Shot::new(i * 480, (i + 1) * 480)).collect();
        let place = |i: usize| if i < 3 { vec![1.0, 0.0] } else { vec![0.0, 1.0] };
        let pictures: Vec<Vec<Vec<f32>>> = (0..6).map(|i| vec![place(i)]).collect();
        let ids = scenes(&shots, &[Between::Cut; 5], &[20.0; 6], None, &pictures, &Recipe::LIVE);
        assert_eq!(ids, [0, 0, 0, 1, 1, 1]);
        assert!(scenes(&[], &[], &[], None, &[], &Recipe::LIVE).is_empty());
        assert_eq!(scenes(&shots[..1], &[], &[20.0], None, &pictures[..1], &Recipe::LIVE), [0]);
    }
}
