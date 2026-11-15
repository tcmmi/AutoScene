//! Dev tool: what bodies add to the face pass on a labelled episode
//! (`bench/bodies`) — how many labelled faces sit on a detected body, how
//! clean body tracks are, how many faces their bodies name, how well
//! clothes find a person again, and how much more of the cast is found.
//! `cargo run --release -p as-analysis --example bodies_bench -- <cmd> <faces.tracks> <run.bodies> <bb_dir>
//! <labels.tsv> [out_dir]` with `<cmd>`: `recall`, `tracks`, `score`, `reid`, `presence`, `sheets`.
//! `<run.bodies>` may be a `.found` file instead (`faces_run` with `PEOPLE=`): the people as the
//! app's face pass found them (every command but `recall`).
//! `bodies_bench audit <items.tsv> <verdicts.tsv>` scores the verdicts given on the sheets.
//!
//! The test set of bodies whose face does not show (`bench/bodies/README.md`, "The test set"):
//! - `pools … <out_prefix>` writes every query of an episode (`<out_prefix>.pools.tsv`) and
//!   what today's pass answers (`<out_prefix>.today.tsv`): E, a body with a face asked by its
//!   sightings without one; B, the body under a named `back` head track of the labels; N,
//!   bodies of extras; and, from `HARD=<title.hard.tsv>`, the bodies readers named: M (a
//!   face of that figure is labelled in the shot), H (none is), N (someone outside the cast).
//! - `back …` lists pool B alone.
//! - `draw … <out_dir>` draws body tracks without any face for the readers (`DRAW=<m>,<o>`:
//!   how many from shots that hold a labelled face of the main cast, the candidates for pool
//!   M, and from the other shots; 100,100 by default), and writes `draw.tsv`, `items.tsv`
//!   (one sheet a track, `GRID=3x2 bodies_render`), `gallery.items.tsv` and `gallery.txt`.
//! - `bodies_bench audit <draw.tsv> <reader_a.tsv> <reader_b.tsv> <third.tsv|-> <out.hard.tsv>`
//!   joins the readers' answers (`item answer [note]`) into the hard file.
//! - `bodies_bench table <title>=<today.tsv>[,<today.tsv>…] …` prints the pools per title
//!   and pooled, with intervals drawn over characters.
//!
//! Env for labels of the animation benches: `GALLERY=<gallery.tsv>` counts a figure's looks
//! as one name; `SPANS=<spans.tsv>` leaves opening, ending and the like out;
//! `EPISODE=<first>,<end>` takes one episode's frames out of a project's timeline;
//! `KEY=<episode>` names the episode in what is written; `ANIME=1` groups the tracks as the
//! anime scans do (with `GRID=6,10`), `ANIMATED=1` as 3D animation; `INFO=<bb.info.json>`
//! gives the frame rate.
//!
//! Env: `BODY_STRIDE=3|6|12` uses every 1st/2nd/4th body sample; `HALF=first|second` keeps
//! the queries of one half of the episode (`reid`); `WINDOW_S` (180), `REID_MIN` (0.65),
//! `REID_MARGIN` (0.15), `RESCUE_UNSURE` (1), `RESCUE_BELOW` (0.45) set the propagation;
//! `VETO`, `FAR_COS` the tracker; `TRACK_REID=0` tracks without appearance (the
//! embeddings join the tracks afterwards); `REID_TOP=n` keeps the appearance of a
//! track's n clearest sightings only; `DETAIL=1` lists cases.

#[path = "common/anime.rs"]
mod anime;
#[path = "common/bodyfile.rs"]
mod bodyfile;
#[path = "common/labels.rs"]
mod labels;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

use as_analysis::bodies::{BodyDet, BodyObs, BodyTrack};
use as_analysis::body_tracker::{BodyTrackerParams, track_bodies};
use as_analysis::link::{LinkParams, fit, link_shot};
use as_analysis::tracks::{self, FaceTrack};
use as_core::FrameRange;
use as_identity::bodies::{BodyAssign, BodyIn, BodyOutcome, BodyParams, FaceOn, propagate};
use as_identity::metrics::bcubed;
use as_identity::{Outcome, Params, TrackIn, identify};
use anime::{Gallery, Level, Spans, at_level};
use labels::{Kind, Label, STRIDE, box_iou, character_names, found_faces, fps, input, kind, members, named, names_of};

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().map_or(default, |v| v.parse().ok().unwrap_or_else(|| panic!("{name}")))
}

fn detail() -> bool {
    std::env::var_os("DETAIL").is_some()
}

/// Tiny deterministic random numbers (xorshift64*), for samples to look at.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn pick<T: Clone>(&mut self, v: &[T], n: usize) -> Vec<T> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        for i in (1..idx.len()).rev() {
            idx.swap(i, (self.next() % (i as u64 + 1)) as usize);
        }
        let mut idx: Vec<usize> = idx.into_iter().take(n).collect();
        idx.sort_unstable();
        idx.into_iter().map(|i| v[i].clone()).collect()
    }
}

/// Everything derived once per run.
struct World<'a> {
    tracks: Vec<FaceTrack>,
    input: Vec<TrackIn>,
    outcome: Outcome,
    got: Vec<Option<usize>>,
    truth: Vec<Option<&'a str>>,
    char_name: HashMap<usize, &'a str>,
    /// Main cast: named in the labels with 30 s or more.
    main: HashSet<&'a str>,
    /// Opening, ending and the like: nothing inside them is asked.
    spans: &'a Spans,
    shots: Vec<FrameRange>,
    samples: Vec<bodyfile::Sample>,
    body_stride: u32,
    /// The people are the app's own (a `.found` file): sightings carry no
    /// appearance of their own, only the track does.
    from_app: bool,
    bodies: Vec<BodyTrack>,
    /// Per body: (face track, samples tied).
    faces_on: Vec<Vec<(usize, u32)>>,
    body_in: Vec<BodyIn>,
    res: BodyOutcome,
    params: BodyParams,
}

fn read_shots(dir: &Path) -> Vec<FrameRange> {
    std::fs::read_to_string(dir.join("bb.segments.tsv"))
        .unwrap()
        .lines()
        .filter_map(|l| {
            let v: Vec<&str> = l.split('\t').collect();
            (v[2].trim() == "shot").then(|| FrameRange::new(v[0].parse().unwrap(), v[1].parse().unwrap()))
        })
        .collect()
}

/// The propagation as the study recommends it, each value overridable.
fn body_params() -> BodyParams {
    let d = BodyParams::default();
    BodyParams {
        rescue_unsure: env("RESCUE_UNSURE", 1) != 0,
        rescue_below: env("RESCUE_BELOW", Params::default().core_quality),
        window_s: env("WINDOW_S", d.window_s),
        reid_min: env("REID_MIN", d.reid_min),
        reid_margin: env("REID_MARGIN", d.reid_margin),
        min_score: env("MIN_DET", d.min_score),
    }
}

/// What the labels say: every label of the episode, the main cast of the
/// title, and the spans left out.
struct Truth {
    labels: Vec<Label>,
    main: HashSet<String>,
    spans: Spans,
}

/// The labels of `path` as the bench reads them. With `GALLERY` a figure's
/// looks are one name. The main cast is every named figure with 30 s or more
/// in the whole file (all its episodes; at 23.976 frames a second, so that
/// the episodes of one title agree on it), labels inside a span of `SPANS`
/// left out. With `EPISODE=<first>,<end>` only the labels that start in
/// those frames stay, counted from `first` (one episode of a project's
/// timeline, on its own frames).
fn read_truth(path: &str) -> Truth {
    let gallery = std::env::var("GALLERY").ok().map(|p| Gallery::read(&p));
    let mut labels = labels::read_labels(path);
    if let Some(g) = &gallery {
        for l in &mut labels {
            l.name = at_level(&l.name, Some(g), Level::Figure);
        }
    }
    let mut spans = std::env::var("SPANS").ok().map_or_else(Spans::default, |p| Spans::read(&p));
    let mut screen: HashMap<&str, u32> = HashMap::new();
    for l in labels.iter().filter(|l| spans.mostly_in(l.span.0, l.span.1).is_none()) {
        *screen.entry(l.name.as_str()).or_default() += l.span.1 - l.span.0;
    }
    let main: HashSet<String> = screen
        .iter()
        .filter(|(n, f)| named(n).is_some() && **f as f64 / labels::FPS >= 30.0)
        .map(|(n, _)| n.to_string())
        .collect();
    if let Ok(v) = std::env::var("EPISODE") {
        let (first, end) = v.split_once(',').expect("EPISODE=<first>,<end>");
        let (first, end): (u32, u32) = (first.trim().parse().unwrap(), end.trim().parse().unwrap());
        labels.retain(|l| (first..end).contains(&l.span.0));
        for l in &mut labels {
            l.span = (l.span.0 - first, l.span.1 - first);
            l.boxes.iter_mut().for_each(|b| b.0 -= first);
        }
        spans.0.retain(|s| s.0 < end && first < s.1);
        for s in &mut spans.0 {
            (s.0, s.1) = (s.0.saturating_sub(first), s.1.min(end) - first);
        }
    }
    Truth { labels, main, spans }
}

/// The grouping the app uses for the footage: the anime scans' with
/// `ANIME=1`, 3D animation's with `ANIMATED=1`, live action's otherwise.
fn identity_params() -> Params {
    let mut p = if std::env::var_os("ANIME").is_some() { Params::anime() } else { Params::default() };
    p.animated = std::env::var_os("ANIMATED").is_some_and(|v| v == "1");
    p
}

