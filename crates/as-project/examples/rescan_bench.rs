//! Dev tool: how well Rescan's judge tells who a face is, on a COPY of a
//! project (`project.db` and its `-wal` in a folder of their own: opening it
//! may migrate it).
//!
//! `cargo run --release -p as-project --example rescan_bench -- <project_copy_dir> [labels.tsv]`
//!
//! Pins: every face the owner confirmed, asked about blind (no pin, on no one,
//! its shot held out). Machine pins are mostly easy faces, so hard ones
//! (below `core_quality`) are counted apart.
//! Labels (`bench/identity`): every face has a truth, so the suggestions for
//! Everyone else and the doubts about assigned faces can be scored, strangers
//! included.

use std::collections::HashMap;
use std::path::PathBuf;

use as_analysis::tracks::dets_from_bytes;
use as_api::TrackAssign;
use as_identity::{CharKey, Verdict};
use as_project::{Project, RescanCast};

#[derive(Default)]
struct Tally {
    right: u32,
    wrong: u32,
    maybe_right: u32,
    maybe_wrong: u32,
    nobody: u32,
    other: u32,
}

impl Tally {
    fn add(&mut self, v: Verdict, is: impl Fn(CharKey) -> bool) {
        match v {
            Verdict::Newly(c) | Verdict::Elsewhere(c) if is(c) => self.right += 1,
            Verdict::Newly(_) | Verdict::Elsewhere(_) => self.wrong += 1,
            Verdict::Maybe(c) if is(c) => self.maybe_right += 1,
            Verdict::Maybe(_) => self.maybe_wrong += 1,
            Verdict::Between(a, b) if is(a) || is(b) => self.maybe_right += 1,
            Verdict::Between(..) => self.maybe_wrong += 1,
            Verdict::Nobody => self.nobody += 1,
            _ => self.other += 1,
        }
    }

    fn print(&self, what: &str) {
        let n = self.right + self.wrong + self.maybe_right + self.maybe_wrong + self.nobody + self.other;
        let share = |a: u32, b: u32| if a + b == 0 { 0.0 } else { a as f64 / (a + b) as f64 };
        println!(
            "{what:36} n {n:5} | clear right {:4} wrong {:4} ({:.3}) | maybe right {:4} wrong {:4} ({:.3}) | nobody {:4} other {:3}",
            self.right,
            self.wrong,
            share(self.right, self.wrong),
            self.maybe_right,
            self.maybe_wrong,
            share(self.maybe_right, self.maybe_wrong),
            self.nobody,
            self.other,
        );
    }
}

/// The owner's pins, asked about blind.
fn pins(cast: &RescanCast) {
    let core = cast.params.core_quality;
    let pinned: Vec<usize> = (0..cast.tracks.len()).filter(|&i| cast.tracks[i].pin.is_some()).collect();
    let truth: HashMap<usize, CharKey> = pinned.iter().map(|&i| (i, cast.tracks[i].pin.unwrap())).collect();
    let mut blind = cast.clone();
    for &i in &pinned {
        blind.tracks[i].forbid.clear();
        blind.now[i] = None;
    }
    // A pin stays a member; only the subject's own answer is hidden, one fold
    // of shots at a time, by the judge itself.
    let subjects: Vec<usize> = pinned.clone();
    for &i in &subjects {
        blind.tracks[i].pin = None;
    }
    let (mut hard, mut easy) = (Tally::default(), Tally::default());
    for j in blind.judge(&subjects, &[]) {
        let c = truth[&j.track];
        let tally = if cast.tracks[j.track].quality < core { &mut hard } else { &mut easy };
        tally.add(j.verdict, |k| k == c);
    }
    hard.print("pins, hard (quality < core)");
    easy.print("pins, clear");
}

/// One line of a labels file.
struct Label {
    name: String,
    boxes: HashMap<u32, [f32; 4]>,
}

fn read_labels(path: &str) -> Vec<Label> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let boxes = f[3]
                .split_whitespace()
                .map(|b| {
                    let (frame, xy) = b.split_once(':').unwrap();
                    let v: Vec<f32> = xy.split(',').map(|x| x.parse().unwrap()).collect();
                    (frame.parse().unwrap(), [v[0], v[1], v[2], v[3]])
                })
                .collect();
            Label { name: f[2].to_string(), boxes }
        })
        .collect()
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let iw = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let ih = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let u = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - iw * ih;
    if u <= 0.0 { 0.0 } else { iw * ih / u }
}

