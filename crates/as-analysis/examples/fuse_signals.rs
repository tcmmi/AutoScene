//! Dev tool: run shot fusion on saved signals (`shot_signals`), list
//! everything that isn't a plain shot, and optionally compare the hard cuts
//! with a reference list (a JSON file whose first array holds cut frames) or
//! print the raw signals of a frame range (`--tsv FROM TO`), or write the
//! segments as `bb.segments.tsv` (`--tsv <out>`: `start  end  kind` rows, no
//! header, kind `shot`, `dissolve`, `fade_out`, `fade_in`, `black` or `flash`),
//! the shot list `faces_run`, `fast_run` and the anime bench read, or score
//! them against the boundaries checked by eye in `bench/shots`
//! (`--bench <dir> <episode>`; exits with 1 when a labelled cut is merged, a
//! dialogue window is split, or more spans are split or lost than its
//! `limits.tsv` allows, see its README).
//! `cargo run -p as-analysis --example fuse_signals -- <in.sig> [--anime] [--ref cuts.json] [--tsv FROM TO | --tsv out.tsv] [--bench bench/shots <episode>]`

use std::path::Path;

use as_analysis::{FusionParams, Segment, SegmentKind, TransitionKind, fuse, signals};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name);
    let tsv = tsv(&args);
    // The bench is read first, so a wrong folder or episode fails at once.
    let bench = bench_args(&args).map(|(dir, episode)| {
        let bench = Bench::load(Path::new(dir)).unwrap_or_else(|e| panic!("{e}"));
        assert!(bench.has(episode), "{dir} has no rows for {episode}; its episodes: {}", bench.episodes().join(", "));
        assert!(bench.limit(episode).is_some(), "{dir}/limits.tsv has no row for {episode}");
        (bench, episode)
    });
    let sig = signals::decode(&std::fs::read(&args[1]).unwrap()).expect("a signals file");
    if let Some(Tsv::Raw(from, to)) = tsv {
        println!("f\tsingle\tmany\tmu\tsd\td1\td2\td3\td4\thist\tdark\tlight");
        for (f, x) in sig.iter().enumerate().take(to.min(sig.len())).skip(from) {
            println!(
                "{f}\t{:.3}\t{:.3}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.3}\t{:.2}\t{:.2}",
                x.single, x.many, x.mu, x.sd, x.diff[0], x.diff[1], x.diff[2], x.diff[3], x.hist, x.dark, x.light
            );
        }
        return;
    }
    let params = if flag("--anime").is_some() { FusionParams::anime() } else { FusionParams::live() };
    let segs = fuse(&sig, &params);
    let count = |k: fn(&SegmentKind) -> bool| segs.iter().filter(|s| k(&s.kind)).count();
    println!(
        "{} frames → {} segments: {} shots ({} tiny), {} transitions, {} black, {} flash",
        sig.len(),
        segs.len(),
        count(|k| *k == SegmentKind::Shot),
        segs.iter().filter(|s| s.tiny).count(),
        count(|k| matches!(k, SegmentKind::Transition(_))),
        count(|k| *k == SegmentKind::Black),
        count(|k| *k == SegmentKind::Flash),
    );
    let hard = hard_cuts(&segs);
    println!("hard cuts: {}", hard.len());
    if bench.is_none() {
        for s in segs.iter().filter(|s| !s.is_shot() || s.tiny) {
            println!("  {:>6}..{:<6} {:?}{}", s.start, s.end, s.kind, if s.tiny { " (tiny)" } else { "" });
        }
    }
    if let Some(i) = flag("--ref") {
        let j = std::fs::read_to_string(&args[i + 1]).unwrap();
        let start = j.find('[').unwrap();
        let end = j[start..].find(']').unwrap() + start;
        let reference: Vec<u32> = j[start + 1..end].split(',').filter_map(|x| x.trim().parse().ok()).collect();
        let missing: Vec<u32> = reference.iter().copied().filter(|&c| !near(c, &hard)).collect();
        let extra: Vec<u32> = hard.iter().copied().filter(|&c| !near(c, &reference)).collect();
        println!(
            "reference {} | matched {} | missing {}: {:?}",
            reference.len(),
            reference.len() - missing.len(),
            missing.len(),
            missing
        );
        println!("extra {}: {:?}", extra.len(), extra);
    }
    if let Some(Tsv::Write(out)) = tsv {
        let rows: String = segs.iter().map(|s| format!("{}\t{}\t{}\n", s.start, s.end, kind_name(&s.kind))).collect();
        std::fs::write(out, rows).unwrap();
        println!("wrote {} segments to {out}", segs.len());
    }
    if let Some((bench, episode)) = bench {
        let limit = bench.limit(episode).expect("checked when the bench was read");
        let score = score(&bench, episode, &segs);
        print!("{}", score.report(&bench, episode, limit));
        if !score.passes(limit) {
            std::process::exit(1);
        }
    }
}