fn build<'a>(tracks_path: &str, bodies_path: &str, dir: &Path, said: &'a Truth) -> World<'a> {
    let labels = &said.labels[..];
    let tracks = labels::read_tracks(tracks_path);
    let truth = names_of(&tracks, labels);
    let input = input(&tracks);
    let outcome = identify(&input, &identity_params());
    let got = members(&outcome);
    let char_name = character_names(&tracks, &truth, &got);
    let main: HashSet<&str> = said.main.iter().map(String::as_str).collect();
    let shots = read_shots(dir);
    if bodies_path.ends_with(".found") {
        // The app's own: tracked, tied and embedded in the face pass.
        let found = as_analysis::bodies::decode(&std::fs::read(bodies_path).unwrap()).expect("readable people");
        let people = as_analysis::PeopleParams::default();
        let bodies: Vec<BodyTrack> = found
            .iter()
            .map(|b| BodyTrack {
                start: b.start,
                end: b.end,
                shot: b.shot,
                dets: b.dets.clone(),
                reids: vec![None; b.dets.len()],
            })
            .collect();
        let faces_on: Vec<Vec<(usize, u32)>> =
            found.iter().map(|b| b.faces.iter().map(|&(f, n)| (f as usize, n)).collect()).collect();
        let body_in: Vec<BodyIn> = found
            .iter()
            .zip(&bodies)
            .map(|(f, b)| BodyIn {
                media: 0,
                fps: fps() as f32,
                spans: b.spans(people.stride, people.span_gap()),
                faces: f.faces.iter().map(|&(track, samples)| FaceOn { track: track as usize, samples }).collect(),
                reid: f.reid.clone(),
                reid_away: f.reid_away.clone(),
                score: f.showing.score,
                pin: None,
                forbid: vec![],
            })
            .collect();
        let params = body_params();
        let res = propagate(&input, &outcome, &body_in, &params);
        return World {
            tracks,
            input,
            outcome,
            got,
            truth,
            char_name,
            main,
            spans: &said.spans,
            shots,
            samples: Vec::new(),
            body_stride: people.stride,
            from_app: true,
            bodies,
            faces_on,
            body_in,
            res,
            params,
        };
    }
    let body_stride: u32 = env("BODY_STRIDE", 3);
    assert!(body_stride.is_multiple_of(STRIDE));
    let file = bodyfile::read(Path::new(bodies_path));
    let samples: Vec<bodyfile::Sample> =
        file.samples.into_iter().filter(|s| s.frame.is_multiple_of(body_stride)).collect();

    let mut tp = BodyTrackerParams::new(body_stride);
    tp.veto_cos = env("VETO", tp.veto_cos);
    tp.far_cos = env("FAR_COS", tp.far_cos);
    tp.max_gap = tp.max_gap.max(2 * body_stride);
    let track_reid = env("TRACK_REID", 1) != 0;
    let reid_top: usize = env("REID_TOP", usize::MAX);
    // The faces' own sample grid says when two of them are on a body at once.
    let lp = LinkParams { max_interp: 24.max(2 * body_stride), stride: labels::grid().0, ..LinkParams::default() };
    let gap = (24 / body_stride).max(1);
    let (mut bodies, mut faces_on) = (Vec::new(), Vec::new());
    let mut s = 0;
    for shot in &shots {
        while s < samples.len() && samples[s].frame < shot.start {
            s += 1;
        }
        let mut here = Vec::new();
        while s < samples.len() && samples[s].frame < shot.end {
            let people = &samples[s].people;
            let obs = people
                .iter()
                .enumerate()
                .map(|(i, p)| BodyObs {
                    det: BodyDet {
                        frame: samples[s].frame,
                        bbox: p.bbox,
                        visible: p.visible,
                        score: p.score,
                        sharpness: p.sharpness,
                        luma: p.luma,
                        rival: people
                            .iter()
                            .enumerate()
                            .filter(|&(j, _)| j != i)
                            .map(|(_, o)| o.sharpness)
                            .fold(0.0, f32::max),
                    },
                    reid: p.reid.clone().filter(|_| track_reid),
                })
                .collect();
            here.push((samples[s].frame, obs));
            s += 1;
        }
        let mut found = track_bodies(*shot, here, &tp);
        for b in &mut found {
            if !track_reid {
                // The sightings' embeddings, on the tracks made without them.
                for (d, e) in b.dets.iter().zip(b.reids.iter_mut()) {
                    let at = samples.binary_search_by_key(&d.frame, |x| x.frame).expect("a sample");
                    *e = samples[at].people.iter().find(|p| p.visible == d.visible).and_then(|p| p.reid.clone());
                }
            }
            if reid_top < b.dets.len() {
                // Only the clearest sightings are embedded: the biggest, surest, sharpest.
                let weight = |d: &BodyDet| d.score * (d.height() / 0.3).min(1.0);
                let mut order: Vec<usize> = (0..b.dets.len())
                    .filter(|&i| b.reids[i].is_some() && b.dets[i].luma >= as_analysis::bodies::MIN_LUMA)
                    .collect();
                order.sort_by(|&i, &j| {
                    weight(&b.dets[j])
                        .total_cmp(&weight(&b.dets[i]))
                        .then(b.dets[j].sharpness.total_cmp(&b.dets[i].sharpness))
                });
                for &i in order.iter().skip(reid_top) {
                    b.reids[i] = None;
                }
            }
        }
        let face_ids: Vec<usize> = (0..tracks.len()).filter(|&i| tracks[i].shot == shot.start).collect();
        let fs: Vec<&FaceTrack> = face_ids.iter().map(|&i| &tracks[i]).collect();
        let bs: Vec<&BodyTrack> = found.iter().collect();
        let links = link_shot(&fs, &bs, &lp);
        let base = bodies.len();
        faces_on.extend((0..found.len()).map(|_| Vec::new()));
        for l in links {
            faces_on[base + l.body].push((face_ids[l.face], l.samples));
        }
        bodies.extend(found);
    }
    let body_in: Vec<BodyIn> = bodies
        .iter()
        .zip(&faces_on)
        .map(|(b, fo)| {
            let face_spans: Vec<(u32, u32)> = fo.iter().flat_map(|&(f, _)| input[f].spans.clone()).collect();
            let has_face = |d: &BodyDet| face_spans.iter().any(|&(a, e)| a <= d.frame && d.frame < e);
            BodyIn {
                media: 0,
                fps: fps() as f32,
                spans: b.spans(body_stride, gap),
                faces: fo.iter().map(|&(track, samples)| FaceOn { track, samples }).collect(),
                reid: b.appearance(|_| true),
                reid_away: b.appearance(|d| !has_face(d)),
                score: b.dets.iter().map(|d| d.score).fold(0.0, f32::max),
                pin: None,
                forbid: vec![],
            }
        })
        .collect();
    let params = body_params();
    let res = propagate(&input, &outcome, &body_in, &params);
    World {
        tracks,
        input,
        outcome,
        got,
        truth,
        char_name,
        main,
        spans: &said.spans,
        shots,
        samples,
        body_stride,
        from_app: false,
        bodies,
        faces_on,
        body_in,
        res,
        params,
    }
}

impl World<'_> {
    /// The label name of a body: the one most of its tied faces' samples have.
    fn body_truth(&self, b: usize) -> Option<&str> {
        let mut by: HashMap<&str, u32> = HashMap::new();
        for &(f, n) in &self.faces_on[b] {
            if let Some(name) = self.truth[f] {
                *by.entry(name).or_default() += n;
            }
        }
        by.into_iter().max_by_key(|&(n, v)| (v, std::cmp::Reverse(n))).map(|(n, _)| n)
    }

    fn character_of(&self, b: usize) -> Option<usize> {
        self.res.bodies[b].character()
    }

    fn name(&self, c: usize) -> &str {
        self.char_name.get(&c).copied().unwrap_or("?")
    }

    fn shot_of(&self, frame: u32) -> Option<FrameRange> {
        let i = self.shots.partition_point(|s| s.end <= frame);
        self.shots.get(i).filter(|s| s.contains(frame)).copied()
    }

    /// The sightings of body `b` while none of its faces shows: where the
    /// run kept an appearance per sighting, only those that have one.
    fn away(&self, b: usize) -> Vec<&BodyDet> {
        let face_at = |f: u32| {
            self.faces_on[b].iter().any(|&(t, _)| self.input[t].spans.iter().any(|&(a, z)| a <= f && f < z))
        };
        let body = &self.bodies[b];
        body.dets
            .iter()
            .zip(&body.reids)
            .filter(|(d, e)| (self.from_app || e.is_some()) && !face_at(d.frame))
            .map(|(d, _)| d)
            .collect()
    }

    /// Whether body `b` lies mostly inside an opening, ending or the like.
    fn in_span(&self, b: usize) -> bool {
        self.spans.mostly_in(self.bodies[b].start, self.bodies[b].end).is_some()
    }

    /// The body track of shot `shot` whose box at `frame` is `bbox` (IoU of
    /// 0.5 or more; the most alike of several).
    fn body_at(&self, shot: u32, frame: u32, bbox: &[f32; 4]) -> Option<usize> {
        let gap = 24.max(2 * self.body_stride);
        (0..self.bodies.len())
            .filter(|&b| self.bodies[b].shot == shot)
            .filter_map(|b| self.bodies[b].box_at(frame, gap).map(|x| (box_iou(&x, bbox), b)))
            .filter(|&(iou, _)| iou >= 0.5)
            .max_by(|x, y| x.0.total_cmp(&y.0).then(y.1.cmp(&x.1)))
            .map(|(_, b)| b)
    }
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 { "   -  ".into() } else { format!("{:5.1}%", 100.0 * a as f64 / b as f64) }
}

/// Labelled faces sitting on a detected body.
fn recall(w: &World, labels: &[Label]) {
    let lp = LinkParams::default();
    let at: HashMap<u32, &bodyfile::Sample> = w.samples.iter().map(|s| (s.frame, s)).collect();
    let buckets = [(0.0, 0.08, "< 8 %"), (0.08, 0.15, "8-15 %"), (0.15, 0.3, "15-30 %"), (0.3, 2.0, ">= 30 %")];
    println!("labelled main-cast faces on a detected body (sample frames of the body run only):");
    for th in [0.3f32, 0.4, 0.5] {
        let (mut all, mut on, mut unique) = (0usize, 0usize, 0usize);
        let mut by_bucket = [(0usize, 0usize); 4];
        let mut by_name: HashMap<&str, (usize, usize)> = HashMap::new();
        for l in labels.iter().filter(|l| w.main.contains(l.name.as_str())) {
            for (frame, fb) in &l.boxes {
                let Some(s) = at.get(frame) else { continue };
                let fits: Vec<f32> =
                    s.people.iter().filter(|p| p.score >= th).filter_map(|p| fit(fb, &p.visible, &lp)).collect();
                let h = fb[3] - fb[1];
                let k = buckets.iter().position(|b| (b.0..b.1).contains(&h)).unwrap();
                all += 1;
                by_bucket[k].0 += 1;
                let e = by_name.entry(l.name.as_str()).or_default();
                e.0 += 1;
                if !fits.is_empty() {
                    on += 1;
                    by_bucket[k].1 += 1;
                    e.1 += 1;
                    let mut f = fits.clone();
                    f.sort_by(|a, b| b.total_cmp(a));
                    unique += usize::from(f.len() == 1 || f[0] - f[1] >= lp.rival_margin);
                }
            }
        }
        let buck: Vec<String> =
            buckets.iter().zip(&by_bucket).map(|(b, (n, k))| format!("{} {} of {n}", b.2, pct(*k, *n))).collect();
        println!(
            "  score >= {th}: {} of {all} ({} told apart) | by face height: {}",
            pct(on, all),
            pct(unique, all),
            buck.join(", ")
        );
        if th == 0.5 {
            let mut names: Vec<(&&str, &(usize, usize))> = by_name.iter().collect();
            names.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
            let v: Vec<String> = names.iter().map(|(n, (a, k))| format!("{n} {}", pct(*k, *a))).collect();
            println!("    by character: {}", v.join(", "));
        }
    }
    // Backs of heads and gas masks the face detector found: on a body?
    let (mut xs, mut xon) = (0, 0);
    for l in labels.iter().filter(|l| matches!(kind(&l.name), Kind::Excluded)) {
        for (frame, fb) in &l.boxes {
            if let Some(s) = at.get(frame) {
                xs += 1;
                xon += usize::from(s.people.iter().any(|p| p.score >= 0.5 && fit(fb, &p.visible, &lp).is_some()));
            }
        }
    }
    println!("  'x' faces (masks, backs of heads) on a body: {} of {xs}", pct(xon, xs));
    // People without any face.
    let mut faces_at: HashMap<u32, Vec<[f32; 4]>> = HashMap::new();
    for t in &w.tracks {
        for d in &t.dets {
            faces_at.entry(d.frame).or_default().push(d.bbox);
        }
    }
    let (mut people, mut faceless) = (0usize, 0usize);
    for s in &w.samples {
        for p in s.people.iter().filter(|p| p.score >= 0.5) {
            people += 1;
            let f = faces_at.get(&s.frame).map_or(&[][..], Vec::as_slice);
            faceless += usize::from(!f.iter().any(|fb| fit(fb, &p.visible, &lp).is_some()));
        }
    }
    println!(
        "people (score >= 0.5): {:.2} per sample, {} of them without a face of the face pass",
        people as f64 / w.samples.len() as f64,
        pct(faceless, people)
    );
    let with_face: HashSet<u32> = w.tracks.iter().map(|t| t.shot).collect();
    let bare: Vec<&FrameRange> = w.shots.iter().filter(|s| !with_face.contains(&s.start)).collect();
    let bodied = bare.iter().filter(|s| w.bodies.iter().any(|b| b.shot == s.start)).count();
    let secs: u32 = w.bodies.iter().filter(|b| !with_face.contains(&b.shot)).map(|b| b.end - b.start).sum();
    println!(
        "shots without any face track: {}; with a body track: {bodied} ({:.0} s of body tracks there)",
        bare.len(),
        secs as f64 / fps()
    );
}