/// The name of each track: the label most of its detections fall on.
fn names(cast: &RescanCast, labels: &[Label]) -> Vec<Option<String>> {
    let mut by_frame: HashMap<u32, Vec<usize>> = HashMap::new();
    for (n, l) in labels.iter().enumerate() {
        for &f in l.boxes.keys() {
            by_frame.entry(f).or_default().push(n);
        }
    }
    cast.row_of
        .iter()
        .map(|&r| {
            let dets = dets_from_bytes(&cast.rows[r].det_bytes).unwrap_or_default();
            let mut hits: HashMap<usize, usize> = HashMap::new();
            for d in &dets {
                for &n in by_frame.get(&d.frame).into_iter().flatten() {
                    if iou(&d.bbox, &labels[n].boxes[&d.frame]) > 0.3 {
                        *hits.entry(n).or_default() += 1;
                    }
                }
            }
            let (&best, &count) = hits.iter().max_by_key(|&(n, c)| (*c, usize::MAX - *n))?;
            (count * 2 >= dets.len()).then(|| labels[best].name.clone())
        })
        .collect()
}

fn known(name: &str) -> bool {
    !name.starts_with('?') && name != "x"
}

/// Faces with a truth: what a rescan of Everyone else and of the whole cast
/// would suggest.
fn labelled(cast: &RescanCast, labels: &[Label]) {
    let names = names(cast, labels);
    println!("labelled tracks {} of {}", names.iter().flatten().count(), names.len());
    // Who each character is: the name most of its members have.
    let who: HashMap<CharKey, String> = cast
        .members
        .iter()
        .filter_map(|(k, g)| {
            let mut count: HashMap<&str, usize> = HashMap::new();
            for &i in g {
                if let Some(n) = names[i].as_deref().filter(|n| known(n)) {
                    *count.entry(n).or_default() += 1;
                }
            }
            count.into_iter().max_by_key(|(n, c)| (*c, *n)).map(|(n, _)| (*k, n.to_string()))
        })
        .collect();
    let assign = |i: usize| cast.rows[cast.row_of[i]].assign;
    let kind = |v: Verdict| match v {
        Verdict::Right => "right",
        Verdict::Doubt => "doubt",
        Verdict::Elsewhere(_) => "elsewhere",
        Verdict::Newly(_) => "newly",
        Verdict::Maybe(_) => "maybe",
        Verdict::Between(..) => "between",
        Verdict::Nobody => "nobody",
        Verdict::Thin => "thin",
    };
    let pass = |subjects: Vec<usize>, what: &str| {
        let (mut named, mut strangers) = (Tally::default(), Tally::default());
        let mut seen: HashMap<&'static str, u32> = HashMap::new();
        for j in cast.judge(&subjects, &[]) {
            let Some(name) = names[j.track].as_deref().filter(|n| *n != "x") else { continue };
            if known(name) && who.values().any(|w| w == name) {
                named.add(j.verdict, |k| who.get(&k).is_some_and(|w| w == name));
            } else {
                strangers.add(j.verdict, |_| false);
            }
            *seen.entry(kind(j.verdict)).or_default() += 1;
        }
        named.print(&format!("{what}: people of the cast"));
        strangers.print(&format!("{what}: strangers"));
        let mut seen: Vec<_> = seen.into_iter().collect();
        seen.sort();
        println!("{what:36} verdicts {seen:?}");
    };
    let with = |a: TrackAssign| (0..cast.tracks.len()).filter(|&i| assign(i) == a).collect::<Vec<_>>();
    pass(with(TrackAssign::None), "everyone else");
    pass(with(TrackAssign::Unsure), "unsure");

    // Wrongly placed faces: does the judge see them?
    let (mut caught, mut missed, mut alarm, mut quiet) = (0, 0, 0, 0);
    for j in cast.judge(&with(TrackAssign::Auto), &[]) {
        let Some(name) = names[j.track].as_deref().filter(|n| *n != "x") else { continue };
        let wrong = cast.now[j.track].and_then(|k| who.get(&k)).is_some_and(|w| w != name);
        let raised = matches!(j.verdict, Verdict::Elsewhere(_) | Verdict::Doubt | Verdict::Between(..));
        match (wrong, raised) {
            (true, true) => caught += 1,
            (true, false) => missed += 1,
            (false, true) => alarm += 1,
            (false, false) => quiet += 1,
        }
    }
    println!("assigned: wrong+raised {caught}, wrong+missed {missed}, right+raised {alarm}, right+quiet {quiet}");
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    assert!(!a.is_empty(), "usage: rescan_bench <project_copy_dir> [labels.tsv]");
    let project = Project::open_copy(PathBuf::from(&a[0])).unwrap();
    let started = std::time::Instant::now();
    let cast = project.rescan_cast().unwrap();
    let on = |a: TrackAssign| cast.row_of.iter().filter(|&&r| cast.rows[r].assign == a).count();
    println!(
        "{} tracks, {} characters; none {}, unsure {}, auto {}, pinned {}; fitted {}",
        cast.tracks.len(),
        cast.characters.len(),
        on(TrackAssign::None),
        on(TrackAssign::Unsure),
        on(TrackAssign::Auto),
        on(TrackAssign::Pinned),
        matches!(cast.space, Some(as_identity::Space::Fitted(_))),
    );
    pins(&cast);
    if let Some(l) = a.get(1) {
        labelled(&cast, &read_labels(l));
    }
    println!("{:.1} s", started.elapsed().as_secs_f64());
}
