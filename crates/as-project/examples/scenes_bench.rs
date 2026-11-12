//! Dev tool: scenes from what a project already stores, against scene labels
//! (`bench/scenes`). Step A of the scenes slot (`bench/models/DECISIONS.md`,
//! row 6): no new pass over the video. The shots, the characters seen in each
//! shot and the frames' colour signatures are read from a COPY of a project
//! (`project.db` and its `-wal` in a folder of their own).
//!
//! `scenes_bench labels <serial_speakers.json> <season> <episode> <project_copy_dir>`
//!   fits the label file's times to the project's frames by the shot cuts and
//!   prints the scene labels (`frame  source_s  speakers`).
//! `scenes_bench score <project_copy_dir> <scenes.tsv> <identity labels.tsv>`
//!   scores the scenes: the boundaries, the scenes' overlap, and per main
//!   character the shots of their scenes (see `bench/scenes/README.md`).
//!   `-` for the identity labels takes the project's own cast as who is
//!   where (a title without such labels).
//! `scenes_bench agree <project_copy_dir> <one.scenes.tsv> <other.scenes.tsv>`
//!   says how well two sets of scene labels agree, and where they differ.
//!
//! In place of a project copy, `score` and `agree` also take a folder of bench
//! inputs (`bb.segments.tsv`, `bb.sig`, `bb.info.json`, as the anime bench
//! makes them): an episode that is in no project. It has no stored cast, so
//! only `true cast` counts there.
//!
//! Scene labels name each scene's first shot by its first frame (`frame`) or
//! by the shot's number among the project's shots (`shot`), as the header says.
//!
//! Step B adds picture embeddings of a few frames per shot, made by
//! `tools/slot-bench/shot_embed.py`: `EMB=<weight>:<file>[,<weight>:<file>…]`.
//!
//! `MEDIA=<n>` picks the n-th media of a project with several (from 0).
//! `P=<window>,<decay>,<far>,<dark>,<blend>` tries other settings,
//! `W=<colour>,<cast>` other weights, `SHORT=<seconds>,<curve>` the rule for
//! short pieces, `AT=<threshold>` another threshold;
//! `SWEEP=1` lists a grid of settings, `STATS=1` how alike shots are inside
//! a scene and across a boundary, `DETAIL=1` every gap with its score,
//! `CURVE=1` the scores at every threshold (to choose one across titles).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use as_analysis::FrameSignals;
use as_analysis::signals;
use as_analysis::tracks::{FaceDet, dets_from_bytes};
use rusqlite::Connection;

/// Frames of a shot whose colour signatures stand for it.
const KEYS: u32 = 5;
/// A main character has at least this long on screen (as in `bench/identity`).
const MAIN_SEC: f64 = 30.0;

// -- what a project stores ---------------------------------------------------

struct Track {
    start: u32,
    end: u32,
    /// Index into [`Episode::characters`], when the cast shows the track there.
    character: Option<usize>,
    dets: Vec<FaceDet>,
}

struct Episode {
    fps: f64,
    /// `(start, end, kind)`, in order.
    segments: Vec<(u32, u32, String)>,
    signals: Vec<FrameSignals>,
    tracks: Vec<Track>,
    /// What the cast calls each character.
    characters: Vec<String>,
}

/// An episode that is in no project, from its bench inputs: the shots, the
/// frames' signals and the frame rate. Nobody is known in it.
fn load_inputs(dir: &Path) -> Episode {
    let read = |name: &str| std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{}/{name}: {e}", dir.display()));
    let segments = String::from_utf8(read("bb.segments.tsv"))
        .expect("segments")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[0].parse().unwrap(), f[1].parse().unwrap(), f[2].to_owned())
        })
        .collect();
    let info: serde_json::Value = serde_json::from_slice(&read("bb.info.json")).expect("media info");
    let rate = &info["video"]["avgFrameRate"];
    let fps = rate["num"].as_f64().expect("frame rate") / rate["den"].as_f64().expect("frame rate");
    let signals = signals::decode(&read("bb.sig")).expect("a signals file");
    Episode { fps, segments, signals, tracks: Vec::new(), characters: Vec::new() }
}

fn load(dir: &Path) -> Episode {
    let path = dir.join("project.db");
    if !path.is_file() && dir.join("bb.segments.tsv").is_file() {
        return load_inputs(dir);
    }
    assert!(path.is_file(), "{}: no project.db (a COPY of a project's folder)", dir.display());
    let db = Connection::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let media: Vec<(String, f64)> = db
        .prepare("SELECT id, CAST(fps_num AS REAL) / fps_den FROM media WHERE shots = 'ready' ORDER BY position, id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let nth = std::env::var("MEDIA").ok().map(|v| v.parse::<usize>().expect("MEDIA=<n>"));
    assert!(nth.is_some() || media.len() == 1, "{} media with shots: pick one with MEDIA=<n>", media.len());
    let (id, fps) = media.get(nth.unwrap_or(0)).expect("MEDIA: no such media").clone();

    let segments = db
        .prepare("SELECT start, end, kind FROM segments WHERE media_id = ?1 ORDER BY start")
        .unwrap()
        .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut all = Vec::new();
    let mut st = db.prepare("SELECT start, data FROM shot_signals WHERE media_id = ?1 ORDER BY start").unwrap();
    let mut rows = st.query([&id]).unwrap();
    while let Some(r) = rows.next().unwrap() {
        let (start, data): (u32, Vec<u8>) = (r.get(0).unwrap(), r.get(1).unwrap());
        assert_eq!(start as usize, all.len(), "the stored signals have a hole");
        all.extend(signals::decode(&data).expect("stored signals"));
    }

    let mut index = HashMap::new();
    let mut characters = Vec::new();
    let mut st = db.prepare("SELECT id, number, name FROM characters ORDER BY number").unwrap();
    let mut rows = st.query([]).unwrap();
    while let Some(r) = rows.next().unwrap() {
        let (cid, number, name): (String, u32, Option<String>) =
            (r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap());
        index.insert(cid, characters.len());
        characters.push(name.unwrap_or_else(|| format!("Character {number}")));
    }
    let mut tracks = Vec::new();
    let mut st = db
        .prepare(
            "SELECT start, end, character_id, assign, dets FROM tracks WHERE media_id = ?1 AND not_face = 0 ORDER BY start",
        )
        .unwrap();
    let mut rows = st.query([&id]).unwrap();
    while let Some(r) = rows.next().unwrap() {
        let (start, end, cid, assign, dets): (u32, u32, Option<String>, String, Vec<u8>) =
            (r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap(), r.get(3).unwrap(), r.get(4).unwrap());
        // The cast shows a face with its character when it is sure of it.
        let shown = matches!(assign.as_str(), "auto" | "pinned");
        tracks.push(Track {
            start,
            end,
            character: cid.filter(|_| shown).and_then(|c| index.get(&c).copied()),
            dets: dets_from_bytes(&dets).expect("stored detections"),
        });
    }
    Episode { fps, segments, signals: all, tracks, characters }
}

// -- shots -------------------------------------------------------------------

/// What lies between two shots that follow each other.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Between {
    Cut,
    /// A dissolve.
    Blend,
    /// Black, or a fade to or from it.
    Dark,
}