/// How clean body tracks are, judged by the labels of the faces on them.
fn tracks_cmd(w: &World) {
    let n = w.bodies.len();
    let linked = w.faces_on.iter().filter(|f| !f.is_empty()).count();
    let named = |f: usize| w.truth[f].and_then(named);
    let (mut one, mut two, mut impure, mut links, mut wrong) = (0, 0, 0, 0, 0);
    let mut list = Vec::new();
    for (b, fo) in w.faces_on.iter().enumerate() {
        let names: Vec<&str> = fo.iter().filter_map(|&(f, _)| named(f)).collect();
        if names.is_empty() {
            continue;
        }
        one += 1;
        if names.len() < 2 {
            continue;
        }
        two += 1;
        let major = w.body_truth(b).unwrap_or("?");
        links += names.len();
        let off = names.iter().filter(|&&x| x != major).count();
        wrong += off;
        if off > 0 {
            impure += 1;
            list.push(format!("{}-{} {:?}", w.bodies[b].start, w.bodies[b].end, names));
        }
    }
    let conflicts = w.res.bodies.iter().filter(|a| **a == BodyAssign::Conflict).count();
    let mut frag: HashMap<(u32, &str), HashSet<usize>> = HashMap::new();
    for (b, fo) in w.faces_on.iter().enumerate() {
        for &(f, _) in fo {
            if let Some(name) = named(f) {
                frag.entry((w.bodies[b].shot, name)).or_default().insert(b);
            }
        }
    }
    let split = frag.values().filter(|s| s.len() > 1).count();
    let dets: usize = w.bodies.iter().map(|b| b.dets.len()).sum();
    println!(
        "{n} body tracks (body stride {}; {:.1} sightings each); {linked} carry a face track ({})",
        w.body_stride,
        dets as f64 / n as f64,
        pct(linked, n)
    );
    // What the ReID costs: the sightings big enough to embed, all of them or
    // a track's clearest few (embedded as they come, a clearer one replacing
    // the weakest).
    let embeddable: Vec<Vec<f32>> = w
        .bodies
        .iter()
        .map(|b| {
            b.dets
                .iter()
                .zip(&b.reids)
                .filter(|(d, e)| e.is_some() && d.luma >= as_analysis::bodies::MIN_LUMA)
                .map(|(d, _)| d.score * (d.height() / 0.3).min(1.0))
                .collect()
        })
        .collect();
    let all: usize = embeddable.iter().map(Vec::len).sum();
    let online = |cap: usize| -> usize {
        embeddable
            .iter()
            .map(|ws| {
                let mut kept: Vec<f32> = Vec::new();
                let mut made = 0;
                for &x in ws {
                    if kept.len() < cap {
                        kept.push(x);
                        made += 1;
                    } else if let Some((i, _)) =
                        kept.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).filter(|(_, m)| x > **m)
                    {
                        kept[i] = x;
                        made += 1;
                    }
                }
                made
            })
            .sum()
    };
    println!(
        "  embeddable sightings: {all}; embedded with a cap of 8 / 16 / 32 per track: {} / {} / {}",
        online(8),
        online(16),
        online(32)
    );
    println!(
        "  with a named face: {one}; with 2+ named faces: {two}, of them mixed: {impure} ({}); faces on the wrong body: {wrong} of {links} ({})",
        pct(impure, two),
        pct(wrong, links)
    );
    println!(
        "  conflicts (faces of different characters): {conflicts}; (shot, person) split over several bodies: {split}"
    );
    if detail() {
        for l in list {
            println!("    mixed: {l}");
        }
    }
}

/// BCubed of the main cast per track and per screen time.
fn bcubed_main(w: &World, got: &[Option<usize>]) -> (f32, f32, f32, f32) {
    let keep: Vec<usize> =
        (0..w.tracks.len()).filter(|&i| w.truth[i].is_some_and(|n| !matches!(kind(n), Kind::Excluded))).collect();
    let mut ids: HashMap<&str, usize> = HashMap::new();
    let truth: Vec<usize> = keep
        .iter()
        .map(|&i| match w.truth[i] {
            Some(n) if named(n).is_some() => {
                let k = ids.len();
                *ids.entry(n).or_insert(k)
            }
            _ => 1_000_000 + i,
        })
        .collect();
    let main_ids: HashSet<usize> = ids.iter().filter(|(n, _)| w.main.contains(*n)).map(|(_, &k)| k).collect();
    let sub: Vec<Option<usize>> = keep.iter().map(|&i| got[i]).collect();
    let frames: Vec<f32> = keep.iter().map(|&i| (w.tracks[i].end - w.tracks[i].start) as f32).collect();
    let ones = vec![1f32; keep.len()];
    let counted = |k: usize| main_ids.contains(&truth[k]);
    let a = bcubed(&sub, &truth, &ones, counted);
    let b = bcubed(&sub, &truth, &frames, counted);
    (a.precision, a.recall, a.f, b.f)
}

fn score(w: &World, labels: &[Label]) {
    let show = |what: &str, got: &[Option<usize>]| {
        let (p, r, f, tf) = bcubed_main(w, got);
        println!("{what:34} main cast tracks P {p:.3} R {r:.3} F {f:.3} | screen time F {tf:.3}");
        found_faces(&w.tracks, labels, &w.truth, got);
    };
    show("faces only", &w.got);
    for rescue_unsure in [false, true] {
        let p = BodyParams { rescue_unsure, ..w.params };
        let res = propagate(&w.input, &w.outcome, &w.body_in, &p);
        let mut got = w.got.clone();
        for &(t, c) in &res.promoted {
            got[t] = Some(c);
        }
        let wrong: Vec<String> = res
            .promoted
            .iter()
            .filter(|&&(t, c)| w.truth[t].is_some_and(|n| named(n).is_some() && n != w.name(c)))
            .map(|&(t, c)| {
                format!("{}@{}-{} -> {}", w.truth[t].unwrap(), w.tracks[t].start, w.tracks[t].end, w.name(c))
            })
            .collect();
        let what = if rescue_unsure { "+ bodies name unassigned + unsure" } else { "+ bodies name unassigned faces" };
        println!("{} faces named by their body, {} of them wrong:", res.promoted.len(), wrong.len());
        show(what, &got);
        if detail() {
            for x in wrong {
                println!("    wrong: {x}");
            }
        }
    }
}

/// A query's answer (character, score, runner-up's score) and its true name.
type Ranked = (Option<(usize, f32, f32)>, String);

/// A body a query compares against: character, appearance, when.
struct Anchor<'v> {
    body: usize,
    character: usize,
    shot: u32,
    first: u32,
    last: u32,
    reid: [Option<&'v Vec<f32>>; 2],
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Best character for a query and its score, and the runner-up's score.
#[allow(clippy::too_many_arguments)]
fn rank(
    q: &[f32],
    first: u32,
    last: u32,
    shot: u32,
    own: usize,
    anchors: &[Anchor],
    window: u32,
    skip_shot: bool,
) -> Option<(usize, f32, f32)> {
    let mut by: Vec<(usize, f32)> = Vec::new();
    for a in anchors {
        if a.body == own || (skip_shot && a.shot == shot) {
            continue;
        }
        let gap = a.first.saturating_sub(last).max(first.saturating_sub(a.last));
        if gap > window {
            continue;
        }
        let s = a.reid.iter().flatten().map(|e| dot(q, e)).fold(f32::NEG_INFINITY, f32::max);
        match by.iter_mut().find(|v| v.0 == a.character) {
            Some(v) => v.1 = v.1.max(s),
            None => by.push((a.character, s)),
        }
    }
    by.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let &(c, s) = by.first()?;
    Some((c, s, by.get(1).map_or(-1.0, |v| v.1)))
}

/// Calibrate finding people again by their clothes, on bodies whose
/// person the labels know.
fn reid(w: &World) {
    let half = std::env::var("HALF").unwrap_or_default();
    let mid = w.shots.last().map_or(0, |s| s.end) / 2;
    let in_half = |b: &BodyTrack| match half.as_str() {
        "first" => b.start < mid,
        "second" => b.start >= mid,
        _ => true,
    };
    let anchors: Vec<Anchor> = (0..w.bodies.len())
        .filter_map(|b| match w.res.bodies[b] {
            BodyAssign::Face(c) => Some(Anchor {
                body: b,
                character: c,
                shot: w.bodies[b].shot,
                first: w.bodies[b].start,
                last: w.bodies[b].end,
                reid: [w.body_in[b].reid.as_ref(), w.body_in[b].reid_away.as_ref()],
            }),
            _ => None,
        })
        .collect();
    // Q1: bodies with a face, asked by their sightings without one.
    // Q2: bodies whose faces the face pass couldn't name, the labels can.
    // N: bodies of extras (only faces labelled `?`): any answer is wrong.
    let mut q1 = Vec::new();
    let mut q2 = Vec::new();
    let mut neg = Vec::new();
    for (b, body) in w.bodies.iter().enumerate() {
        if !in_half(body) {
            continue;
        }
        let truth = w.body_truth(b);
        match (w.res.bodies[b], truth) {
            (BodyAssign::Face(_), Some(n)) if w.main.contains(n) => {
                let away = w.away(b).len();
                if let Some(v) = w.body_in[b].reid_away.as_ref().filter(|_| away >= 3) {
                    q1.push((b, n, v));
                }
            }
            (BodyAssign::None, Some(n)) if w.main.contains(n) => {
                if let Some(v) = w.body_in[b].reid.as_ref() {
                    q2.push((b, n, v));
                }
            }
            (BodyAssign::None, Some(n)) if matches!(kind(n), Kind::OwnPerson) => {
                if let Some(v) = w.body_in[b].reid.as_ref() {
                    neg.push((b, n, v));
                }
            }
            _ => {}
        }
    }
    println!(
        "queries ({}): Q1 hidden face {}, Q2 face unnamed {}, N extras {}; anchors {}",
        if half.is_empty() { "all" } else { half.as_str() },
        q1.len(),
        q2.len(),
        neg.len(),
        anchors.len()
    );
    let skip_shot = std::env::var_os("SKIP_SHOT").is_some();
    for window_s in [60.0, 180.0, 600.0, 1e6] {
        let window = (window_s * fps()) as u32;
        let ranked = |qs: &[(usize, &str, &Vec<f32>)]| -> Vec<Ranked> {
            qs.iter()
                .map(|&(b, n, v)| {
                    let body = &w.bodies[b];
                    (rank(v, body.start, body.end, body.shot, b, &anchors, window, skip_shot), n.to_string())
                })
                .collect()
        };
        let (r1, r2, rn) = (ranked(&q1), ranked(&q2), ranked(&neg));
        println!("window {}:", if window_s > 1e5 { "episode".to_string() } else { format!("{window_s} s") });
        for margin in [0.05f32, 0.1, 0.15] {
            let mut line = format!("  margin {margin:.2}:");
            for th in [0.6f32, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9] {
                let count = |r: &[Ranked]| {
                    let (mut right, mut wrong) = (0, 0);
                    for (x, n) in r {
                        if let Some((c, s, s2)) = x
                            && *s >= th
                            && s - s2 >= margin
                        {
                            if w.name(*c) == n.as_str() { right += 1 } else { wrong += 1 }
                        }
                    }
                    (right, wrong)
                };
                let ((a1, b1), (a2, b2), (_, fp)) = (count(&r1), count(&r2), count(&rn));
                let p = |a: usize, b: usize| if a + b == 0 { 1.0 } else { a as f64 / (a + b) as f64 };
                let _ = write!(
                    line,
                    " | {th:.2} Q1 {:.2}/{:.2} Q2 {:.2}/{:.2} N {fp}",
                    p(a1, b1),
                    a1 as f64 / r1.len().max(1) as f64,
                    p(a2, b2),
                    a2 as f64 / r2.len().max(1) as f64
                );
            }
            println!("{line}");
        }
    }
}

/// One query of the test set: a body, the name the labels or the readers
/// give it, and what it is asked by.
struct Query {
    /// `E`, `B`, `M`, `H` or `N`.
    pool: &'static str,
    /// The run's body track (a reader's item may sit on none in another run).
    body: Option<usize>,
    shot: u32,
    start: u32,
    end: u32,
    /// A sighting to find the body by in another run.
    frame: u32,
    bbox: [f32; 4],
    /// The figure; `?` for an extra.
    name: String,
    flags: Vec<&'static str>,
    /// E and B: the appearance the query is asked by (`None`: nothing to embed).
    asked_by: Option<Vec<f32>>,
}

/// An answer: the name of the pass's character, its likeness, the runner-up's.
type Answer = Option<(String, f32, f32)>;

fn anchors_of<'v>(w: &'v World) -> Vec<Anchor<'v>> {
    (0..w.bodies.len())
        .filter_map(|b| match w.res.bodies[b] {
            BodyAssign::Face(c) => Some(Anchor {
                body: b,
                character: c,
                shot: w.bodies[b].shot,
                first: w.bodies[b].start,
                last: w.bodies[b].end,
                reid: [w.body_in[b].reid.as_ref(), w.body_in[b].reid_away.as_ref()],
            }),
            _ => None,
        })
        .collect()
}