/// What `--tsv` asks for.
#[derive(Debug, PartialEq)]
enum Tsv<'a> {
    /// Print the raw signals of frames `FROM..TO`.
    Raw(usize, usize),
    /// Write the segments to this file.
    Write(&'a str),
}

/// `--tsv FROM TO` (two numbers) or `--tsv <out>`. A lone number or a flag
/// after `--tsv` is refused, so a typo never creates a file.
fn tsv(args: &[String]) -> Option<Tsv<'_>> {
    let i = args.iter().position(|a| a == "--tsv")?;
    let next = |k: usize| args.get(i + k).map(String::as_str);
    let number = |k: usize| next(k).and_then(|a| a.parse::<usize>().ok());
    if let (Some(from), Some(to)) = (number(1), number(2)) {
        return Some(Tsv::Raw(from, to));
    }
    let out = next(1).expect("--tsv needs FROM TO or an output file");
    assert!(
        out.parse::<f64>().is_err() && !out.starts_with("--"),
        "--tsv {out}: FROM TO takes two numbers, and an output file is neither a number nor a flag"
    );
    Some(Tsv::Write(out))
}

/// `--bench <dir> <episode>`: the bench folder and the episode the signals
/// are of. Both are needed, and neither may be a flag.
fn bench_args(args: &[String]) -> Option<(&str, &str)> {
    let i = args.iter().position(|a| a == "--bench")?;
    let next = |k: usize| args.get(i + k).map(String::as_str).filter(|a| !a.starts_with("--"));
    match (next(1), next(2)) {
        (Some(dir), Some(episode)) => Some((dir, episode)),
        _ => panic!("--bench needs the bench folder and the episode, as in --bench bench/shots bb_s01e01"),
    }
}

/// A segment's kind as `bb.segments.tsv` names it.
fn kind_name(kind: &SegmentKind) -> &'static str {
    match kind {
        SegmentKind::Shot => "shot",
        SegmentKind::Transition(TransitionKind::Dissolve) => "dissolve",
        SegmentKind::Transition(TransitionKind::FadeOut) => "fade_out",
        SegmentKind::Transition(TransitionKind::FadeIn) => "fade_in",
        SegmentKind::Black => "black",
        SegmentKind::Flash => "flash",
    }
}

/// How far, in frames, a boundary the fusion found may lie from one a
/// person marked and still be that boundary (`--ref` and `--bench`).
const NEAR: u32 = 3;

fn near(frame: u32, list: &[u32]) -> bool {
    list.iter().any(|&b| frame.abs_diff(b) <= NEAR)
}

/// The frames where one segment ends and the next begins with no blend
/// between them: neither side is a transition or a flash.
fn hard_cuts(segs: &[Segment]) -> Vec<u32> {
    let boundary = |k: &SegmentKind| !matches!(k, SegmentKind::Transition(_) | SegmentKind::Flash);
    segs.windows(2).filter(|w| boundary(&w[0].kind) && boundary(&w[1].kind)).map(|w| w[1].start).collect()
}

/// `bench/shots`: shot boundaries a person checked by eye (see its README).
#[derive(Debug)]
struct Bench {
    /// `cuts.tsv`: hard cuts checked by eye.
    cuts: Vec<Cut>,
    /// `windows.tsv`: dialogue runs in which every boundary was checked.
    windows: Vec<Window>,
    /// `transitions.tsv`: the gradual transitions and flashes the fusion put
    /// between two shots, each judged a real transition or one shot.
    spans: Vec<Span>,
    /// `limits.tsv`: how many spans each episode may leave split or lose.
    limits: Vec<Limit>,
}

/// A row of `cuts.tsv`.
#[derive(Debug, PartialEq)]
struct Cut {
    episode: String,
    /// The first frame of the new shot.
    frame: u32,
    /// `cut`; `false` for `no_cut`, a boundary that the fusion finds in what
    /// is one shot.
    real: bool,
}

/// A row of `windows.tsv`: frames `[from, to)`, whose boundaries are
/// exactly the `cuts` cut rows inside.
#[derive(Debug, PartialEq)]
struct Window {
    episode: String,
    from: u32,
    to: u32,
    cuts: u32,
}