#[derive(Clone, Debug, Default)]
struct Shot {
    start: u32,
    end: u32,
    /// Frames whose colour signatures stand for the shot.
    keys: Vec<u32>,
    /// The characters the cast shows in it, sorted.
    cast: Vec<usize>,
}

fn shots_of(ep: &Episode) -> (Vec<Shot>, Vec<Between>) {
    let mut shots: Vec<Shot> = Vec::new();
    let mut between = Vec::new();
    let mut gap = Between::Cut;
    for (start, end, kind) in &ep.segments {
        match kind.as_str() {
            "shot" => {
                if !shots.is_empty() {
                    between.push(gap);
                }
                gap = Between::Cut;
                let len = end - start;
                let n = len.min(KEYS);
                let keys = (0..n).map(|k| start + (2 * k + 1) * len / (2 * n)).collect();
                shots.push(Shot { start: *start, end: *end, keys, cast: Vec::new() });
            }
            "dissolve" if gap == Between::Cut => gap = Between::Blend,
            "black" | "fade_in" | "fade_out" => gap = Between::Dark,
            _ => {}
        }
    }
    for t in &ep.tracks {
        if let (Some(c), Some(s)) = (t.character, shot_at(&shots, t.start))
            && !shots[s].cast.contains(&c)
        {
            shots[s].cast.push(c);
        }
    }
    for s in &mut shots {
        s.cast.sort_unstable();
    }
    (shots, between)
}

/// The shot a frame lies in.
fn shot_at(shots: &[Shot], frame: u32) -> Option<usize> {
    let i = shots.partition_point(|s| s.start <= frame).checked_sub(1)?;
    (frame < shots[i].end).then_some(i)
}

/// The shot whose first frame is nearest to `frame`.
fn shot_starting_near(shots: &[Shot], frame: u32) -> usize {
    (0..shots.len()).min_by_key(|&i| shots[i].start.abs_diff(frame)).expect("shots")
}

// -- scenes from colour, cast, pictures and what lies between -----------------

/// The most shots a link between two alike shots may reach over.
const REACH: usize = 24;

#[derive(Clone, Copy, Debug)]
struct Params {
    /// How many shots on a link between two alike shots may reach.
    window: usize,
    /// What each shot in between leaves of a link.
    decay: f32,
    /// The colour distance at which two shots have nothing in common.
    far: f32,
    /// What black, or a fade, leaves of a link across it; and a dissolve.
    dark: f32,
    blend: f32,
    /// The weights of the colours and of the cast (a picture embedding
    /// brings its own).
    colour: f32,
    cast: f32,
    /// A piece of a scene shorter than this many seconds joins its
    /// neighbour more readily (0: no such rule), by this power of its share.
    short: f32,
    curve: f32,
}

impl Params {
    const DEFAULT: Self =
        Self { window: 8, decay: 0.9, far: 0.2, dark: 0.5, blend: 0.8, colour: 0.7, cast: 0.3, short: 0.0, curve: 1.0 };

    fn from_env() -> Self {
        let list = |name: &str, n: usize, what: &str| {
            std::env::var(name).ok().map(|v| {
                let v: Vec<f32> =
                    v.split(',').map(|x| x.trim().parse().unwrap_or_else(|_| panic!("{name}={what}"))).collect();
                assert_eq!(v.len(), n, "{name}={what}");
                v
            })
        };
        let mut p = Self::DEFAULT;
        if let Some(v) = list("P", 5, "<window>,<decay>,<far>,<dark>,<blend>") {
            p = Self { window: v[0] as usize, decay: v[1], far: v[2], dark: v[3], blend: v[4], ..p };
        }
        if let Some(v) = list("W", 2, "<colour>,<cast>") {
            p = Self { colour: v[0], cast: v[1], ..p };
        }
        if let Some(v) = list("SHORT", 2, "<seconds>,<curve>") {
            p = Self { short: v[0], curve: v[1], ..p };
        }
        assert!((1..=REACH).contains(&p.window), "the window is 1 to {REACH} shots");
        p
    }
}

/// Colour distance of two shots, 0..1: that of their closest pair of frames,
/// so that a shot which moves is still like the shot it ends on.
fn distance(a: &Shot, b: &Shot, sig: &[FrameSignals]) -> f32 {
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
fn shared_cast(a: &[usize], b: &[usize]) -> Option<f32> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let both = a.iter().filter(|c| b.contains(c)).count();
    Some(both as f32 / (a.len() + b.len() - both) as f32)
}

/// How alike the pictures of two shots are: the cosine of their closest
/// pair of frames. `None` when a shot has no vector.
fn closest(a: &[Vec<f32>], b: &[Vec<f32>]) -> Option<f32> {
    a.iter()
        .flat_map(|x| b.iter().map(move |y| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>()))
        .max_by(f32::total_cmp)
}

/// A picture embedding's say on every pair of shots within [`REACH`].
struct Cue {
    name: String,
    weight: f32,
    /// The cosine two shots far apart have: nothing in common.
    floor: f32,
    /// By [`Pairs::at`]; NaN where a shot has no vector.
    cos: Vec<f32>,
}