/// Today's rule for clothes on an appearance `q` of body `own`: the most
/// alike character among the bodies with a face within the window, if alike
/// enough and clear of the next one. `own` itself is never asked, and with
/// `skip_shot` no body of its shot. The rule "not where the character already
/// is" is left out: the body asked is that character's.
fn ask(w: &World, anchors: &[Anchor], q: &[f32], own: usize, skip_shot: bool) -> Answer {
    let body = &w.bodies[own];
    let window = (f64::from(w.params.window_s) * fps()).round() as u32;
    let (c, s, s2) = rank(q, body.start, body.end, body.shot, own, anchors, window, skip_shot)?;
    (s >= w.params.reid_min && s - s2 >= w.params.reid_margin).then(|| (w.name(c).to_string(), s, s2))
}

/// What the pass itself made of a body by its clothes.
fn passed(w: &World, b: usize) -> Answer {
    match w.res.bodies[b] {
        BodyAssign::Reid { character, score, runner_up } => Some((w.name(character).to_string(), score, runner_up)),
        _ => None,
    }
}

/// Pool E: bodies with a face of the main cast, asked by their sightings
/// without one (at least 3).
fn easy(w: &World) -> Vec<Query> {
    let mut out = Vec::new();
    for (b, body) in w.bodies.iter().enumerate() {
        let (BodyAssign::Face(_), Some(n)) = (w.res.bodies[b], w.body_truth(b)) else { continue };
        if !w.main.contains(n) || w.in_span(b) {
            continue;
        }
        let away = w.away(b);
        let Some(v) = w.body_in[b].reid_away.as_ref().filter(|_| away.len() >= 3) else { continue };
        let d = away[away.len() / 2];
        // `mixed`: its faces are more than one named figure's, so the name
        // (the one most of their samples have) is less sure.
        let figures: HashSet<&str> = w.faces_on[b].iter().filter_map(|&(f, _)| w.truth[f].and_then(named)).collect();
        out.push(Query {
            pool: "E",
            body: Some(b),
            shot: body.shot,
            start: body.start,
            end: body.end,
            frame: d.frame,
            bbox: d.visible,
            name: n.to_string(),
            flags: if figures.len() > 1 { vec!["mixed"] } else { Vec::new() },
            asked_by: Some(v.clone()),
        });
    }
    out
}

/// Pool N from the labels: bodies whose faces are extras' (`?`, `?e`, `n`)
/// and that the face pass named no one. Any name is wrong.
fn extras(w: &World) -> Vec<Query> {
    let mut out = Vec::new();
    for (b, body) in w.bodies.iter().enumerate() {
        let unnamed = matches!(w.res.bodies[b], BodyAssign::None | BodyAssign::Reid { .. });
        if !unnamed || w.in_span(b) || !w.body_truth(b).is_some_and(|n| matches!(kind(n), Kind::OwnPerson)) {
            continue;
        }
        let d = body.dets[body.dets.len() / 2];
        out.push(Query {
            pool: "N",
            body: Some(b),
            shot: body.shot,
            start: body.start,
            end: body.end,
            frame: d.frame,
            bbox: d.visible,
            name: "?".into(),
            flags: vec!["face"],
            asked_by: None,
        });
    }
    out
}

fn is_back(l: &Label) -> bool {
    l.flags.iter().any(|f| f == "back")
}

/// Whether a face (not the back of the head) of `name` is labelled in `shot`.
fn face_in_shot(labels: &[Label], name: &str, shot: FrameRange) -> bool {
    labels.iter().any(|l| l.name == name && !is_back(l) && l.span.0 < shot.end && shot.start < l.span.1)
}

/// Pool B: the named `back` head tracks of the labels (main cast, outside
/// the spans), each on the body track most of its boxes sit on, asked by
/// that body's appearance while the back of the head shows. Flags: `far`,
/// no face of the figure is labelled within 48 frames of the track; `cut`,
/// none in its shot; `own`, the body carries a face of the figure at
/// another moment. Also: how many such labels there are, and how many sit
/// on no body.
fn back(w: &World, labels: &[Label]) -> (Vec<Query>, usize, usize) {
    const NEAR: u32 = 48;
    let lp = LinkParams::default();
    let gap = 24.max(2 * w.body_stride);
    let mut by_shot: HashMap<u32, Vec<usize>> = HashMap::new();
    for (b, body) in w.bodies.iter().enumerate() {
        by_shot.entry(body.shot).or_default().push(b);
    }
    let on_body = |hb: &[f32; 4], f: u32, b: usize| w.bodies[b].box_at(f, gap).and_then(|bb| fit(hb, &bb, &lp));
    let (mut out, mut all, mut bare) = (Vec::new(), 0, 0);
    for l in labels.iter().filter(|l| {
        is_back(l) && w.main.contains(l.name.as_str()) && w.spans.mostly_in(l.span.0, l.span.1).is_none()
    }) {
        all += 1;
        let mut votes: BTreeMap<usize, u32> = BTreeMap::new();
        for (f, hb) in &l.boxes {
            let Some(shot) = w.shot_of(*f) else { continue };
            let best = by_shot
                .get(&shot.start)
                .into_iter()
                .flatten()
                .filter_map(|&b| on_body(hb, *f, b).map(|s| (s, b)))
                .max_by(|x, y| x.0.total_cmp(&y.0).then(y.1.cmp(&x.1)));
            if let Some((_, b)) = best {
                *votes.entry(b).or_default() += 1;
            }
        }
        let total: u32 = votes.values().sum();
        let best = votes.iter().max_by_key(|&(&b, &n)| (n, std::cmp::Reverse(b))).map(|(&b, &n)| (b, n));
        let Some((b, _)) = best.filter(|&(_, n)| n >= 2.min(l.boxes.len() as u32) && n as f32 >= 0.6 * total as f32)
        else {
            bare += 1;
            continue;
        };
        let body = &w.bodies[b];
        let within = |d: &BodyDet| l.span.0 <= d.frame && d.frame < l.span.1;
        let mid = (l.span.0 + l.span.1) / 2;
        let d = body.dets.iter().min_by_key(|d| (!within(d), d.frame.abs_diff(mid))).expect("a sighting");
        let face_near = labels.iter().any(|o| {
            o.name == l.name && !is_back(o) && o.span.0 < l.span.1 + NEAR && l.span.0 < o.span.1 + NEAR
        });
        let shot = w.shot_of(mid).unwrap_or(FrameRange::new(body.shot, body.end));
        let own = labels.iter().any(|o| {
            o.name == l.name
                && !is_back(o)
                && o.boxes.iter().any(|(f, hb)| (body.start..body.end).contains(f) && on_body(hb, *f, b).is_some())
        });
        let mut flags = Vec::new();
        if !face_near {
            flags.push("far");
        }
        if !face_in_shot(labels, &l.name, shot) {
            flags.push("cut");
        }
        if own {
            flags.push("own");
        }
        out.push(Query {
            pool: "B",
            body: Some(b),
            shot: body.shot,
            start: l.span.0.max(body.start),
            end: l.span.1.min(body.end),
            frame: d.frame,
            bbox: d.visible,
            name: l.name.clone(),
            flags,
            // The app's people keep one appearance a track, not one a sighting.
            asked_by: if w.from_app { None } else { body.appearance(within) },
        });
    }
    (out, all, bare)
}

fn quad(s: &str) -> [f32; 4] {
    let v: Vec<f32> = s.split(',').map(|x| x.trim().parse().expect("a box")).collect();
    [v[0], v[1], v[2], v[3]]
}

fn quad_text(b: &[f32; 4]) -> String {
    format!("{:.4},{:.4},{:.4},{:.4}", b[0], b[1], b[2], b[3])
}

/// Pools M, H and N from the readers (`<title>.hard.tsv`, the rows of
/// episode `key`): a body they named is M when a face of that figure is
/// labelled in its shot and H when none is; one they call a person outside
/// the cast (`e`) or no person (`n`) is an extra. `x` (cannot tell) and
/// figures outside the main cast are in no pool.
fn from_readers(w: &World, labels: &[Label], path: &str, key: &str) -> Vec<Query> {
    let mut out = Vec::new();
    for v in labels::tsv_rows(path).into_iter().filter(|v| v[0] == key && v[2] != "e") {
        let (shot, start, end, frame): (u32, u32, u32, u32) =
            (v[3].parse().unwrap(), v[4].parse().unwrap(), v[5].parse().unwrap(), v[6].parse().unwrap());
        let (bbox, name) = (quad(&v[7]), v[8].as_str());
        let range = w.shot_of(frame).unwrap_or(FrameRange::new(shot, end));
        let (pool, name, mut flags) = match name {
            "x" => continue,
            "e" => ("N", "?", vec!["e"]),
            "n" => ("N", "?", vec!["n"]),
            n if !w.main.contains(n) => continue,
            n if face_in_shot(labels, n, range) => ("M", n, Vec::new()),
            n => ("H", n, Vec::new()),
        };
        let body = w.body_at(shot, frame, &bbox);
        // `clip`: the body shows enough to be a clip of its own in a pack.
        if body.is_some_and(|b| w.bodies[b].showing(16.0 / 9.0).prominent()) {
            flags.push("clip");
        }
        out.push(Query { pool, body, shot, start, end, frame, bbox, name: name.into(), flags, asked_by: None });
    }
    out
}

fn key() -> String {
    std::env::var("KEY").unwrap_or_else(|_| "-".into())
}