/// A row of `transitions.tsv`: the fusion's segment `[start, end)`.
#[derive(Debug, PartialEq)]
struct Span {
    episode: String,
    start: u32,
    end: u32,
    verdict: Verdict,
}

/// A row of `limits.tsv`: the most `one_shot` spans the fusion may leave
/// split, and `transition` spans it may lose, in one episode. They are the
/// numbers of the fusion as it is, so a change that does worse on either
/// fails, and a change that trades one for the other says so in that file.
#[derive(Debug, PartialEq)]
struct Limit {
    episode: String,
    still_split: u32,
    lost: u32,
}

/// What a person saw in a span.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Verdict {
    /// A real dissolve, wipe, fade or blend, or a cut inside the span.
    Transition,
    /// The pictures before and after belong to one continuous shot.
    OneShot,
    /// Not scored.
    Unsure,
}

impl Bench {
    fn load(dir: &Path) -> Result<Bench, String> {
        let read = |name: &str| {
            let path = dir.join(name);
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        Bench::parse(&read("cuts.tsv")?, &read("windows.tsv")?, &read("transitions.tsv")?, &read("limits.tsv")?)
    }

    fn parse(cuts: &str, windows: &str, transitions: &str, limits: &str) -> Result<Bench, String> {
        let cuts = rows(cuts, "cuts.tsv", &["episode", "frame", "verdict", "picked", "note"])?
            .into_iter()
            .map(|(line, r)| {
                let real = match r[2] {
                    "cut" => true,
                    "no_cut" => false,
                    v => return Err(format!("cuts.tsv:{line}: verdict {v:?} is neither cut nor no_cut")),
                };
                // Why it was checked: A, B, C or R, one or several.
                if r[3].is_empty() || !r[3].chars().all(|p| "ABCR".contains(p)) {
                    return Err(format!("cuts.tsv:{line}: picked {:?} isn't made of A, B, C and R", r[3]));
                }
                let frame = number(r[1], "cuts.tsv", line)?;
                Ok(Cut { episode: r[0].into(), frame, real })
            })
            .collect::<Result<_, String>>()?;
        let windows = rows(windows, "windows.tsv", &["episode", "from", "to", "cuts", "note"])?
            .into_iter()
            .map(|(line, r)| {
                let n = |i: usize| number(r[i], "windows.tsv", line);
                Ok(Window { episode: r[0].into(), from: n(1)?, to: n(2)?, cuts: n(3)? })
            })
            .collect::<Result<_, String>>()?;
        let spans = rows(transitions, "transitions.tsv", &["episode", "start", "end", "verdict", "note"])?
            .into_iter()
            .map(|(line, r)| {
                let verdict = match r[3] {
                    "transition" => Verdict::Transition,
                    "one_shot" => Verdict::OneShot,
                    "unsure" => Verdict::Unsure,
                    v => {
                        return Err(format!(
                            "transitions.tsv:{line}: verdict {v:?} is none of transition, one_shot, unsure"
                        ));
                    }
                };
                let n = |i: usize| number(r[i], "transitions.tsv", line);
                Ok(Span { episode: r[0].into(), start: n(1)?, end: n(2)?, verdict })
            })
            .collect::<Result<_, String>>()?;
        let limits = rows(limits, "limits.tsv", &["episode", "still_split", "lost", "note"])?
            .into_iter()
            .map(|(line, r)| {
                let n = |i: usize| r[i].parse().map_err(|_| format!("limits.tsv:{line}: {:?} is not a count", r[i]));
                Ok(Limit { episode: r[0].into(), still_split: n(1)?, lost: n(2)? })
            })
            .collect::<Result<_, String>>()?;
        Ok(Bench { cuts, windows, spans, limits })
    }

    fn has(&self, episode: &str) -> bool {
        self.episodes().contains(&episode)
    }

    fn limit(&self, episode: &str) -> Option<&Limit> {
        self.limits.iter().find(|l| l.episode == episode)
    }

    /// Every episode named in the three files, in the order they come.
    fn episodes(&self) -> Vec<&str> {
        let all = self.cuts.iter().map(|c| c.episode.as_str());
        let all = all.chain(self.windows.iter().map(|w| w.episode.as_str()));
        let mut out: Vec<&str> = Vec::new();
        for e in all.chain(self.spans.iter().map(|s| s.episode.as_str())) {
            if !out.contains(&e) {
                out.push(e);
            }
        }
        out
    }
}

/// The rows of a tab-separated bench file after its header, which must be
/// `header`, with their line numbers. Empty lines are skipped.
fn rows<'a>(text: &'a str, file: &str, header: &[&str]) -> Result<Vec<(usize, Vec<&'a str>)>, String> {
    let mut lines = text.lines().enumerate();
    let first = lines.next().map(|(_, l)| l).unwrap_or_default();
    if first.split('\t').ne(header.iter().copied()) {
        return Err(format!("{file}: the header is {first:?}, expected {:?}", header.join("\t")));
    }
    lines
        .filter(|(_, l)| !l.is_empty())
        .map(|(i, l)| {
            let cells: Vec<&str> = l.split('\t').collect();
            if cells.len() == header.len() {
                Ok((i + 1, cells))
            } else {
                Err(format!("{file}:{}: {} columns, expected {}", i + 1, cells.len(), header.len()))
            }
        })
        .collect()
}