/// What is known of every pair of shots within [`REACH`] of each other.
struct Pairs {
    /// Colour distance.
    colour: Vec<f32>,
    /// Shared cast; NaN where a shot shows nobody the cast knows.
    cast: Vec<f32>,
    cues: Vec<Cue>,
}

impl Pairs {
    fn at(i: usize, j: usize) -> usize {
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

    fn new(shots: &[Shot], sig: &[FrameSignals]) -> Self {
        let n = shots.len();
        Self {
            colour: Self::fill(n, |i, j| Some(distance(&shots[i], &shots[j], sig))),
            cast: Self::fill(n, |i, j| shared_cast(&shots[i].cast, &shots[j].cast)),
            cues: Vec::new(),
        }
    }

    /// Adds a picture embedding: per shot, the unit vectors of its frames.
    fn add(&mut self, name: &str, weight: f32, vectors: &[Vec<Vec<f32>>]) {
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
    fn says(&self, i: usize, j: usize, p: &Params) -> impl Iterator<Item = (f32, f32)> {
        let k = Self::at(i, j);
        let colour = (1.0 - self.colour[k] / p.far).clamp(0.0, 1.0);
        let cues = self.cues.iter().map(move |c| (((c.cos[k] - c.floor) / (1.0 - c.floor)).clamp(0.0, 1.0), c.weight));
        [(colour, p.colour), (self.cast[k], p.cast)].into_iter().chain(cues).filter(|(v, _)| !v.is_nan())
    }

    /// How alike shots `i < j` are, 0..1: the weighted mean of the cues.
    fn alike(&self, i: usize, j: usize, p: &Params) -> f32 {
        let (sum, weight) = self.says(i, j, p).fold((0.0, 0.0), |(s, w), (v, x)| (s + v * x, w + x));
        if weight > 0.0 { sum / weight } else { 0.0 }
    }
}

/// One score per gap between two shots, 0..1: how much speaks for a new scene
/// there. Two alike shots hold everything between them together (a shot
/// between two alike shots belongs with them); the link weakens with each
/// shot it reaches over and with a fade or a dissolve on its way. Scenes come
/// from a threshold, so a higher one only joins scenes.
fn boundary_scores(pairs: &Pairs, between: &[Between], p: &Params) -> Vec<f32> {
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
fn settle(base: &[f32], secs: &[f32], p: &Params) -> Vec<f32> {
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

/// Reads a file of `shot_embed.py`: per shot, the unit vectors of its frames.
fn embedding(path: &Path, shots: &[Shot]) -> Vec<Vec<Vec<f32>>> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(b.len() >= 12 && &b[..4] == b"ASEM", "{}: not an embedding file", path.display());
    let u32_at = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let (rows, dim) = (u32_at(4) as usize, u32_at(8) as usize);
    assert_eq!(b.len(), 12 + rows * (4 + dim * 4), "{}: cut short", path.display());
    let mut out = vec![Vec::new(); shots.len()];
    for r in 0..rows {
        let at = 12 + r * (4 + dim * 4);
        if let Some(s) = shot_at(shots, u32_at(at)) {
            let v = b[at + 4..at + 4 + dim * 4].as_chunks::<4>().0.iter().map(|x| f32::from_le_bytes(*x)).collect();
            out[s].push(v);
        }
    }
    assert!(out.iter().all(|v| !v.is_empty()), "{}: a shot has no vector: not this media's file", path.display());
    out
}

/// The scene of each shot, for a cut at every gap marked.
fn scene_ids(cuts: &[bool]) -> Vec<usize> {
    let mut ids = vec![0; cuts.len() + 1];
    for (g, &cut) in cuts.iter().enumerate() {
        ids[g + 1] = ids[g] + usize::from(cut);
    }
    ids
}

// -- labels ------------------------------------------------------------------

/// The first shot of each labelled scene (`bench/scenes/*.scenes.tsv`), as a
/// cut per gap. The first column is the shot's first frame, or the shot's
/// number when the header calls it `shot`.
fn scene_labels(path: &Path, shots: &[Shot]) -> Vec<bool> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let by_number = text.lines().next().is_some_and(|h| h.split('\t').next() == Some("shot"));
    let mut cuts = vec![false; shots.len() - 1];
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let v: u32 = line.split('\t').next().unwrap().trim().parse().expect("scene labels: a number first");
        let s = if by_number {
            assert!((v as usize) < shots.len(), "scene at shot {v}: the project has {} shots", shots.len());
            v as usize
        } else {
            let s = shot_starting_near(shots, v);
            assert!(shots[s].start.abs_diff(v) <= 12, "scene at frame {v}: no shot starts there");
            s
        };
        if s > 0 {
            cuts[s - 1] = true;
        }
    }
    cuts
}

struct Label {
    start: u32,
    end: u32,
    name: String,
    boxes: Vec<(u32, [f32; 4])>,
}

/// The identity bench's labels (`bench/identity`): `start end name boxes`.
fn face_labels(path: &Path) -> Vec<Label> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let boxes = f[3]
                .split(' ')
                .filter(|b| !b.is_empty())
                .map(|b| {
                    let (frame, at) = b.split_once(':').expect("frame:x0,y0,x1,y1");
                    let v: Vec<f32> = at.split(',').map(|x| x.parse().unwrap()).collect();
                    (frame.parse().unwrap(), [v[0], v[1], v[2], v[3]])
                })
                .collect();
            Label { start: f[0].parse().unwrap(), end: f[1].parse().unwrap(), name: f[2].to_owned(), boxes }
        })
        .collect()
}

fn named(name: &str) -> bool {
    !matches!(name, "?" | "?e" | "?u" | "x" | "n")
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let area = |r: &[f32; 4]| (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0);
    let both = w * h;
    if both > 0.0 { both / (area(a) + area(b) - both) } else { 0.0 }
}