/// Every query of the episode and today's answer to each.
fn pools(w: &World, labels: &[Label], out: &Path) {
    let key = key();
    let anchors = anchors_of(w);
    let mut qs = easy(w);
    let (b, back_labels, bare) = back(w, labels);
    // The app's people keep no appearance per sighting: nothing to ask a
    // back of the head by.
    if !w.from_app {
        qs.extend(b);
    }
    qs.extend(extras(w));
    if let Ok(hard) = std::env::var("HARD") {
        qs.extend(from_readers(w, labels, &hard, &key));
    }
    let mut set = String::from("pool\tkey\tshot\tstart\tend\tframe\tbox\tname\tflags\n");
    let mut today =
        String::from("pool\tkey\tshot\tframe\tname\tflags\tgot\tscore\tnext\tgot_other_shots\tscore\tnext\n");
    let text = |a: &Answer| match a {
        Some((n, s, s2)) => format!("{n}\t{s:.3}\t{s2:.3}"),
        None => "-\t\t".into(),
    };
    // Per pool: queries, then answered and right, with the same shot's help and without.
    let mut sums: BTreeMap<String, [usize; 5]> = BTreeMap::new();
    for q in &qs {
        let (any, other) = match (q.pool, &q.asked_by, q.body) {
            ("E" | "B", Some(v), Some(b)) => (ask(w, &anchors, v, b, false), ask(w, &anchors, v, b, true)),
            ("E" | "B", ..) => (None, None),
            (pool, _, body) => {
                // An extra called a character outside the main cast was given
                // no cast name: written `~name`, and not counted as answered.
                let a = body.and_then(|b| passed(w, b)).map(|(n, s, s2)| {
                    if pool == "N" && !w.main.contains(n.as_str()) { (format!("~{n}"), s, s2) } else { (n, s, s2) }
                });
                (a.clone(), a)
            }
        };
        let flags = q.flags.join(",");
        let _ = writeln!(
            set,
            "{}\t{key}\t{}\t{}\t{}\t{}\t{}\t{}\t{flags}",
            q.pool,
            q.shot,
            q.start,
            q.end,
            q.frame,
            quad_text(&q.bbox),
            q.name
        );
        let _ = writeln!(
            today,
            "{}\t{key}\t{}\t{}\t{}\t{flags}\t{}\t{}",
            q.pool,
            q.shot,
            q.frame,
            q.name,
            text(&any),
            text(&other)
        );
        let far = if q.pool == "B" && q.flags.contains(&"far") { " far" } else { "" };
        let e = sums.entry(format!("{}{far}", q.pool)).or_default();
        e[0] += 1;
        for (k, a) in [(1, &any), (3, &other)] {
            if let Some((n, ..)) = a.as_ref().filter(|a| !a.0.starts_with('~')) {
                e[k] += 1;
                e[k + 1] += usize::from(*n == q.name);
            }
        }
    }
    let with = |ext: &str| {
        let mut p = out.as_os_str().to_owned();
        p.push(ext);
        std::path::PathBuf::from(p)
    };
    std::fs::write(with(".pools.tsv"), set).unwrap();
    std::fs::write(with(".today.tsv"), today).unwrap();
    println!(
        "{key}: {} bodies, {} with a face ({} anchors); back labels of the main cast {back_labels}, on no body {bare}",
        w.bodies.len(),
        w.faces_on.iter().filter(|f| !f.is_empty()).count(),
        anchors.len()
    );
    for (pool, [n, a, r, a2, r2]) in sums {
        println!("  {pool:6} {n:4} queries | answered {a:4} right {r:4} | without the same shot: answered {a2:4} right {r2:4}");
    }
    // Pool E's names come through the tie of faces to bodies: the readers'
    // names for a sample of its bodies (the draw's group `e`) check them.
    // (`HARD`, or a file of their own: `CHECK=<title.check.tsv>`.)
    let read: Vec<Vec<String>> =
        ["HARD", "CHECK"].iter().filter_map(|k| std::env::var(k).ok()).flat_map(|p| labels::tsv_rows(&p)).collect();
    {
        let (mut same, mut other, mut open) = (0, 0, 0);
        for v in read.into_iter().filter(|v| v[0] == key && v[2] == "e") {
            let body = w.body_at(v[3].parse().unwrap(), v[6].parse().unwrap(), &quad(&v[7]));
            let truth = body.and_then(|b| w.body_truth(b));
            match v[8].as_str() {
                "x" => open += 1,
                n if Some(n) == truth => same += 1,
                n => {
                    other += 1;
                    println!("    pool E, item {}: the labels say {}, the readers {n}", v[1], truth.unwrap_or("-"));
                }
            }
        }
        if same + other + open > 0 {
            println!("  pool E's names read again: {same} the same, {other} another answer, {open} cannot tell");
        }
    }
}

/// Pool B alone, to look at.
fn back_cmd(w: &World, labels: &[Label]) {
    let (qs, all, bare) = back(w, labels);
    let anchors = anchors_of(w);
    println!("{all} back labels of the main cast outside the spans; {bare} on no body track; {} queries", qs.len());
    for flag in ["far", "cut", "own"] {
        println!("  {flag}: {}", qs.iter().filter(|q| q.flags.contains(&flag)).count());
    }
    let (mut answered, mut right, mut bare_vec) = (0, 0, 0);
    for q in &qs {
        let a = match (&q.asked_by, q.body) {
            (Some(v), Some(b)) => ask(w, &anchors, v, b, false),
            _ => {
                bare_vec += 1;
                None
            }
        };
        if let Some((n, s, s2)) = &a {
            answered += 1;
            right += usize::from(*n == q.name);
            if detail() {
                println!("    {} {}-{} {:?}: {n} {s:.3} (next {s2:.3})", q.name, q.start, q.end, q.flags);
            }
        }
    }
    println!("  answered {answered}, right {right}; nothing to embed {bare_vec}");
}

/// The frame of `shot` to show it by: the sample nearest its middle.
fn shot_frame(shot: &FrameRange) -> Option<u32> {
    let mid = (shot.start + (shot.end - shot.start) / 2).next_multiple_of(6);
    [mid, shot.start.next_multiple_of(6)].into_iter().find(|f| shot.contains(*f))
}

/// Body tracks without any face, drawn for the readers; and the gallery
/// they name them from.
fn draw(w: &World, labels: &[Label], out: &Path) {
    std::fs::create_dir_all(out).unwrap();
    let key = key();
    let (n_m, n_o, n_e): (usize, usize, usize) = std::env::var("DRAW").ok().map_or((100, 100, 0), |v| {
        let n: Vec<usize> = v.split(',').map(|x| x.trim().parse().expect("DRAW=<m>,<o>[,<e>]")).collect();
        (n[0], n[1], n.get(2).copied().unwrap_or(0))
    });
    let main_face = |shot: FrameRange| {
        labels.iter().any(|l| {
            w.main.contains(l.name.as_str()) && !is_back(l) && l.span.0 < shot.end && shot.start < l.span.1
        })
    };
    // Every track that may be drawn, in the order they come on screen:
    // without a face tied (whatever the pass made of it), 15 % of the frame
    // high or more, outside the spans.
    let (mut m, mut o) = (Vec::new(), Vec::new());
    for (b, body) in w.bodies.iter().enumerate() {
        if !w.faces_on[b].is_empty() || body.height() < as_analysis::bodies::CLIP_MIN_HEIGHT || w.in_span(b) {
            continue;
        }
        let Some(shot) = w.shot_of(body.start) else { continue };
        if main_face(shot) { m.push(b) } else { o.push(b) }
    }
    let mut rng = Rng(0x0B0D_1E5D_2A77);
    let mut picked: Vec<(usize, &str)> = rng.pick(&m, n_m).into_iter().map(|b| (b, "m")).collect();
    picked.extend(rng.pick(&o, n_o).into_iter().map(|b| (b, "o")));
    // A third group, to check the names of pool E: bodies with a face, shown
    // at their moments without one.
    let e: Vec<usize> = easy(w).iter().filter_map(|q| q.body).collect();
    picked.extend(rng.pick(&e, n_e).into_iter().map(|b| (b, "e")));
    picked.sort_by_key(|&(b, _)| (w.bodies[b].start, b));
    println!(
        "{key}: {} body tracks, {} without a face; of those 15 % high or more, outside the spans: {} in a shot with a labelled face of the main cast, {} in other shots; drawn {} and {}; of pool E's {} bodies {}",
        w.bodies.len(),
        w.faces_on.iter().filter(|f| f.is_empty()).count(),
        m.len(),
        o.len(),
        picked.iter().filter(|p| p.1 == "m").count(),
        picked.iter().filter(|p| p.1 == "o").count(),
        e.len(),
        picked.iter().filter(|p| p.1 == "e").count()
    );

    let mut list = String::from("item\tkey\tstratum\tshot\tstart\tend\tframe\tbox\n");
    let mut items = String::from("sheet\tcell\tstratum\tframe\tboxes\tinsets\tinfo\tmark\n");
    let orange = |b: &[f32; 4]| format!("{},ff8800,4", quad_text(b));
    let inset = |f: u32, b: &[f32; 4]| format!("{f}:{}", quad_text(b));
    let plain = |f: Option<u32>| f.map_or("-".to_string(), |f| f.to_string());
    for (i, &(b, stratum)) in picked.iter().enumerate() {
        let body = &w.bodies[b];
        // A body of pool E is shown where none of its faces is.
        let seen: Vec<BodyDet> =
            if stratum == "e" { w.away(b).into_iter().copied().collect() } else { body.dets.clone() };
        let mid = seen[seen.len() / 2];
        let (first, last) = (seen[0], seen[seen.len() - 1]);
        let other = if mid.frame - first.frame >= last.frame - mid.frame { first } else { last };
        let shot = w.shot_of(body.start).expect("a shot");
        let _ = writeln!(
            list,
            "{i}\t{key}\t{stratum}\t{}\t{}\t{}\t{}\t{}",
            body.shot,
            body.start,
            body.end,
            mid.frame,
            quad_text(&mid.visible)
        );
        // The same shot at another moment: where a labelled face of the main
        // cast shows, as far from the body's moment as there is one; without
        // such a face, the shot's sample furthest from it.
        let faces: Vec<u32> = labels
            .iter()
            .filter(|l| w.main.contains(l.name.as_str()) && !is_back(l))
            .flat_map(|l| l.boxes.iter().map(|x| x.0))
            .filter(|f| shot.contains(*f) && f.is_multiple_of(3))
            .collect();
        let ends = [shot.start.next_multiple_of(6), (shot.end - 1) / 6 * 6];
        let elsewhere = faces
            .into_iter()
            .max_by_key(|f| f.abs_diff(mid.frame))
            .or_else(|| ends.into_iter().filter(|f| shot.contains(*f)).max_by_key(|f| f.abs_diff(mid.frame)))
            .filter(|f| *f != mid.frame);
        let at = w.shots.partition_point(|s| s.end <= body.start);
        let before = at.checked_sub(1).and_then(|k| shot_frame(&w.shots[k]));
        let after = w.shots.get(at + 1).and_then(shot_frame);
        let mut zoom = vec![first, mid, last];
        zoom.dedup_by_key(|d| d.frame);
        let cells: [(String, String, String, String); 6] = [
            (mid.frame.to_string(), orange(&mid.visible), String::new(), i.to_string()),
            (
                plain((other.frame != mid.frame).then_some(other.frame)),
                if other.frame == mid.frame { String::new() } else { orange(&other.visible) },
                String::new(),
                "-".into(),
            ),
            (
                "-".into(),
                String::new(),
                zoom.iter().map(|d| inset(d.frame, &d.visible)).collect::<Vec<_>>().join(";"),
                "-".into(),
            ),
            (plain(elsewhere), String::new(), String::new(), "-".into()),
            (plain(before), String::new(), String::new(), "-".into()),
            (plain(after), String::new(), String::new(), "-".into()),
        ];
        for (c, (frame, boxes, insets, mark)) in cells.into_iter().enumerate() {
            let _ = writeln!(items, "{i}\t{c}\t{stratum}\t{frame}\t{boxes}\t{insets}\titem {i} body {b}\t{mark}");
        }
    }
    std::fs::write(out.join("draw.tsv"), list).unwrap();
    std::fs::write(out.join("items.tsv"), items).unwrap();

    // The gallery: every figure of the main cast the episode's labels show,
    // by its clearest faces of three parts of the episode; for animation
    // (one outfit a figure) two of its bodies too.
    let animation = std::env::var_os("ANIME").is_some() || std::env::var_os("ANIMATED").is_some();
    let notes: HashMap<String, String> = std::env::var("GALLERY").ok().map_or_else(HashMap::new, |p| {
        let g = Gallery::read(&p);
        let mut by: HashMap<String, String> = HashMap::new();
        for v in labels::tsv_rows(&p) {
            let figure = at_level(&v[0], Some(&g), Level::Figure);
            let note = v.get(3).cloned().unwrap_or_default();
            let e = by.entry(figure).or_default();
            if e.is_empty() || v[2] == "base" {
                *e = note;
            }
        }
        by
    });
    let mut screen: BTreeMap<&str, u32> = BTreeMap::new();
    for l in labels.iter().filter(|l| {
        w.main.contains(l.name.as_str()) && !is_back(l) && w.spans.mostly_in(l.span.0, l.span.1).is_none()
    }) {
        *screen.entry(l.name.as_str()).or_default() += l.span.1 - l.span.0;
    }
    let mut figures: Vec<(&str, u32)> = screen.into_iter().collect();
    figures.sort_by_key(|&(n, f)| (std::cmp::Reverse(f), n));
    let length = w.shots.last().map_or(1, |s| s.end).max(1);
    let mut gallery = String::from("sheet\tcell\tstratum\tframe\tboxes\tinsets\tinfo\tmark\n");
    let mut names = String::from("number\tname\tseconds\tnote\n");
    let mut cell = 0;
    for (g, &(name, frames)) in figures.iter().enumerate() {
        let mut faces = Vec::new();
        for part in 0..3 {
            let clearest = (0..w.tracks.len())
                .filter(|&i| w.truth[i] == Some(name) && w.tracks[i].start * 3 / length == part)
                .filter_map(|i| tracks::clearest(&w.tracks[i].dets))
                .max_by(|x, y| x.quality.total_cmp(&y.quality));
            if let Some(d) = clearest {
                faces.push(inset(d.frame, &d.bbox));
            }
        }
        let mut cells = vec![faces];
        if animation {
            // Bodies all of whose faces are the figure's, the best attested
            // and tallest of each half of the episode: a cell of their own.
            let mut bodies = Vec::new();
            for part in 0..2 {
                let weight = |b: usize| w.faces_on[b].iter().map(|f| f.1).sum::<u32>() as f32 * w.bodies[b].height();
                let best = (0..w.bodies.len())
                    .filter(|&b| {
                        !w.faces_on[b].is_empty()
                            && w.faces_on[b].iter().all(|&(f, _)| w.truth[f] == Some(name))
                            && w.bodies[b].start * 2 / length == part
                            && w.bodies[b].showing(16.0 / 9.0).prominent()
                    })
                    .max_by(|&x, &y| weight(x).total_cmp(&weight(y)));
                if let Some(b) = best {
                    let d = w.bodies[b].dets[w.bodies[b].dets.len() / 2];
                    bodies.push(inset(d.frame, &d.visible));
                }
            }
            cells.push(bodies);
        }
        for insets in cells {
            let _ = writeln!(gallery, "{}\t{}\tg\t-\t\t{}\t{name}\t{}", cell / 6, cell % 6, insets.join(";"), g + 1);
            cell += 1;
        }
        let _ = writeln!(
            names,
            "{}\t{name}\t{:.0}\t{}",
            g + 1,
            f64::from(frames) / fps(),
            notes.get(name).map_or("", String::as_str)
        );
    }
    std::fs::write(out.join("gallery.items.tsv"), gallery).unwrap();
    std::fs::write(out.join("gallery.txt"), names).unwrap();
    println!("{} tracks → {}; {} figures in the gallery", picked.len(), out.join("items.tsv").display(), figures.len());
}