fn number(cell: &str, file: &str, line: usize) -> Result<u32, String> {
    cell.parse().map_err(|_| format!("{file}:{line}: {cell:?} is not a frame number"))
}

/// How fused segments do against one episode of the bench.
#[derive(Debug, Default, PartialEq)]
struct Score {
    /// `cut` rows with no hard cut within `NEAR` frames: two shots became
    /// one, or the cut became a blend.
    merged: Vec<u32>,
    /// Hard cuts inside a dialogue window that are no labelled cut, and the
    /// starts of transitions and flashes there: one shot became two.
    split: Vec<u32>,
    /// `no_cut` rows where a hard cut remains.
    known_false: Vec<u32>,
    /// `cut` rows whose hard cut is 2 or 3 frames away: (labelled, found).
    misplaced: Vec<(u32, u32)>,
    /// The `one_shot` spans that a boundary still splits.
    still_split: Vec<u32>,
    one_shot: usize,
    /// The `transition` spans with no boundary of any kind left near them.
    lost: Vec<u32>,
    transitions: usize,
}

/// Scores the segments of one episode: hard cuts against the cuts checked by
/// eye, every boundary inside a dialogue window, and the judged spans.
fn score(bench: &Bench, episode: &str, segs: &[Segment]) -> Score {
    let hard = hard_cuts(segs);
    let blends: Vec<&Segment> =
        segs.iter().filter(|s| matches!(s.kind, SegmentKind::Transition(_) | SegmentKind::Flash)).collect();
    let mut score = Score::default();
    let cuts: Vec<&Cut> = bench.cuts.iter().filter(|c| c.episode == episode).collect();
    for c in &cuts {
        let found = hard.iter().copied().filter(|&h| h.abs_diff(c.frame) <= NEAR).min_by_key(|&h| h.abs_diff(c.frame));
        match (c.real, found) {
            (true, None) => score.merged.push(c.frame),
            (true, Some(h)) if h.abs_diff(c.frame) > 1 => score.misplaced.push((c.frame, h)),
            (false, Some(_)) => score.known_false.push(c.frame),
            _ => {}
        }
    }
    let labelled: Vec<u32> = cuts.iter().filter(|c| c.real).map(|c| c.frame).collect();
    for w in bench.windows.iter().filter(|w| w.episode == episode) {
        // Inside, away from the edges: the window's first and last boundary
        // aren't labelled, and moving one of them by a frame or two splits
        // nothing.
        let inside = |start: u32, end: u32| start + NEAR < w.to && end > w.from + NEAR;
        score.split.extend(hard.iter().copied().filter(|&h| inside(h, h) && !near(h, &labelled)));
        score.split.extend(blends.iter().filter(|b| inside(b.start, b.end)).map(|b| b.start));
    }
    score.split.sort_unstable();
    score.split.dedup();
    for s in bench.spans.iter().filter(|s| s.episode == episode) {
        // A boundary of any kind within `NEAR` frames keeps the two sides of
        // the span apart.
        let (lo, hi) = (s.start.saturating_sub(NEAR), s.end + NEAR);
        let apart = hard.iter().any(|&h| (lo..=hi).contains(&h))
            || segs.iter().any(|g| !g.is_shot() && g.start <= hi && g.end > lo);
        match s.verdict {
            Verdict::OneShot => {
                score.one_shot += 1;
                if apart {
                    score.still_split.push(s.start);
                }
            }
            Verdict::Transition => {
                score.transitions += 1;
                if !apart {
                    score.lost.push(s.start);
                }
            }
            Verdict::Unsure => {}
        }
    }
    score
}

impl Score {
    /// No labelled cut merged, nothing split inside a dialogue window, and no
    /// more spans left split or lost than the episode's limits.
    fn passes(&self, limit: &Limit) -> bool {
        self.failures(limit).is_empty()
    }