/// The label a stored track is: the one most of its detections lie on. A
/// detection lies on the label whose box (the nearest in time, at most 4
/// frames away) overlaps it most, by 0.3 at least.
fn label_of<'a>(track: &Track, labels: &'a [Label]) -> Option<(&'a str, u32)> {
    let mut votes: HashMap<&str, u32> = HashMap::new();
    for d in &track.dets {
        let hit = labels
            .iter()
            .filter(|l| l.start <= d.frame + 4 && d.frame <= l.end + 4)
            .filter_map(|l| {
                let (f, b) = l.boxes.iter().min_by_key(|(f, _)| f.abs_diff(d.frame))?;
                (f.abs_diff(d.frame) <= 4).then(|| (iou(b, &d.bbox), l))
            })
            .filter(|(overlap, _)| *overlap >= 0.3)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, l)) = hit {
            *votes.entry(&l.name).or_default() += 1;
        }
    }
    votes.into_iter().max_by_key(|&(name, n)| (n, std::cmp::Reverse(name)))
}

// -- scores ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
struct Prf {
    p: f64,
    r: f64,
    f: f64,
}

fn prf(hit: f64, given: f64, wanted: f64) -> Prf {
    let p = if given > 0.0 { hit / given } else { 0.0 };
    let r = if wanted > 0.0 { hit / wanted } else { 0.0 };
    Prf { p, r, f: if p + r > 0.0 { 2.0 * p * r / (p + r) } else { 0.0 } }
}

/// Boundaries found against the labelled ones; `slack` gaps to either side
/// still count. A found boundary answers for one labelled boundary at most,
/// and a labelled one for one found.
fn boundary_prf(found: &[bool], truth: &[bool], slack: usize) -> Prf {
    let at = |c: &[bool]| (0..c.len()).filter(|&g| c[g]).collect::<Vec<usize>>();
    let (given, wanted) = (at(found), at(truth));
    // Both lists rise: a labelled boundary takes the first found one within
    // reach that no labelled boundary before it took.
    let (mut matched, mut next) = (0, 0);
    for &t in &wanted {
        while next < given.len() && given[next] + slack < t {
            next += 1;
        }
        if next < given.len() && given[next] <= t + slack {
            matched += 1;
            next += 1;
        }
    }
    prf(matched as f64, given.len() as f64, wanted.len() as f64)
}

/// Average precision of the gaps' scores against the labelled boundaries.
/// Gaps with one and the same score count as if the labelled ones among them
/// were spread evenly: where they lie in the episode says nothing.
fn average_precision(score: &[f32], truth: &[bool]) -> f64 {
    let mut order: Vec<usize> = (0..score.len()).collect();
    order.sort_by(|&a, &b| score[b].total_cmp(&score[a]));
    let (mut hits, mut rank, mut sum) = (0f64, 0f64, 0f64);
    for tied in order.chunk_by(|&a, &b| score[a] == score[b]) {
        let (all, labelled) = (tied.len() as f64, tied.iter().filter(|&&g| truth[g]).count());
        for i in 1..=labelled {
            sum += (hits + i as f64) / (rank + i as f64 * all / labelled as f64);
        }
        hits += labelled as f64;
        rank += all;
    }
    if hits > 0.0 { sum / hits } else { 0.0 }
}

/// How well two ways to cut the shots into scenes overlap, 0..1: each scene's
/// best intersection over union with a scene of the other way, weighted by
/// the scene's frames, the mean of both directions.
fn overlap(a: &[usize], b: &[usize], frames: &[u32]) -> f64 {
    let one_way = |x: &[usize], y: &[usize]| {
        let mut len_x: BTreeMap<usize, f64> = BTreeMap::new();
        let mut len_y: BTreeMap<usize, f64> = BTreeMap::new();
        let mut both: BTreeMap<(usize, usize), f64> = BTreeMap::new();
        for s in 0..x.len() {
            let f = frames[s] as f64;
            *len_x.entry(x[s]).or_default() += f;
            *len_y.entry(y[s]).or_default() += f;
            *both.entry((x[s], y[s])).or_default() += f;
        }
        let mut best: BTreeMap<usize, f64> = BTreeMap::new();
        for (&(i, j), &n) in &both {
            let v = n / (len_x[&i] + len_y[&j] - n);
            let b = best.entry(i).or_default();
            *b = b.max(v);
        }
        let total: f64 = len_x.values().sum();
        best.iter().map(|(i, v)| v * len_x[i]).sum::<f64>() / total
    };
    (one_way(a, b) + one_way(b, a)) / 2.0
}

/// A character's scenes, as the editor asks for them: every shot of each
/// scene with at least one shot the character is seen in. `truth` holds the
/// labelled scenes and where the character truly is, `given` the scenes under
/// test and where the cast under test shows the character. `None` when the
/// character is not in these shots at all.
fn character_scenes(
    truth: (&[usize], &[bool]),
    given: (&[usize], &[bool]),
    range: std::ops::Range<usize>,
) -> Option<Prf> {
    let in_scenes = |(scene, seen): (&[usize], &[bool])| {
        let with: HashSet<usize> = range.clone().filter(|&s| seen[s]).map(|s| scene[s]).collect();
        range.clone().map(|s| with.contains(&scene[s])).collect::<Vec<bool>>()
    };
    let (wanted, got) = (in_scenes(truth), in_scenes(given));
    let count = |f: &dyn Fn(usize) -> bool| (0..wanted.len()).filter(|&k| f(k)).count() as f64;
    let all = count(&|k| wanted[k]);
    (all > 0.0).then(|| prf(count(&|k| wanted[k] && got[k]), count(&|k| got[k]), all))
}

fn mean(v: impl Iterator<Item = f64>) -> f64 {
    let v: Vec<f64> = v.collect();
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}

// -- score -------------------------------------------------------------------

struct Bench {
    shots: Vec<Shot>,
    between: Vec<Between>,
    pairs: Pairs,
    /// Each shot's length in seconds.
    secs: Vec<f32>,
    truth_cuts: Vec<bool>,
    /// Per main character: its name, the shots it is truly seen in, the
    /// shots the cast shows it in.
    cast: Vec<(String, Vec<bool>, Vec<bool>)>,
}

impl Bench {
    /// One score per gap: what speaks for a new scene there.
    fn scores(&self, p: &Params) -> Vec<f32> {
        settle(&boundary_scores(&self.pairs, &self.between, p), &self.secs, p)
    }