/// The readers' answers joined: `item answer [note]` per line. Where the two
/// readers agree that is the name; where they differ the third reader's
/// answer is, and `x` without one.
fn join_readers(draw: &str, a: &str, b: &str, third: &str, out: &str) {
    let answers = |path: &str| -> HashMap<String, String> {
        if path == "-" {
            return HashMap::new();
        }
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#') && !l.starts_with("item"))
            .map(|l| {
                let v: Vec<&str> = l.split('\t').collect();
                (v[0].trim().to_string(), v.get(1).unwrap_or_else(|| panic!("{path}: {l}")).trim().to_string())
            })
            .collect()
    };
    let known: Option<HashSet<String>> =
        std::env::var("NAMES").ok().map(|p| labels::tsv_rows(&p).into_iter().map(|v| v[1].clone()).collect());
    let (a, b, third) = (answers(a), answers(b), answers(third));
    for (who, set) in [("first", &a), ("second", &b), ("third", &third)] {
        for (item, name) in set {
            let ok = matches!(name.as_str(), "e" | "n" | "x") || known.as_ref().is_none_or(|k| k.contains(name));
            assert!(ok, "the {who} reader's answer for item {item} is no name of the gallery: {name}");
        }
    }
    let mut text = String::from("key\titem\tstratum\tshot\tstart\tend\tframe\tbox\tname\tfirst\tsecond\thow\n");
    let (mut same, mut decided, mut open) = (0, 0, 0);
    let mut by: BTreeMap<&str, usize> = BTreeMap::new();
    for v in labels::tsv_rows(draw) {
        let item = &v[0];
        let (x, y) = (
            a.get(item).unwrap_or_else(|| panic!("the first reader left out item {item}")),
            b.get(item).unwrap_or_else(|| panic!("the second reader left out item {item}")),
        );
        let (name, how) = if x == y {
            same += 1;
            (x.as_str(), "same")
        } else if let Some(z) = third.get(item) {
            decided += 1;
            (z.as_str(), "third")
        } else {
            open += 1;
            ("x", "open")
        };
        *by.entry(match name {
            "e" => "a person outside the gallery",
            "n" => "no person",
            "x" => "cannot tell",
            _ => "named",
        })
        .or_default() += 1;
        let _ = writeln!(
            text,
            "{}\t{item}\t{}\t{}\t{}\t{}\t{}\t{}\t{name}\t{x}\t{y}\t{how}",
            v[1], v[2], v[3], v[4], v[5], v[6], v[7]
        );
    }
    std::fs::write(out, text).unwrap();
    println!("{} items: both readers the same {same}, decided by the third {decided}, left open {open}", same + decided + open);
    println!("  {by:?}");
}

/// One query's result, as `pools` wrote it.
struct Scored {
    title: String,
    pool: String,
    name: String,
    flags: Vec<String>,
    /// Answered and right, with any anchor and without the same shot's.
    got: [(bool, bool); 2],
}

/// The 2.5 % and 97.5 % points of `v` (`None` without any value).
fn interval(mut v: Vec<f64>) -> Option<(f64, f64)> {
    v.retain(|x| x.is_finite());
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    (!v.is_empty()).then(|| (at(0.025), at(0.975)))
}

/// Queries, answered and right of `rows`; then the share answered, the share
/// right among the answered, and right of all queries.
fn shares(counts: [usize; 3]) -> [f64; 3] {
    let [n, a, r] = counts.map(|x| x as f64);
    [a / n, r / a, r / n]
}