    /// What fails, one phrase each.
    fn failures(&self, limit: &Limit) -> Vec<String> {
        let mut out = Vec::new();
        if !self.merged.is_empty() {
            out.push("a labelled cut is merged".to_string());
        }
        if !self.split.is_empty() {
            out.push("a dialogue window is split".to_string());
        }
        for (what, n, max) in self.trade(limit) {
            if n > max {
                out.push(format!("{what} {n}, over its limit of {max}"));
            }
        }
        out
    }

    /// The spans left split and lost, each with its limit.
    fn trade(&self, limit: &Limit) -> [(&'static str, usize, usize); 2] {
        [
            ("one-shot spans still split", self.still_split.len(), limit.still_split as usize),
            ("real transitions lost", self.lost.len(), limit.lost as usize),
        ]
    }

    fn report(&self, bench: &Bench, episode: &str, limit: &Limit) -> String {
        let cuts = bench.cuts.iter().filter(|c| c.episode == episode);
        let windows: Vec<&Window> = bench.windows.iter().filter(|w| w.episode == episode).collect();
        let unsure = bench.spans.iter().filter(|s| s.episode == episode && s.verdict == Verdict::Unsure).count();
        let list = |frames: &[u32]| {
            if frames.is_empty() { "0".to_string() } else { format!("{} {frames:?}", frames.len()) }
        };
        let moved: Vec<String> = self.misplaced.iter().map(|(l, f)| format!("{l}→{f}")).collect();
        let mut out = format!(
            "bench {episode}: cuts checked by eye {}, dialogue windows {} ({} cuts), transitions judged {}\n",
            cuts.count(),
            windows.len(),
            windows.iter().map(|w| w.cuts).sum::<u32>(),
            self.one_shot + self.transitions + unsure,
        );
        out += &format!(
            "merged {} | split {} | known false cuts {} | misplaced {}\n",
            list(&self.merged),
            list(&self.split),
            list(&self.known_false),
            if moved.is_empty() { "0".to_string() } else { format!("{} [{}]", moved.len(), moved.join(", ")) },
        );
        out += &format!(
            "one-shot spans still split {} / {} | real transitions lost {} / {} | {unsure} unsure, not scored\n",
            self.still_split.len(),
            self.one_shot,
            self.lost.len(),
            self.transitions,
        );
        let failures = self.failures(limit);
        if !failures.is_empty() {
            return out + &format!("FAIL: {} (limits.tsv)\n", failures.join("; "));
        }
        let trade = self.trade(limit);
        let held: Vec<String> = trade.iter().map(|(what, n, max)| format!("{what} {n} (limit {max})")).collect();
        out += &format!("pass: no labelled cut merged, no dialogue window split, {}\n", held.join(", "));
        // A gain stays only once its limit comes down with it.
        let under: Vec<String> =
            trade.iter().filter(|(_, n, max)| n < max).map(|(what, n, max)| format!("{what} {n} < {max}")).collect();
        if !under.is_empty() {
            out += &format!("under limits.tsv: {}; lower it there, so the gain stays\n", under.join(", "));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &str) -> Vec<String> {
        a.split(' ').map(String::from).collect()
    }

    #[test]
    fn tsv_prints_a_range_or_writes_a_file() {
        assert_eq!(tsv(&args("x in.sig --anime")), None);
        assert_eq!(tsv(&args("x in.sig --tsv 3 40 --anime")), Some(Tsv::Raw(3, 40)));
        assert_eq!(tsv(&args("x in.sig --anime --tsv bb.segments.tsv")), Some(Tsv::Write("bb.segments.tsv")));
    }

    #[test]
    #[should_panic(expected = "neither a number nor a flag")]
    fn tsv_refuses_a_lone_number() {
        tsv(&args("x in.sig --tsv 3 --anime"));
    }

    #[test]
    #[should_panic(expected = "neither a number nor a flag")]
    fn tsv_refuses_a_flag() {
        tsv(&args("x in.sig --tsv --anime"));
    }

    #[test]
    #[should_panic(expected = "needs FROM TO or an output file")]
    fn tsv_needs_something_after_it() {
        tsv(&args("x in.sig --tsv"));
    }

    #[test]
    fn kinds_named_as_the_segments_file_has_them() {
        let kinds = [
            SegmentKind::Shot,
            SegmentKind::Transition(TransitionKind::Dissolve),
            SegmentKind::Transition(TransitionKind::FadeOut),
            SegmentKind::Transition(TransitionKind::FadeIn),
            SegmentKind::Black,
            SegmentKind::Flash,
        ];
        assert_eq!(kinds.map(|k| kind_name(&k)), ["shot", "dissolve", "fade_out", "fade_in", "black", "flash"]);
    }

    #[test]
    fn bench_takes_a_folder_and_an_episode() {
        assert_eq!(bench_args(&args("x in.sig --anime")), None);
        assert_eq!(bench_args(&args("x in.sig --bench bench/shots rz_31 --anime")), Some(("bench/shots", "rz_31")));
    }

    #[test]
    #[should_panic(expected = "needs the bench folder and the episode")]
    fn bench_refuses_a_missing_episode() {
        bench_args(&args("x in.sig --bench bench/shots --anime"));
    }

    const CUTS: &str = "episode\tframe\tverdict\tpicked\tnote\n";
    const WINDOWS: &str = "episode\tfrom\tto\tcuts\tnote\n";
    const SPANS: &str = "episode\tstart\tend\tverdict\tnote\n";
    const LIMITS: &str = "episode\tstill_split\tlost\tnote\n";

    fn limit(still_split: u32, lost: u32) -> Limit {
        Limit { episode: "ep".into(), still_split, lost }
    }

    #[test]
    fn bench_files_parse_and_skip_the_header() {
        let b = Bench::parse(
            &format!("{CUTS}ep\t100\tcut\tA\t\nep\t200\tno_cut\tBC\tlightning\n\n"),
            &format!("{WINDOWS}ep\t90\t300\t1\t\n"),
            &format!("{SPANS}ep\t400\t412\tone_shot\tsmoke\nep\t500\t520\ttransition\t\nep\t600\t603\tunsure\t\n"),
            &format!("{LIMITS}ep\t3\t2\tthe fusion of today\n"),
        )
        .unwrap();
        assert_eq!(
            b.cuts,
            [
                Cut { episode: "ep".into(), frame: 100, real: true },
                Cut { episode: "ep".into(), frame: 200, real: false },
            ]
        );
        assert_eq!(b.windows, [Window { episode: "ep".into(), from: 90, to: 300, cuts: 1 }]);
        assert_eq!(
            b.spans.iter().map(|s| (s.start, s.end, s.verdict)).collect::<Vec<_>>(),
            [(400, 412, Verdict::OneShot), (500, 520, Verdict::Transition), (600, 603, Verdict::Unsure)]
        );
        assert_eq!(b.limits, [limit(3, 2)]);
        assert_eq!(b.limit("ep"), Some(&limit(3, 2)));
        assert_eq!(b.limit("other"), None);
        assert_eq!(b.episodes(), ["ep"]);
        // Windows line endings, as a checkout may have them.
        let crlf =
            Bench::parse(&format!("{CUTS}ep\t100\tcut\tA\t\n").replace('\n', "\r\n"), WINDOWS, SPANS, LIMITS).unwrap();
        assert_eq!(crlf.cuts, [Cut { episode: "ep".into(), frame: 100, real: true }]);
    }

    #[test]
    fn a_bad_bench_file_names_its_line() {
        let parse = |cuts: &str, spans: &str| Bench::parse(cuts, WINDOWS, spans, LIMITS).unwrap_err();
        assert_eq!(
            parse(&format!("{CUTS}ep\t100\tmaybe\tA\t\n"), SPANS),
            "cuts.tsv:2: verdict \"maybe\" is neither cut nor no_cut"
        );
        assert_eq!(parse(&format!("{CUTS}ep\tx\tcut\tA\t\n"), SPANS), "cuts.tsv:2: \"x\" is not a frame number");
        assert_eq!(
            parse(&format!("{CUTS}ep\t100\tcut\tAW\t\n"), SPANS),
            "cuts.tsv:2: picked \"AW\" isn't made of A, B, C and R"
        );
        assert!(parse(&format!("{CUTS}ep\t100\tcut\t\t\n"), SPANS).starts_with("cuts.tsv:2: picked \"\""));
        assert_eq!(parse(&format!("{CUTS}ep\t100\tcut\n"), SPANS), "cuts.tsv:2: 3 columns, expected 5");
        assert!(parse(CUTS, &format!("{SPANS}ep\t1\t4\tfade\t\n")).starts_with("transitions.tsv:2: verdict \"fade\""));
        assert!(parse(WINDOWS, SPANS).starts_with("cuts.tsv: the header is"));
        let limits = |l: &str| Bench::parse(CUTS, WINDOWS, SPANS, l).unwrap_err();
        assert_eq!(limits(&format!("{LIMITS}ep\t3\t-1\t\n")), "limits.tsv:2: \"-1\" is not a count");
        assert!(limits(SPANS).starts_with("limits.tsv: the header is"));
    }

    fn seg(start: u32, end: u32, kind: SegmentKind) -> Segment {
        Segment { start, end, kind, tiny: false }
    }

    fn shots(cuts: &[u32], end: u32) -> Vec<Segment> {
        let mut starts = vec![0];
        starts.extend_from_slice(cuts);
        starts.iter().zip(cuts.iter().chain([&end])).map(|(&a, &b)| seg(a, b, SegmentKind::Shot)).collect()
    }

    const DISSOLVE: SegmentKind = SegmentKind::Transition(TransitionKind::Dissolve);

    #[test]
    fn a_lost_cut_is_merged_and_a_new_cut_in_a_window_is_split() {
        // Labelled: 100 (found), 200 (lost), 300 (found at 302) and the
        // known false cut 700; the window 50..400 holds the three cuts.
        let b = Bench::parse(
            &format!("{CUTS}ep\t100\tcut\tA\t\nep\t200\tcut\tC\t\nep\t300\tcut\tC\t\nep\t700\tno_cut\tB\t\n"),
            &format!("{WINDOWS}ep\t50\t400\t3\t\n"),
            SPANS,
            LIMITS,
        )
        .unwrap();
        // A dissolve inside the window, and a cut 2 frames before its end that
        // only moves the edge.
        let mut segs = shots(&[50, 101, 250, 302, 398, 600, 700], 1000);
        segs.splice(
            2..3,
            [seg(101, 150, SegmentKind::Shot), seg(150, 160, DISSOLVE), seg(160, 250, SegmentKind::Shot)],
        );
        let s = score(&b, "ep", &segs);
        assert_eq!(s.merged, [200]);
        assert_eq!(s.misplaced, [(300, 302)]);
        assert_eq!(s.known_false, [700]);
        // The new cut at 250 and the dissolve at 150 split the window; 600 is
        // outside every window, 50 and 398 are at its edges.
        assert_eq!(s.split, [150, 250]);
        // No limit lets a cut be merged or a window be split.
        assert!(!s.passes(&limit(99, 99)));
        assert!(
            s.report(&b, "ep", &limit(99, 99))
                .ends_with("FAIL: a labelled cut is merged; a dialogue window is split (limits.tsv)\n")
        );

        // The same labels on segments that hold every cut where it belongs.
        let good = score(&b, "ep", &shots(&[52, 100, 199, 301, 403, 600], 1000));
        assert_eq!(good, Score::default());
        assert!(good.passes(&limit(0, 0)));
        // Another episode's rows don't count.
        assert_eq!(score(&b, "other", &shots(&[], 1000)), Score::default());
    }

    #[test]
    fn spans_still_split_or_lost_are_held_to_their_limits() {
        let b = Bench::parse(
            CUTS,
            WINDOWS,
            &format!(
                "{SPANS}ep\t100\t110\tone_shot\t\nep\t200\t210\tone_shot\t\nep\t300\t310\tone_shot\t\n\
                 ep\t400\t412\ttransition\t\nep\t500\t512\ttransition\t\nep\t600\t612\ttransition\t\n\
                 ep\t700\t703\tunsure\t\n"
            ),
            LIMITS,
        )
        .unwrap();
        // One-shot spans: 100..110 is still a dissolve and 300..310 has a hard
        // cut inside (both still split); 200..210 is shot frames now.
        // Transitions: 400..412 is a flash now and 500..512 a hard cut 3
        // frames after its end (both kept); 600..612 has no boundary near it
        // (lost).
        let segs = vec![
            seg(0, 100, SegmentKind::Shot),
            seg(100, 110, DISSOLVE),
            seg(110, 305, SegmentKind::Shot),
            seg(305, 400, SegmentKind::Shot),
            seg(400, 403, SegmentKind::Flash),
            seg(403, 515, SegmentKind::Shot),
            seg(515, 1000, SegmentKind::Shot),
        ];
        let s = score(&b, "ep", &segs);
        assert_eq!((s.still_split.as_slice(), s.one_shot), ([100, 300].as_slice(), 3));
        assert_eq!((s.lost.as_slice(), s.transitions), ([600].as_slice(), 3));
        assert!(
            s.report(&b, "ep", &limit(2, 1))
                .contains("one-shot spans still split 2 / 3 | real transitions lost 1 / 3 | 1 unsure, not scored\n")
        );
        // At its limits it passes, and says what it trades.
        assert!(s.passes(&limit(2, 1)));
        assert!(s.report(&b, "ep", &limit(2, 1)).ends_with(
            "pass: no labelled cut merged, no dialogue window split, \
             one-shot spans still split 2 (limit 2), real transitions lost 1 (limit 1)\n"
        ));
        // One more span split, or one more transition lost, than the limit fails.
        assert!(!s.passes(&limit(1, 1)));
        assert!(
            s.report(&b, "ep", &limit(1, 1))
                .ends_with("FAIL: one-shot spans still split 2, over its limit of 1 (limits.tsv)\n")
        );
        assert!(!s.passes(&limit(2, 0)));
        assert!(s.report(&b, "ep", &limit(1, 0)).ends_with(
            "FAIL: one-shot spans still split 2, over its limit of 1; \
                 real transitions lost 1, over its limit of 0 (limits.tsv)\n"
        ));
        // Below a limit it passes and asks for the limit to come down.
        assert!(s.passes(&limit(5, 1)));
        assert!(s.report(&b, "ep", &limit(5, 1)).ends_with(
            "one-shot spans still split 2 (limit 5), real transitions lost 1 (limit 1)\n\
             under limits.tsv: one-shot spans still split 2 < 5; lower it there, so the gain stays\n"
        ));
    }

    /// Rows come in blocks per episode, and in each block their frames rise.
    fn sorted_and_unique<'a>(file: &str, rows: impl Iterator<Item = (&'a str, u32)>) {
        let mut done: Vec<&str> = Vec::new();
        let mut last: Option<(&str, u32)> = None;
        for (episode, frame) in rows {
            match last {
                Some((e, f)) if e == episode => assert!(frame > f, "{file}: {episode} {frame} after {f}"),
                Some((e, _)) => {
                    done.push(e);
                    assert!(!done.contains(&episode), "{file}: {episode} comes back after {e}");
                }
                None => {}
            }
            last = Some((episode, frame));
        }
    }