    /// Mean over the main characters of the F of their scenes' shots, for
    /// scenes cut at `cuts`: with the true cast, and with the stored one.
    fn characters(&self, cuts: &[bool], range: std::ops::Range<usize>) -> (f64, f64) {
        let (truth, given) = (scene_ids(&self.truth_cuts), scene_ids(cuts));
        let each = |stored: bool| {
            mean(self.cast.iter().filter_map(|(_, seen, shown)| {
                let at = if stored { shown } else { seen };
                character_scenes((&truth, seen), (&given, at), range.clone()).map(|x| x.f)
            }))
        };
        (each(false), each(true))
    }

    fn cuts_at(&self, score: &[f32], threshold: f32) -> Vec<bool> {
        score.iter().map(|&s| s > threshold).collect()
    }

    /// The threshold with the best mean F (true cast) over `range`; of
    /// several as good, the lowest.
    fn best_threshold(&self, score: &[f32], range: std::ops::Range<usize>) -> f32 {
        let mut best = (0.0, f64::MIN);
        for t in (2..=38).map(|k| k as f32 * 0.025) {
            let f = self.characters(&self.cuts_at(score, t), range.clone()).0;
            if f > best.1 {
                best = (t, f);
            }
        }
        best.0
    }
}

/// Per main character of the identity labels: its name, the shots it is
/// truly seen in, and the shots the stored character most of its faces are
/// on is shown in.
fn labelled_cast(ep: &Episode, shots: &[Shot], labels: &[Label]) -> Vec<(String, Vec<bool>, Vec<bool>)> {
    let n = shots.len();
    let mut screen: BTreeMap<&str, u32> = BTreeMap::new();
    for l in labels.iter().filter(|l| named(&l.name)) {
        *screen.entry(&l.name).or_default() += l.end - l.start;
    }
    let mut main: Vec<(&str, u32)> = screen.into_iter().filter(|&(_, f)| f as f64 / ep.fps >= MAIN_SEC).collect();
    main.sort_by_key(|&(name, f)| (std::cmp::Reverse(f), name));
    let mut held: HashMap<(&str, usize), u32> = HashMap::new();
    for t in &ep.tracks {
        if let (Some(c), Some((name, dets))) = (t.character, label_of(t, labels)) {
            *held.entry((name, c)).or_default() += dets;
        }
    }
    println!("\nmain cast (at least {MAIN_SEC} s on screen) and the stored character most of their faces are on:");
    let mut cast = Vec::new();
    for &(name, frames) in &main {
        let mut seen = vec![false; n];
        for l in labels.iter().filter(|l| l.name == name) {
            for s in 0..n {
                if shots[s].start < l.end.max(l.start + 1) && l.start < shots[s].end {
                    seen[s] = true;
                }
            }
        }
        let theirs =
            held.iter().filter(|((who, _), _)| *who == name).max_by_key(|&((_, c), &d)| (d, std::cmp::Reverse(*c)));
        let mut shown = vec![false; n];
        let mut what = "nobody".to_owned();
        if let Some((&(_, c), &dets)) = theirs {
            for s in 0..n {
                shown[s] = shots[s].cast.contains(&c);
            }
            let of_name: u32 = held.iter().filter(|((who, _), _)| *who == name).map(|(_, d)| d).sum();
            let of_char: u32 = held.iter().filter(|((_, x), _)| *x == c).map(|(_, d)| d).sum();
            what = format!(
                "{} (holds {:.0} % of their faces, {:.0} % of its faces are theirs)",
                ep.characters[c],
                100.0 * dets as f64 / of_name as f64,
                100.0 * dets as f64 / of_char as f64
            );
        }
        println!(
            "  {name:10} {:6.1} s, seen in {:3} shots, shown in {:3}: {what}",
            frames as f64 / ep.fps,
            seen.iter().filter(|&&x| x).count(),
            shown.iter().filter(|&&x| x).count()
        );
        cast.push((name.to_owned(), seen, shown));
    }
    cast
}

/// The same for a title without identity labels: the stored characters with
/// at least [`MAIN_SEC`] on screen, taken as who is where.
fn stored_cast(ep: &Episode, shots: &[Shot]) -> Vec<(String, Vec<bool>, Vec<bool>)> {
    let mut frames = vec![0u32; ep.characters.len()];
    for t in &ep.tracks {
        if let Some(c) = t.character {
            frames[c] += t.end - t.start;
        }
    }
    let mut main: Vec<usize> = (0..frames.len()).filter(|&c| frames[c] as f64 / ep.fps >= MAIN_SEC).collect();
    main.sort_by_key(|&c| (std::cmp::Reverse(frames[c]), c));
    println!("\nmain cast (the stored characters with at least {MAIN_SEC} s on screen; no identity labels):");
    main.iter()
        .map(|&c| {
            let shown: Vec<bool> = shots.iter().map(|s| s.cast.contains(&c)).collect();
            println!(
                "  {:16} {:6.1} s, shown in {:3} shots",
                ep.characters[c],
                frames[c] as f64 / ep.fps,
                shown.iter().filter(|&&x| x).count()
            );
            (ep.characters[c].clone(), shown.clone(), shown)
        })
        .collect()
}

/// How well two sets of scene labels of one episode agree.
fn agree(args: &[String]) {
    let ep = load(Path::new(&args[0]));
    let (shots, _) = shots_of(&ep);
    let (one, other) = (scene_labels(Path::new(&args[1]), &shots), scene_labels(Path::new(&args[2]), &shots));
    let frames: Vec<u32> = shots.iter().map(|s| s.end - s.start).collect();
    let count = |c: &[bool]| c.iter().filter(|&&x| x).count() + 1;
    let (exact, slack) = (boundary_prf(&one, &other, 0), boundary_prf(&one, &other, 1));
    println!(
        "{} scenes against {}: boundaries P {:.3} R {:.3} F {:.3}; one shot off still counted: F {:.3}; overlap {:.3}",
        count(&one),
        count(&other),
        exact.p,
        exact.r,
        exact.f,
        slack.f,
        overlap(&scene_ids(&one), &scene_ids(&other), &frames)
    );
    for g in (0..one.len()).filter(|&g| one[g] != other[g]) {
        let at = shots[g + 1].start as f64 / ep.fps;
        println!(
            "  shot {:4} ({}:{:02}): only in the {}",
            g + 1,
            at as u32 / 60,
            at as u32 % 60,
            if one[g] { "first" } else { "second" }
        );
    }
}