/// The pools per title and pooled: counts, shares, and intervals drawn over
/// characters (each title's figures redrawn with all their queries, 2,000
/// times; extras have no character and get a Wilson interval on the share
/// answered).
fn table(args: &[String]) {
    let mut rows: Vec<Scored> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    for arg in args {
        let (title, files) = arg.split_once('=').expect("<title>=<today.tsv>[,<today.tsv>…]");
        titles.push(title.to_string());
        for file in files.split(',') {
            for v in labels::tsv_rows(file) {
                // `-`: no answer; `~name`: an extra called no one of the main cast.
                let yes = |k: usize| {
                    let got = v.get(k).map_or("-", String::as_str);
                    (got != "-" && !got.starts_with('~'), got == v[4])
                };
                rows.push(Scored {
                    title: title.to_string(),
                    pool: v[0].clone(),
                    name: v[4].clone(),
                    flags: v[5].split(',').map(String::from).collect(),
                    got: [yes(6), yes(9)],
                });
            }
        }
    }
    // A line of the table: its name, which rows, and whether pools E and B
    // are read without the same shot's anchors.
    type Pick = fn(&Scored) -> bool;
    let lines: [(&str, Pick, usize); 11] = [
        ("E, as today", |r| r.pool == "E", 0),
        ("E, no help from the same shot", |r| r.pool == "E", 1),
        ("B, back of the head", |r| r.pool == "B", 0),
        ("B, no help from the same shot", |r| r.pool == "B", 1),
        ("B far, no face within 2 s", |r| r.pool == "B" && r.flags.iter().any(|f| f == "far"), 0),
        ("B far, no help from the same shot", |r| r.pool == "B" && r.flags.iter().any(|f| f == "far"), 1),
        ("M, face in the shot", |r| r.pool == "M", 0),
        ("H, no face in the shot", |r| r.pool == "H", 0),
        ("H, bodies that would make a clip", |r| r.pool == "H" && r.flags.iter().any(|f| f == "clip"), 0),
        ("E + M + H (E without the same shot)", |r| matches!(r.pool.as_str(), "E" | "M" | "H"), 1),
        ("N, extras", |r| r.pool == "N", 0),
    ];
    let mut scopes: Vec<(String, Vec<&str>)> = titles.iter().map(|t| (t.clone(), vec![t.as_str()])).collect();
    scopes.push(("all titles".into(), titles.iter().map(String::as_str).collect()));
    println!("| Title | Pool | Queries | Characters | Answered | Right | Share answered | Share right | Right of all |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let mut rng = Rng(0x7AB1_E0FB_0D1E);
    for (scope, of) in &scopes {
        for (line, pick, variant) in &lines {
            let here: Vec<&Scored> = rows.iter().filter(|r| of.contains(&r.title.as_str()) && pick(r)).collect();
            if here.is_empty() {
                continue;
            }
            // Per title, per character: queries, answered, right.
            let mut by: BTreeMap<&str, BTreeMap<&str, [usize; 3]>> = BTreeMap::new();
            for r in &here {
                let e = by.entry(r.title.as_str()).or_default().entry(r.name.as_str()).or_default();
                let (answered, right) = r.got[*variant];
                e[0] += 1;
                e[1] += usize::from(answered);
                e[2] += usize::from(right);
            }
            let groups: Vec<Vec<[usize; 3]>> = by.values().map(|c| c.values().copied().collect()).collect();
            let sum = |v: &[[usize; 3]]| v.iter().fold([0; 3], |a, x| [a[0] + x[0], a[1] + x[1], a[2] + x[2]]);
            let total = sum(&groups.concat());
            let characters: usize = groups.iter().map(Vec::len).sum();
            let s = shares(total);
            let cell = |x: f64, ci: Option<(f64, f64)>| match ci {
                _ if !x.is_finite() => "–".to_string(),
                Some((lo, hi)) => format!("{x:.2} [{lo:.2}, {hi:.2}]"),
                None => format!("{x:.2}"),
            };
            let extras = here.iter().all(|r| r.name == "?");
            let cis: [Option<(f64, f64)>; 3] = if extras {
                [Some(wilson(total[1], total[0])), None, None]
            } else {
                let mut drawn: [Vec<f64>; 3] = Default::default();
                for _ in 0..2000 {
                    let mut again = Vec::new();
                    for g in &groups {
                        again.extend((0..g.len()).map(|_| g[(rng.next() % g.len() as u64) as usize]));
                    }
                    for (k, x) in shares(sum(&again)).into_iter().enumerate() {
                        drawn[k].push(x);
                    }
                }
                drawn.map(interval)
            };
            if extras {
                // No name is right for an extra: only how many got one.
                println!("| {scope} | {line} | {} | – | {} | – | {} | – | – |", total[0], total[1], cell(s[0], cis[0]));
                continue;
            }
            println!(
                "| {scope} | {line} | {} | {characters} | {} | {} | {} | {} | {} |",
                total[0],
                total[1],
                total[2],
                cell(s[0], cis[0]),
                cell(s[1], cis[1]),
                cell(s[2], cis[2])
            );
        }
    }
}

fn union_len(mut v: Vec<(u32, u32)>) -> u32 {
    v.sort_unstable();
    let (mut total, mut end) = (0u32, 0u32);
    for (a, b) in v {
        let a = a.max(end);
        if b > a {
            total += b - a;
            end = b;
        }
    }
    total
}

/// How much more of the main cast is found: screen time and shots.
fn presence(w: &World, out: Option<&Path>) {
    let mut names: Vec<&str> = w.main.iter().copied().collect();
    names.sort_by_key(|n| {
        std::cmp::Reverse(
            w.tracks.iter().zip(&w.truth).filter(|(_, t)| **t == Some(*n)).map(|(t, _)| t.end - t.start).sum::<u32>(),
        )
    });
    let mut tsv = String::from(
        "name\tsource\tshot\tstart\tend\theight\tforeground\tdark\tscore\tmid_frame\tmid_box\tbest_det\tmedian_det\tmedian_aspect\tmedian_luma\n",
    );
    println!(
        "{:10} {:>9} {:>6} | {:>8} {:>8} {:>8} | {:>9} {:>9} | (c) new shots by visible height >=50/30/15/<15 %, foreground, dark",
        "", "face s", "shots", "+(a) s", "+(b) s", "+(c) s", "+(c) shots", "of them"
    );
    let (mut tot_face, mut tot_new) = (0usize, 0usize);
    for n in names {
        let chars: HashSet<usize> = w.char_name.iter().filter(|(_, x)| **x == n).map(|(c, _)| *c).collect();
        let face: Vec<usize> = (0..w.tracks.len()).filter(|&i| w.got[i].is_some_and(|c| chars.contains(&c))).collect();
        let face_spans: Vec<(u32, u32)> = face.iter().flat_map(|&i| w.input[i].spans.clone()).collect();
        let face_shots: HashSet<u32> = face.iter().map(|&i| w.tracks[i].shot).collect();
        let a_bodies: Vec<usize> = (0..w.bodies.len())
            .filter(|&b| matches!(w.res.bodies[b], BodyAssign::Face(c) if chars.contains(&c)))
            .collect();
        let a_spans: Vec<(u32, u32)> = a_bodies.iter().flat_map(|&b| w.body_in[b].spans.clone()).collect();
        let b_spans: Vec<(u32, u32)> = w
            .res
            .promoted
            .iter()
            .filter(|(_, c)| chars.contains(c))
            .flat_map(|&(t, _)| w.input[t].spans.clone())
            .collect();
        let c_bodies: Vec<usize> = (0..w.bodies.len())
            .filter(|&b| matches!(w.res.bodies[b], BodyAssign::Reid { character: c, .. } if chars.contains(&c)))
            .collect();
        let c_spans: Vec<(u32, u32)> = c_bodies.iter().flat_map(|&b| w.body_in[b].spans.clone()).collect();
        let base = union_len(face_spans.clone());
        let with_a = union_len([face_spans.clone(), a_spans.clone()].concat());
        let with_b = union_len([face_spans.clone(), a_spans.clone(), b_spans.clone()].concat());
        let with_c = union_len([face_spans, a_spans, b_spans, c_spans].concat());
        let a_shots: HashSet<u32> = a_bodies.iter().map(|&b| w.bodies[b].shot).collect();
        let mut new_shots: HashMap<u32, Vec<usize>> = HashMap::new();
        for &b in &c_bodies {
            let s = w.bodies[b].shot;
            if !face_shots.contains(&s) && !a_shots.contains(&s) {
                new_shots.entry(s).or_default().push(b);
            }
        }
        let mut hb = [0usize; 4];
        let (mut fg, mut dark, mut clips) = (0, 0, 0);
        for bs in new_shots.values() {
            clips += usize::from(bs.iter().any(|&b| w.bodies[b].showing(16.0 / 9.0).prominent()));
            let best =
                bs.iter().copied().max_by(|&x, &y| w.bodies[x].height().total_cmp(&w.bodies[y].height())).unwrap();
            let h = w.bodies[best].height();
            hb[if h >= 0.5 {
                0
            } else if h >= 0.3 {
                1
            } else if h >= 0.15 {
                2
            } else {
                3
            }] += 1;
            let (f, d) = flags(w, best);
            fg += usize::from(f);
            dark += usize::from(d);
        }
        for &b in &c_bodies {
            let (f, d) = flags(w, b);
            let BodyAssign::Reid { score, .. } = w.res.bodies[b] else { unreachable!() };
            let body = &w.bodies[b];
            let median = |mut v: Vec<f32>| {
                v.sort_by(f32::total_cmp);
                v[v.len() / 2]
            };
            let mid = body.dets[body.dets.len() / 2];
            let v = mid.visible;
            let _ = writeln!(
                tsv,
                "{n}\tc\t{}\t{}\t{}\t{:.3}\t{}\t{}\t{score:.3}\t{}\t{:.4},{:.4},{:.4},{:.4}\t{:.2}\t{:.2}\t{:.2}\t{:.0}",
                body.shot,
                body.start,
                body.end,
                body.height(),
                u8::from(f),
                u8::from(d),
                mid.frame,
                v[0],
                v[1],
                v[2],
                v[3],
                body.dets.iter().map(|d| d.score).fold(0.0, f32::max),
                median(body.dets.iter().map(|d| d.score).collect()),
                median(
                    body.dets
                        .iter()
                        .map(|d| (d.visible[2] - d.visible[0]) * 16.0 / ((d.visible[3] - d.visible[1]) * 9.0))
                        .collect()
                ),
                median(body.dets.iter().map(|d| d.luma).collect()),
            );
        }
        tot_face += face_shots.len();
        tot_new += new_shots.len();
        println!(
            "{n:10} {:9.1} {:6} | {:8.1} {:8.1} {:8.1} | {:9} {:>9} | {:?} fg {fg} dark {dark} clips {clips}",
            base as f64 / fps(),
            face_shots.len(),
            (with_a - base) as f64 / fps(),
            (with_b - with_a) as f64 / fps(),
            (with_c - with_b) as f64 / fps(),
            new_shots.len(),
            pct(new_shots.len(), face_shots.len()),
            hb
        );
        if detail() && !new_shots.is_empty() {
            let mut v: Vec<&u32> = new_shots.keys().collect();
            v.sort();
            println!("    new shots: {v:?}");
        }
    }
    println!("main cast: {tot_face} shots with a face, {tot_new} more by clothes ({})", pct(tot_new, tot_face));
    if let Some(out) = out {
        std::fs::create_dir_all(out).unwrap();
        std::fs::write(out.join("presence.tsv"), tsv).unwrap();
    }
}

/// A body that is a blurred shoulder in the foreground, and one that is dark.
fn flags(w: &World, b: usize) -> (bool, bool) {
    let body = &w.bodies[b];
    let fg = body.dets.iter().filter(|d| d.blurred_foreground()).count();
    let mut lumas: Vec<f32> = body.dets.iter().map(|d| d.luma).collect();
    lumas.sort_by(f32::total_cmp);
    (2 * fg > body.dets.len(), lumas[lumas.len() / 2] < 35.0)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a[0] == "audit" {
        return match a.len() {
            3 => audit(&a[1], &a[2]),
            6 => join_readers(&a[1], &a[2], &a[3], &a[4], &a[5]),
            _ => panic!("audit <items.tsv> <verdicts.tsv>, or audit <draw.tsv> <first> <second> <third|-> <out>"),
        };
    }
    if a[0] == "table" {
        return table(&a[1..]);
    }
    let said = read_truth(&a[4]);
    let labels = &said.labels[..];
    let t = std::time::Instant::now();
    let w = build(&a[1], &a[2], Path::new(&a[3]), &said);
    let found = w.res.bodies.iter().filter(|x| matches!(x, BodyAssign::Reid { .. })).count();
    let face = w.res.bodies.iter().filter(|x| matches!(x, BodyAssign::Face(_))).count();
    eprintln!(
        "built in {:.1?}: {} body tracks, {face} by face, {found} by clothes, {} faces named by their body",
        t.elapsed(),
        w.bodies.len(),
        w.res.promoted.len()
    );
    let out = a.get(5).map(Path::new);
    match a[0].as_str() {
        "recall" => recall(&w, labels),
        "tracks" => tracks_cmd(&w),
        "score" => score(&w, labels),
        "reid" => reid(&w),
        "presence" => presence(&w, out),
        "sheets" => sheets(&w, out.expect("out_dir")),
        "back" => back_cmd(&w, labels),
        "draw" => draw(&w, labels, out.expect("out_dir")),
        "pools" => pools(&w, labels, out.expect("out_prefix")),
        other => panic!("unknown command {other}"),
    }
}

/// Cells for looking at: frames with boxes and small pictures of who the
/// pipeline says it is (drawn by `bodies_render`).
fn sheets(w: &World, out: &Path) {
    std::fs::create_dir_all(out).unwrap();
    let mut rng = Rng(0x5EED_B0D1E5);
    let mut items = String::from("sheet\tcell\tstratum\tframe\tboxes\tinsets\tinfo\n");
    let mut cell = 0usize;
    let mut add = |stratum: &str,
                   frame: u32,
                   boxes: Vec<([f32; 4], &str, u8)>,
                   insets: Vec<(u32, [f32; 4])>,
                   info: String| {
        let b: Vec<String> =
            boxes.iter().map(|(x, c, t)| format!("{:.4},{:.4},{:.4},{:.4},{c},{t}", x[0], x[1], x[2], x[3])).collect();
        let i: Vec<String> =
            insets.iter().map(|(f, x)| format!("{f}:{:.4},{:.4},{:.4},{:.4}", x[0], x[1], x[2], x[3])).collect();
        let _ =
            writeln!(items, "{}\t{}\t{stratum}\t{frame}\t{}\t{}\t{info}", cell / 6, cell % 6, b.join(";"), i.join(";"));
        cell += 1;
    };
    // The clearest face of each character: its picture.
    let mut clearest: HashMap<usize, (f32, u32, [f32; 4])> = HashMap::new();
    for (i, t) in w.tracks.iter().enumerate() {
        let (Some(c), Some(d)) = (w.got[i], tracks::clearest(&t.dets)) else { continue };
        let e = clearest.entry(c).or_insert((d.quality, d.frame, d.bbox));
        if d.quality > e.0 {
            *e = (d.quality, d.frame, d.bbox);
        }
    }
    let face_of: HashMap<usize, (u32, [f32; 4])> = clearest.into_iter().map(|(c, (_, f, b))| (c, (f, b))).collect();
    let has_face_at =
        |b: usize, f: u32| w.faces_on[b].iter().any(|&(t, _)| w.input[t].spans.iter().any(|&(a, z)| a <= f && f < z));
    let mid_det = |b: usize, pick: &dyn Fn(&BodyDet) -> bool| -> Option<BodyDet> {
        let v: Vec<&BodyDet> = w.bodies[b].dets.iter().filter(|d| pick(d)).collect();
        v.get(v.len() / 2).map(|d| **d)
    };
    // S1 (a): a body with a face, at a moment without one.
    let s1: Vec<usize> = (0..w.bodies.len())
        .filter(|&b| {
            matches!(w.res.bodies[b], BodyAssign::Face(c) if w.main.contains(w.name(c)))
                && w.bodies[b].dets.iter().any(|d| !has_face_at(b, d.frame))
        })
        .collect();
    for b in rng.pick(&s1, 30) {
        let BodyAssign::Face(c) = w.res.bodies[b] else { unreachable!() };
        let d = mid_det(b, &|d| !has_face_at(b, d.frame)).unwrap();
        let tf = w.faces_on[b].iter().find(|(t, _)| w.got[*t] == Some(c)).map_or(w.faces_on[b][0].0, |x| x.0);
        let fd = tracks::clearest(&w.tracks[tf].dets).unwrap();
        add(
            "a",
            d.frame,
            vec![(d.visible, "ff8800", 4)],
            vec![(fd.frame, fd.bbox)],
            format!("body {b} = {} (truth {})", w.name(c), w.body_truth(b).unwrap_or("-")),
        );
    }
    // S2 (b): faces their body named.
    let s2: Vec<(usize, usize)> = w.res.promoted.clone();
    for (t, c) in rng.pick(&s2, 42) {
        let d = tracks::clearest(&w.tracks[t].dets).unwrap();
        let reference = face_of.get(&c).map(|&(f, x)| vec![(f, x)]).unwrap_or_default();
        add(
            "b",
            d.frame,
            vec![(d.bbox, "ff8800", 3)],
            reference,
            format!("face {t} = {} (truth {})", w.name(c), w.truth[t].unwrap_or("-")),
        );
    }
    // S3 (c): bodies found by their clothes, with the body that matched.
    let anchors: Vec<Anchor> = (0..w.bodies.len())
        .filter_map(|b| match w.res.bodies[b] {
            BodyAssign::Face(c) => Some(Anchor {
                body: b,
                character: c,
                shot: w.bodies[b].shot,
                first: w.bodies[b].start,
                last: w.bodies[b].end,
                reid: [w.body_in[b].reid.as_ref(), w.body_in[b].reid_away.as_ref()],
            }),
            _ => None,
        })
        .collect();
    let s3: Vec<usize> = (0..w.bodies.len()).filter(|&b| matches!(w.res.bodies[b], BodyAssign::Reid { .. })).collect();
    let s3 = if s3.len() <= 150 { s3 } else { rng.pick(&s3, 130) };
    for b in s3 {
        let BodyAssign::Reid { character: c, score, runner_up } = w.res.bodies[b] else { unreachable!() };
        let d = mid_det(b, &|_| true).unwrap();
        let q = w.body_in[b].reid.as_ref().unwrap();
        // The anchor that matched best.
        let best = anchors
            .iter()
            .filter(|a| a.character == c)
            .max_by(|x, y| {
                let s = |a: &Anchor| a.reid.iter().flatten().map(|e| dot(q, e)).fold(f32::NEG_INFINITY, f32::max);
                s(x).total_cmp(&s(y))
            })
            .map(|a| a.body);
        let mut insets = Vec::new();
        if let Some(a) = best {
            let ad = mid_det(a, &|_| true).unwrap();
            insets.push((ad.frame, ad.visible));
        }
        if let Some(&(f, x)) = face_of.get(&c) {
            insets.push((f, x));
        }
        add(
            "c",
            d.frame,
            vec![(d.visible, "ff8800", 4)],
            insets,
            format!("body {b} = {} by clothes {score:.3} (next {runner_up:.3})", w.name(c)),
        );
    }
    // S4: shots, their middle sample, everyone coloured by who the pipeline says they are.
    let palette = [
        "e6194b", "3cb44b", "4363d8", "f58231", "911eb4", "42d4f4", "f032e6", "bfef45", "fabed4", "469990", "dcbeff",
        "9a6324",
    ];
    let mut colour: HashMap<usize, &str> = HashMap::new();
    let mut cast: Vec<usize> = w.char_name.keys().copied().filter(|c| w.main.contains(w.name(*c))).collect();
    cast.sort_unstable();
    for (k, c) in cast.iter().enumerate() {
        colour.insert(*c, palette[k % palette.len()]);
    }
    let body_shots: Vec<u32> = {
        let mut v: Vec<u32> = (0..w.bodies.len())
            .filter(|&b| {
                matches!(w.res.bodies[b], BodyAssign::Reid { .. })
                    || (matches!(w.res.bodies[b], BodyAssign::Face(_))
                        && w.bodies[b].dets.iter().any(|d| !has_face_at(b, d.frame)))
            })
            .map(|b| w.bodies[b].shot)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let all_shots: Vec<u32> = w.shots.iter().filter(|s| s.len() >= 24).map(|s| s.start).collect();
    let mut chosen = rng.pick(&body_shots, 20);
    chosen.extend(rng.pick(&all_shots, 20));
    chosen.sort_unstable();
    chosen.dedup();
    for s in chosen {
        let shot = w.shot_of(s).unwrap();
        let frames: Vec<u32> = w.samples.iter().map(|x| x.frame).filter(|f| shot.contains(*f)).collect();
        if frames.is_empty() {
            continue;
        }
        for k in [frames.len() / 2] {
            let f = frames[k];
            let mut boxes = Vec::new();
            let mut who = Vec::new();
            for (i, t) in w.tracks.iter().enumerate().filter(|(_, t)| t.shot == s) {
                if let Some(d) = t.dets.iter().find(|d| d.frame == f) {
                    let c = w.got[i].and_then(|c| colour.get(&c).copied()).unwrap_or("808080");
                    boxes.push((d.bbox, c, 2u8));
                }
            }
            for b in (0..w.bodies.len()).filter(|&b| w.bodies[b].shot == s) {
                let Some(x) = w.bodies[b].box_at(f, 24) else { continue };
                let c = w.character_of(b);
                if let Some(c) = c {
                    who.push(w.name(c).to_string());
                }
                let col = c.and_then(|c| colour.get(&c).copied()).unwrap_or("808080");
                boxes.push((x, col, if matches!(w.res.bodies[b], BodyAssign::Reid { .. }) { 6 } else { 4 }));
            }
            for (i, _) in w.tracks.iter().enumerate().filter(|(_, t)| t.shot == s) {
                if let Some(c) = w.got[i] {
                    who.push(w.name(c).to_string());
                }
            }
            who.sort();
            who.dedup();
            let legend: Vec<(u32, [f32; 4])> = who
                .iter()
                .filter_map(|n| {
                    w.char_name.iter().find(|(_, x)| **x == n.as_str()).and_then(|(c, _)| face_of.get(c).copied())
                })
                .take(4)
                .collect();
            add("s", f, boxes, legend, format!("shot {s}: says {}", who.join(",")));
        }
    }
    std::fs::write(out.join("items.tsv"), items).unwrap();
    println!("{cell} cells in {} sheets → {}", cell.div_ceil(6), out.join("items.tsv").display());
}

/// Wilson score interval (95 %) of `k` successes in `n`.
fn wilson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let (p, n, z) = (k as f64 / n as f64, n as f64, 1.96f64);
    let d = 1.0 + z * z / n;
    let c = (p + z * z / (2.0 * n)) / d;
    let h = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / d;
    (c - h, c + h)
}

/// Precision per stratum from verdicts (`sheet cell y|n|u [note]`).
fn audit(items: &str, verdicts: &str) {
    let strata: HashMap<(String, String), String> = std::fs::read_to_string(items)
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| {
            let v: Vec<&str> = l.split('\t').collect();
            ((v[0].to_string(), v[1].to_string()), v[2].to_string())
        })
        .collect();
    let mut by: HashMap<String, (usize, usize, usize)> = HashMap::new();
    for l in std::fs::read_to_string(verdicts).unwrap().lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let v: Vec<&str> = l.split('\t').collect();
        let Some(s) = strata.get(&(v[0].to_string(), v[1].to_string())) else { panic!("no cell {} {}", v[0], v[1]) };
        let e = by.entry(s.clone()).or_default();
        match v[2].trim() {
            "y" => e.0 += 1,
            "n" => e.1 += 1,
            _ => e.2 += 1,
        }
    }
    let mut keys: Vec<&String> = by.keys().collect();
    keys.sort();
    for k in keys {
        let (y, n, u) = by[k];
        let (lo, hi) = wilson(y, y + n);
        println!(
            "{k}: {y} right, {n} wrong, {u} unclear → precision {} (95 % {:.1}-{:.1} %)",
            pct(y, y + n),
            100.0 * lo,
            100.0 * hi
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(name: &str, span: (u32, u32), flags: &[&str]) -> Label {
        Label {
            span,
            boxes: vec![(span.0, [0.4, 0.1, 0.5, 0.3])],
            name: name.into(),
            flags: flags.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn the_back_of_a_head_is_no_face_in_the_shot() {
        let labels = [label("ann", (100, 160), &[]), label("bob", (100, 160), &["back"]), label("bob", (400, 460), &[])];
        let shot = FrameRange::new(90, 200);
        assert!(face_in_shot(&labels, "ann", shot));
        // Bob's head is in the shot, from behind only: his face is in another.
        assert!(!face_in_shot(&labels, "bob", shot));
        assert!(face_in_shot(&labels, "bob", FrameRange::new(380, 470)));
        // A label that ends where the shot starts is not in it.
        assert!(!face_in_shot(&labels, "ann", FrameRange::new(160, 300)));
    }

    #[test]
    fn shares_are_of_all_queries_and_of_the_answered() {
        // 84 queries, 24 answered, 21 right: what the README once called
        // "25 % answered" is right answers of all queries.
        let [answered, right, of_all] = shares([84, 24, 21]);
        assert!((answered - 24.0 / 84.0).abs() < 1e-12);
        assert!((right - 0.875).abs() < 1e-12);
        assert!((of_all - 0.25).abs() < 1e-12);
        // Nothing answered: no share right to give.
        assert!(shares([10, 0, 0])[1].is_nan());
    }

    #[test]
    fn an_interval_leaves_out_what_is_no_number() {
        let v: Vec<f64> = (0..=100).map(f64::from).chain([f64::NAN]).collect();
        assert_eq!(interval(v), Some((3.0, 98.0)));
        assert_eq!(interval(vec![f64::NAN]), None);
        assert_eq!(interval(vec![0.5]), Some((0.5, 0.5)));
    }

    #[test]
    fn a_shot_is_shown_by_a_sample_inside_it() {
        assert_eq!(shot_frame(&FrameRange::new(100, 160)), Some(132));
        // The middle's sample lies past a short shot's end: its first sample.
        assert_eq!(shot_frame(&FrameRange::new(5, 11)), Some(6));
        // No sample at all in a shot of a few frames.
        assert_eq!(shot_frame(&FrameRange::new(7, 11)), None);
    }

    #[test]
    fn boxes_read_back_as_written() {
        let b = [0.1234, 0.5, 0.75, 1.0];
        assert_eq!(quad(&quad_text(&b)), b);
    }
}