    #[test]
    fn the_repository_bench_is_consistent() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/shots");
        let b = Bench::load(&dir).unwrap();
        sorted_and_unique("cuts.tsv", b.cuts.iter().map(|c| (c.episode.as_str(), c.frame)));
        sorted_and_unique("windows.tsv", b.windows.iter().map(|w| (w.episode.as_str(), w.from)));
        sorted_and_unique("transitions.tsv", b.spans.iter().map(|s| (s.episode.as_str(), s.start)));
        for s in &b.spans {
            assert!(s.start < s.end, "transitions.tsv: {} {}..{}", s.episode, s.start, s.end);
        }
        for (i, w) in b.windows.iter().enumerate() {
            assert!(w.from < w.to, "windows.tsv: {} {}..{}", w.episode, w.from, w.to);
            if let Some(next) = b.windows.get(i + 1).filter(|n| n.episode == w.episode) {
                assert!(w.to <= next.from, "windows.tsv: {} {}..{} overlaps {}", w.episode, w.from, w.to, next.from);
            }
            let inside: Vec<&Cut> =
                b.cuts.iter().filter(|c| c.episode == w.episode && w.from < c.frame && c.frame < w.to).collect();
            assert!(
                inside.iter().all(|c| c.real),
                "windows.tsv: {} {}..{} holds a no_cut row",
                w.episode,
                w.from,
                w.to
            );
            assert_eq!(inside.len() as u32, w.cuts, "windows.tsv: {} {}..{}", w.episode, w.from, w.to);
        }
        // One run scores all three files: every episode has checked cuts.
        let with_cuts: Vec<&str> = b.cuts.iter().map(|c| c.episode.as_str()).collect();
        for e in b.episodes() {
            assert!(with_cuts.contains(&e), "{e} has windows or transitions but no row in cuts.tsv");
        }
        // Each episode has one row of limits, and none is beyond its spans.
        for e in b.episodes() {
            let rows: Vec<&Limit> = b.limits.iter().filter(|l| l.episode == e).collect();
            assert_eq!(rows.len(), 1, "limits.tsv: {e} needs exactly one row");
            let spans = |v: Verdict| b.spans.iter().filter(|s| s.episode == e && s.verdict == v).count() as u32;
            assert!(rows[0].still_split <= spans(Verdict::OneShot), "limits.tsv: {e} still_split");
            assert!(rows[0].lost <= spans(Verdict::Transition), "limits.tsv: {e} lost");
        }
        for l in &b.limits {
            assert!(with_cuts.contains(&l.episode.as_str()), "limits.tsv: {} is no episode of the bench", l.episode);
        }
    }
}