fn score(args: &[String]) {
    let ep = load(Path::new(&args[0]));
    let (shots, between) = shots_of(&ep);
    let truth_cuts = scene_labels(Path::new(&args[1]), &shots);
    let labels = (args[2] != "-").then(|| face_labels(Path::new(&args[2])));
    let n = shots.len();
    println!(
        "{} shots ({} with someone the cast knows), {} labelled scenes; between shots: {} dissolves, {} black or fades",
        n,
        shots.iter().filter(|s| !s.cast.is_empty()).count(),
        truth_cuts.iter().filter(|&&c| c).count() + 1,
        between.iter().filter(|&&b| b == Between::Blend).count(),
        between.iter().filter(|&&b| b == Between::Dark).count(),
    );

    let cast = match labels.as_deref() {
        Some(labels) => labelled_cast(&ep, &shots, labels),
        None => stored_cast(&ep, &shots),
    };

    assert!(!cast.is_empty(), "nobody is known in this episode: its scores need identity labels");

    let mut pairs = Pairs::new(&shots, &ep.signals);
    for cue in std::env::var("EMB").unwrap_or_default().split(',').filter(|c| !c.is_empty()) {
        let (weight, file) = cue.split_once(':').expect("EMB=<weight>:<file>[,<weight>:<file>…]");
        let path = Path::new(file);
        let name = path.file_name().unwrap().to_string_lossy();
        pairs.add(&name, weight.parse().expect("EMB: a weight"), &embedding(path, &shots));
    }
    let secs = shots.iter().map(|s| ((s.end - s.start) as f64 / ep.fps) as f32).collect();
    let bench = Bench { shots, between, pairs, secs, truth_cuts, cast };
    let frames: Vec<u32> = bench.shots.iter().map(|s| s.end - s.start).collect();
    let truth = scene_ids(&bench.truth_cuts);

    let p = Params::from_env();
    println!("\n{p:?}");
    for c in &bench.pairs.cues {
        println!("{}: weight {}, shots far apart are {:.3} alike", c.name, c.weight, c.floor);
    }
    if std::env::var("STATS").is_ok() {
        stats(&bench, &p);
    }
    if std::env::var("SWEEP").is_ok() {
        sweep(&bench, &p);
        return;
    }
    let scores = bench.scores(&p);
    let at = std::env::var("AT")
        .ok()
        .map_or_else(|| bench.best_threshold(&scores, 0..n), |v| v.parse().expect("AT=<threshold>"));
    println!("boundary scores: average precision {:.3}", average_precision(&scores, &bench.truth_cuts));
    if std::env::var("CURVE").is_ok() {
        println!("curve	threshold	true cast	stored	boundary F	scenes");
        for k in 2..=38 {
            let t = k as f32 * 0.025;
            let cuts = bench.cuts_at(&scores, t);
            let (seen, shown) = bench.characters(&cuts, 0..n);
            let b = boundary_prf(&cuts, &bench.truth_cuts, 0);
            println!("curve	{t:.3}	{seen:.4}	{shown:.4}	{:.4}	{}", b.f, cuts.iter().filter(|&&c| c).count() + 1);
        }
    }

    println!(
        "\n{:34} {:>6} {:>22} {:>8} {:>10} {:>10}",
        "scenes", "scenes", "boundary P / R / F", "overlap", "true cast", "stored"
    );
    let row = |what: &str, cuts: &[bool]| {
        let b = boundary_prf(cuts, &bench.truth_cuts, 0);
        let (seen, shown) = bench.characters(cuts, 0..n);
        println!(
            "{what:34} {:>6} {:>8.3} {:>6.3} {:>6.3} {:>8.3} {:>10.3} {:>10.3}",
            cuts.iter().filter(|&&c| c).count() + 1,
            b.p,
            b.r,
            b.f,
            overlap(&scene_ids(cuts), &truth, &frames),
            seen,
            shown
        );
    };
    row("every shot its own scene", &vec![true; n - 1]);
    row("one scene", &vec![false; n - 1]);
    row("a new scene at each fade to black", &bench.between.iter().map(|&b| b == Between::Dark).collect::<Vec<_>>());
    row("the labelled scenes", &bench.truth_cuts);
    for t in [at - 0.1, at - 0.05, at, at + 0.05, at + 0.1] {
        row(&format!("threshold {t:.3}"), &bench.cuts_at(&scores, t));
    }
    let slack = boundary_prf(&bench.cuts_at(&scores, at), &bench.truth_cuts, 1);
    println!("at {at:.3}, a boundary one shot off still counted: P {:.3} R {:.3} F {:.3}", slack.p, slack.r, slack.f);

    // One episode only: a threshold chosen on one half, read on the other.
    let half = (0..n).find(|&s| truth[s] >= truth[n - 1].div_ceil(2)).unwrap();
    let (first, second) = (bench.best_threshold(&scores, 0..half), bench.best_threshold(&scores, half..n));
    println!(
        "\nthreshold from the other half: shots 0..{half} at {second:.3}: true cast {:.3}, stored {:.3}; shots {half}..{n} at {first:.3}: true cast {:.3}, stored {:.3}",
        bench.characters(&bench.cuts_at(&scores, second), 0..half).0,
        bench.characters(&bench.cuts_at(&scores, second), 0..half).1,
        bench.characters(&bench.cuts_at(&scores, first), half..n).0,
        bench.characters(&bench.cuts_at(&scores, first), half..n).1,
    );

    println!("\nper character at {at:.3} (P / R / F of the shots of their scenes):");
    let given = scene_ids(&bench.cuts_at(&scores, at));
    // A main character of a project's labels may not be in this episode at all.
    for (name, seen, shown) in bench.cast.iter().filter(|(_, seen, _)| seen.contains(&true)) {
        let alone = character_scenes((&truth, seen), (&(0..n).collect::<Vec<_>>(), seen), 0..n).unwrap();
        let a = character_scenes((&truth, seen), (&given, seen), 0..n).unwrap();
        let b = character_scenes((&truth, seen), (&given, shown), 0..n).unwrap();
        let c = character_scenes((&truth, seen), (&truth, shown), 0..n).unwrap();
        println!(
            "  {name:10} only the shots they are seen in: R {:.3} | true cast {:.3} / {:.3} / {:.3} | stored {:.3} / {:.3} / {:.3} | stored cast, labelled scenes F {:.3}",
            alone.r, a.p, a.r, a.f, b.p, b.r, b.f, c.f
        );
    }

    if std::env::var("DETAIL").is_ok() {
        println!("\ngap\tframe\tsec\tbetween\tscore\tlabel\tcast before | after");
        for g in 0..n - 1 {
            let names = |s: &Shot| s.cast.iter().map(|&c| ep.characters[c].as_str()).collect::<Vec<_>>().join(",");
            println!(
                "{g}\t{}\t{:.1}\t{:?}\t{:.3}\t{}\t{} | {}",
                bench.shots[g + 1].start,
                bench.shots[g + 1].start as f64 / ep.fps,
                bench.between[g],
                scores[g],
                if bench.truth_cuts[g] { "scene" } else { "" },
                names(&bench.shots[g]),
                names(&bench.shots[g + 1])
            );
        }
    }
}

/// How alike two shots that follow each other are by each cue: inside a
/// labelled scene, and across a labelled boundary (10 %, median, 90 %).
fn stats(bench: &Bench, p: &Params) {
    let names: Vec<&str> =
        ["colours", "cast"].into_iter().chain(bench.pairs.cues.iter().map(|c| c.name.as_str())).collect();
    println!("\nhow alike two shots in a row are: inside a scene | across a boundary (10 %, median, 90 %; pairs)");
    for (k, name) in names.iter().enumerate() {
        let of = |boundary: bool| {
            let mut v: Vec<f32> = (0..bench.between.len())
                .filter(|&g| bench.truth_cuts[g] == boundary)
                .map(|g| {
                    let at = Pairs::at(g, g + 1);
                    match k {
                        0 => (1.0 - bench.pairs.colour[at] / p.far).clamp(0.0, 1.0),
                        1 => bench.pairs.cast[at],
                        _ => {
                            let c = &bench.pairs.cues[k - 2];
                            ((c.cos[at] - c.floor) / (1.0 - c.floor)).clamp(0.0, 1.0)
                        }
                    }
                })
                .filter(|x| !x.is_nan())
                .collect();
            v.sort_by(f32::total_cmp);
            let q = |s: f32| v.get(((v.len() as f32 - 1.0) * s) as usize).copied().unwrap_or(f32::NAN);
            format!("{:.2} {:.2} {:.2} ({})", q(0.1), q(0.5), q(0.9), v.len())
        };
        println!("  {name:24} {} | {}", of(false), of(true));
    }
}

/// A grid of settings around the given ones, each at its best threshold: is
/// the result a plateau?
fn sweep(bench: &Bench, given: &Params) {
    let n = bench.shots.len();
    println!("\nwindow decay dark blend colour cast | AP | threshold | true cast | stored");
    for window in [4, 8, 12, 16, 24] {
        for decay in [0.8, 0.9, 1.0] {
            for (dark, blend) in [(1.0, 1.0), (0.5, 0.8), (0.0, 0.5)] {
                for colour in [0.0, given.colour] {
                    for cast in [0.0, given.cast, 2.0 * given.cast] {
                        let p = Params { window, decay, dark, blend, colour, cast, ..*given };
                        let scores = bench.scores(&p);
                        let at = bench.best_threshold(&scores, 0..n);
                        let (seen, shown) = bench.characters(&bench.cuts_at(&scores, at), 0..n);
                        println!(
                            "{window:2} {decay:.1} {dark:.1} {blend:.1} {colour:.2} {cast:.2} | {:.3} | {at:.3} | {seen:.3} | {shown:.3}",
                            average_precision(&scores, &bench.truth_cuts)
                        );
                    }
                }
            }
        }
    }
}

// -- labels from Serial Speakers ---------------------------------------------

/// Fits the label file's times to the project's frames and prints the scenes.
/// The file's episodes come from a 25 fps disc: either the same pictures
/// played faster (a frame there is a frame here) or the same seconds.
fn labels(args: &[String]) {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args[0]).unwrap_or_else(|e| panic!("{}: {e}", args[0])))
            .expect("the label file");
    let (season, episode): (u64, u64) = (args[1].parse().unwrap(), args[2].parse().unwrap());
    let find = |list: &serde_json::Value, id: u64| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"].as_u64() == Some(id))
            .cloned()
            .expect("no such season or episode")
    };
    let ep_json = find(&find(&json["seasons"], season)["episodes"], episode);
    let ep = load(Path::new(&args[3]));
    let (shots, _) = shots_of(&ep);
    let ours: Vec<u32> = ep.segments.iter().map(|s| s.0).collect();

    let scenes = ep_json["data"]["scenes"].as_array().unwrap();
    let theirs: Vec<f64> = scenes
        .iter()
        .flat_map(|s| s["shots"].as_array().unwrap().iter().map(|x| x["start"].as_f64().unwrap()))
        .collect();
    // The shift most of their cuts agree on, for both ways to read their clock.
    let fit = |rate: f64| {
        let mut votes: HashMap<i64, u32> = HashMap::new();
        for &t in &theirs {
            let at = (t * rate).round() as i64;
            for &o in &ours {
                let d = o as i64 - at;
                if d.abs() <= 3000 {
                    *votes.entry(d).or_default() += 1;
                }
            }
        }
        votes.into_iter().max_by_key(|&(d, n)| (n, std::cmp::Reverse(d.abs()), d)).unwrap()
    };
    let source_fps = ep_json["fps"].as_f64().unwrap();
    let (rate, (shift, _)) =
        [source_fps, ep.fps].into_iter().map(|r| (r, fit(r))).max_by_key(|&(_, (_, n))| n).unwrap();
    let to_frame = |t: f64| ((t * rate).round() as i64 + shift).max(0) as u32;
    let near = |f: u32, slack: u32| ours.iter().any(|&o| o.abs_diff(f) <= slack);
    let exact = theirs.iter().filter(|&&t| near(to_frame(t), 0)).count();
    let close = theirs.iter().filter(|&&t| near(to_frame(t), 2)).count();
    eprintln!(
        "{} cuts in the label file, {} boundaries here; a frame here = their second x {rate:.3} {shift:+}: {exact} of their cuts on a boundary here, {close} within 2 frames",
        theirs.len(),
        ours.len()
    );

    let speech = ep_json["data"]["speech_segments"].as_array().unwrap();
    println!("frame\tsource_s\tspeakers");
    for (i, scene) in scenes.iter().enumerate() {
        let start = scene["start"].as_f64().unwrap();
        let end = scenes.get(i + 1).map_or(f64::MAX, |s| s["start"].as_f64().unwrap());
        let at = shot_starting_near(&shots, to_frame(start));
        let off = shots[at].start.abs_diff(to_frame(start));
        if off > 2 {
            eprintln!(
                "scene {i} at {start:.1} s: frame {} lies {off} frames from the shot at {}",
                to_frame(start),
                shots[at].start
            );
        }
        let mut said: BTreeMap<&str, f64> = BTreeMap::new();
        for s in speech {
            let (a, b) = (s["start"].as_f64().unwrap(), s["end"].as_f64().unwrap());
            if a >= start && a < end {
                *said.entry(s["speaker"].as_str().unwrap()).or_default() += b - a;
            }
        }
        let mut said: Vec<(&str, f64)> = said.into_iter().filter(|&(_, s)| s >= 1.0).collect();
        said.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
        let said: Vec<String> = said.iter().map(|(who, s)| format!("{who}:{s:.0}")).collect();
        println!("{}\t{start:.2}\t{}", shots[at].start, said.join(";"));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.len()) {
        (Some("labels"), 5) => labels(&args[1..]),
        (Some("score"), 4) => score(&args[1..]),
        (Some("agree"), 4) => agree(&args[1..]),
        _ => {
            eprintln!(
                "usage: scenes_bench labels <serial_speakers.json> <season> <episode> <project_copy_dir>\n       scenes_bench score <project_copy_dir> <scenes.tsv> <identity labels.tsv | ->\n       scenes_bench agree <project_copy_dir> <one.scenes.tsv> <other.scenes.tsv>"
            );
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let s = boundary_scores(&Pairs::new(&shots, &sig), &between, &Params::DEFAULT);
        assert!(s[3] > 0.8, "the two scenes share nothing: {s:?}");
        for g in [0, 1, 2, 4, 5] {
            assert!(s[g] < 0.2, "gap {g} lies between two alike shots: {s:?}");
        }
        // No reach: every change of picture and people is a boundary.
        let alone = boundary_scores(&Pairs::new(&shots, &sig), &between, &Params { window: 1, ..Params::DEFAULT });
        assert!(alone.iter().all(|&x| x > 0.8), "{alone:?}");
    }

    #[test]
    fn a_fade_weakens_the_link_across_it() {
        let (shots, sig) = film(&[0, 0, 0, 0], &[&[], &[], &[], &[]]);
        let plain = boundary_scores(&Pairs::new(&shots, &sig), &[Between::Cut; 3], &Params::DEFAULT);
        let faded =
            boundary_scores(&Pairs::new(&shots, &sig), &[Between::Cut, Between::Dark, Between::Cut], &Params::DEFAULT);
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
    fn scores_of_known_cases() {
        // Scenes 0 0 0 | 1 1 against 0 0 | 1 1 1.
        let (truth, given) = (scene_ids(&[false, false, true, false]), scene_ids(&[false, true, false, false]));
        assert_eq!(boundary_prf(&[false, true, false, false], &[false, false, true, false], 0).f, 0.0);
        assert_eq!(boundary_prf(&[false, true, false, false], &[false, false, true, false], 1).f, 1.0);
        // One shot off: a boundary answers for one boundary only.
        let two = boundary_prf(&[false, true, false, true, false], &[false, false, true, false, false], 1);
        assert_eq!((two.p, two.r), (0.5, 1.0));
        let one = boundary_prf(&[false, true, false, false], &[false, true, true, false], 1);
        assert_eq!((one.p, one.r), (1.0, 0.5));
        // Equal scores: the labelled gap's place among them does not count.
        assert_eq!(average_precision(&[0.5, 0.5], &[true, false]), average_precision(&[0.5, 0.5], &[false, true]));
        let o = overlap(&truth, &given, &[1, 1, 1, 1, 1]);
        assert!((o - 2.0 / 3.0).abs() < 1e-9, "{o}");
        assert_eq!(average_precision(&[0.1, 0.9, 0.5], &[false, true, false]), 1.0);
        assert_eq!(average_precision(&[0.9, 0.1, 0.5], &[false, true, false]), 1.0 / 3.0);
        // Seen in shot 0 only: their scene is shots 0..3; the scenes under
        // test give shots 0..2, so 2 of 3 wanted, both given ones right.
        let seen = [true, false, false, false, false];
        let x = character_scenes((&truth, &seen), (&given, &seen), 0..5).unwrap();
        assert_eq!((x.p, x.r), (1.0, 2.0 / 3.0));
        let nowhere = [false; 5];
        assert!(character_scenes((&truth, &nowhere), (&given, &seen), 0..5).is_none());
    }

    #[test]
    fn labels_find_their_track() {
        let labels = vec![
            Label {
                start: 10,
                end: 20,
                name: "a".into(),
                boxes: vec![(10, [0.1, 0.1, 0.3, 0.5]), (16, [0.1, 0.1, 0.3, 0.5])],
            },
            Label { start: 10, end: 20, name: "b".into(), boxes: vec![(10, [0.6, 0.1, 0.8, 0.5])] },
        ];
        let det = |frame, x: f32| FaceDet { frame, bbox: [x, 0.1, x + 0.2, 0.5], score: 1.0, norm: 20.0, quality: 1.0 };
        let track = |x| Track { start: 11, end: 15, character: None, dets: vec![det(11, x), det(14, x)] };
        assert_eq!(label_of(&track(0.11), &labels), Some(("a", 2)));
        assert_eq!(label_of(&track(0.61), &labels), Some(("b", 2)));
        assert_eq!(label_of(&track(0.35), &labels), None);
    }
}
