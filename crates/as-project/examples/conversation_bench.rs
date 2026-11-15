//! Dev tool: conversation packs from who is in which shot, measured against
//! who speaks in each scene (`bench/scenes/README.md`, "Conversations from
//! who is in which shot"; `bench/models/DECISIONS.md`, row 25). Nothing of
//! the pack code is used or changed: the sets of shots are built here, from
//! a COPY of a project (`project.db` and its `-wal` in a folder of their
//! own), which is opened read only.
//!
//! `conversation_bench <project_copy_dir> <scenes.tsv> <identity labels.tsv> <speakers.tsv> <pictures.emb> [out.tsv]`
//! `conversation_bench shots <project_copy_dir> <scenes.tsv> <shot labels.tsv> <cast.tsv> <pictures.emb> [out.tsv]`
//! `conversation_bench agree <project_copy_dir> <scenes.tsv> <identity labels.tsv> <speakers.tsv> <shot labels.tsv>`
//!
//! `<scenes.tsv>` holds the labelled scenes with the seconds of speech per
//! speaker (`bench/scenes`), `<identity labels.tsv>` who is to be seen where
//! (`bench/identity`), `<speakers.tsv>` which face label each speaker is,
//! `<pictures.emb>` the app's pictures of the shots (`scenes_run`, as
//! `shot_embed.py` writes them): the scenes the app would find are worked
//! out from them with the live-action recipe. `[out.tsv]` gets every number
//! per reading, character and set.
//!
//! For each main character (named in the identity labels, at least 30 s on
//! screen) four sets of shots (row 25) and a variant of the third (row 27):
//! - `own`: the shots the character is seen in;
//! - `groups`: those, and the shots between two of them that one of the
//!   app's shot groups holds (`as_project::pack`, `GROUP_GAP`);
//! - `walk`: version 0.1's conversation (`src/autoscene/pack_context.py`):
//!   from each own shot out both ways while the next shot is in the same
//!   scene and holds the character or someone they share frames with;
//! - `bridged`: the walk, which also steps over a stretch of up to
//!   `GROUP_GAP` that holds neither; `br-kept` is the same without the
//!   shots it steps over;
//! - `scenes`: every shot of the scenes with an own shot ("Whole scenes");
//!   `sc-any` those of them in which the cast shows anyone, `sc-part`
//!   those that hold the character or a partner (row 28).
//!
//! Three readings: the stored cast with the app's scenes (what a port would
//! give), the stored cast with the labelled scenes, and the labels for both.
//! Who is truly in a shot, and so every score, comes from the labels.
//!
//! `shots` is the same run on a title without hand labels (row 29): two
//! readers' shot labels (`bench/scenes`, `*.shots.tsv`) say who is seen and
//! who speaks, `<cast.tsv>` which stored characters each of their names is;
//! the reading with labels for both is left out, shot labels carry no times.
//! `agree` checks such shot labels against the hand labels and the speakers'
//! seconds of an episode that has both.
//!
//! `MEDIA=<n>` picks the n-th media of a project with several (from 0),
//! `SURE=1` leaves out the speakers whose face label is unsure,
//! `DETAIL=<label name>` lists every shot of that character's scenes,
//! `FOUND=<file>` writes every shot with its labelled scene and the scene
//! the app's recipe finds (for a check of the scores by other means).
//! Its unit tests run with `cargo test -p as-project --example conversation_bench`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

use as_analysis::pictures;
use as_analysis::scenes::{self, Between, Piece, Recipe, Shot, scene_ids};
use as_analysis::tracks::{FaceDet, dets_from_bytes};
use rusqlite::{Connection, OpenFlags};

/// A main character has at least this long on screen (as in `bench/identity`).
const MAIN_SEC: f64 = 30.0;
/// Two people share frames when their faces are on screen together for this
/// long at least (0.1's `cocast_min_overlap_sec`).
const TOGETHER_SEC: f64 = 0.2;
/// `as_project::pack::GROUP_GAP`: the seconds two shots of a character may
/// lie apart and still share a group.
const GROUP_GAP: i64 = 6;
/// 0.1's `DEFAULT_MAX_CLIP_SEC`: the longest clip its walk made.
const CAP_SEC: f64 = 180.0;

const SETS: [&str; 8] = ["own", "groups", "walk", "bridged", "br-kept", "sc-any", "sc-part", "scenes"];
const GROUPS: usize = 1;
const WALK: usize = 2;
const BRIDGED: usize = 3;
const KEPT: usize = 4;
const ANY: usize = 5;
const PARTNERS: usize = 6;
const SCENES: usize = 7;

// -- what a project stores ---------------------------------------------------

struct Track {
    start: u32,
    end: u32,
    /// The first frame of its shot.
    shot: u32,
    /// Index into [`Episode::characters`], when the cast shows the track there.
    character: Option<usize>,
    dets: Vec<FaceDet>,
}

struct Episode {
    /// Frames per second, as the media states them.
    rate: (i64, i64),
    /// `(start, end, kind)`, in order.
    segments: Vec<(u32, u32, String)>,
    tracks: Vec<Track>,
    /// What the cast calls each character, and whether it hides them.
    characters: Vec<(String, bool)>,
    /// Each character's number in the project (`Character <number>`).
    numbers: Vec<u32>,
}

impl Episode {
    fn fps(&self) -> f64 {
        self.rate.0 as f64 / self.rate.1 as f64
    }
}

fn load(dir: &Path) -> Episode {
    let path = dir.join("project.db");
    assert!(path.is_file(), "{}: no project.db (a COPY of a project's folder)", dir.display());
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let media: Vec<(String, i64, i64)> = db
        .prepare("SELECT id, fps_num, fps_den FROM media WHERE shots = 'ready' ORDER BY position, id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let nth = std::env::var("MEDIA").ok().map(|v| v.parse::<usize>().expect("MEDIA=<n>"));
    assert!(nth.is_some() || media.len() == 1, "{} media with shots: pick one with MEDIA=<n>", media.len());
    let (id, num, den) = media.get(nth.unwrap_or(0)).expect("MEDIA: no such media").clone();
    assert!(num > 0 && den > 0, "the media has no frame rate");

    let segments = db
        .prepare("SELECT start, end, kind FROM segments WHERE media_id = ?1 ORDER BY start")
        .unwrap()
        .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    let mut index = HashMap::new();
    let mut characters = Vec::new();
    let mut numbers = Vec::new();
    let mut st = db.prepare("SELECT id, number, name, hidden FROM characters ORDER BY number").unwrap();
    let mut rows = st.query([]).unwrap();
    while let Some(r) = rows.next().unwrap() {
        let (cid, number, name, hidden): (String, u32, Option<String>, bool) =
            (r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap(), r.get(3).unwrap());
        index.insert(cid, characters.len());
        characters.push((name.unwrap_or_else(|| format!("Character {number}")), hidden));
        numbers.push(number);
    }
    let mut tracks = Vec::new();
    let mut st = db
        .prepare(
            "SELECT start, end, shot, character_id, assign, dets FROM tracks
              WHERE media_id = ?1 AND not_face = 0 ORDER BY start, id",
        )
        .unwrap();
    let mut rows = st.query([&id]).unwrap();
    while let Some(r) = rows.next().unwrap() {
        let (start, end, shot, cid, assign, dets): (u32, u32, u32, Option<String>, String, Vec<u8>) = (
            r.get(0).unwrap(),
            r.get(1).unwrap(),
            r.get(2).unwrap(),
            r.get(3).unwrap(),
            r.get(4).unwrap(),
            r.get(5).unwrap(),
        );
        // The cast shows a face with its character when it is sure of it
        // (`as_db`'s `shot_cast` and `pack_rows`).
        let shown = matches!(assign.as_str(), "auto" | "pinned");
        tracks.push(Track {
            start,
            end,
            shot,
            character: cid.filter(|_| shown).and_then(|c| index.get(&c).copied()),
            dets: dets_from_bytes(&dets).expect("stored detections"),
        });
    }
    Episode { rate: (num, den), segments, tracks, characters, numbers }
}

/// The project's shots and what lies between them, each with the characters
/// the cast shows in it (hidden ones too, as the app's scenes read them).
fn shots_of(ep: &Episode) -> (Vec<Shot>, Vec<Between>) {
    let (mut shots, between) = scenes::shots(ep.segments.iter().map(|(start, end, kind)| {
        let piece = match kind.as_str() {
            "shot" => Piece::Shot,
            "dissolve" => Piece::Dissolve,
            "black" | "fade_in" | "fade_out" => Piece::Dark,
            _ => Piece::Other,
        };
        (*start, *end, piece)
    }));
    for t in &ep.tracks {
        if let (Some(c), Ok(s)) = (t.character, shots.binary_search_by_key(&t.shot, |s| s.start))
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

/// Where the media goes to or comes from black, in frame order
/// (`as_db::ProjectDb::fades`).
fn fades_of(ep: &Episode) -> Vec<(u32, u32)> {
    ep.segments
        .iter()
        .filter(|s| matches!(s.2.as_str(), "black" | "fade_in" | "fade_out"))
        .map(|s| (s.0, s.1))
        .collect()
}

/// Reads a file of `shot_embed.py` or `scenes_run`: per shot, the unit
/// vectors of its frames, as the app would read them back from its database.
fn pictures_of(path: &Path, shots: &[Shot]) -> Vec<Vec<Vec<f32>>> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(b.len() >= 12 && &b[..4] == b"ASEM", "{}: not an embedding file", path.display());
    let u32_at = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let (rows, dim) = (u32_at(4) as usize, u32_at(8) as usize);
    assert_eq!(b.len(), 12 + rows * (4 + dim * 4), "{}: cut short", path.display());
    let mut out = vec![Vec::new(); shots.len()];
    for r in 0..rows {
        let at = 12 + r * (4 + dim * 4);
        if let Some(s) = scenes::shot_at(shots, u32_at(at)) {
            let v = b[at + 4..at + 4 + dim * 4].as_chunks::<4>().0.iter().map(|x| f32::from_le_bytes(*x)).collect();
            out[s].push(v);
        }
    }
    assert!(out.iter().all(|v| !v.is_empty()), "{}: a shot has no vector: not this media's file", path.display());
    // The database keeps half floats.
    out.iter().map(|v| pictures::from_bytes(&pictures::to_bytes(v)).expect("the pictures' size")).collect()
}

// -- labels ------------------------------------------------------------------

/// The labelled scenes (`bench/scenes/*.scenes.tsv`): a cut per gap, and per
/// scene who speaks for how many seconds.
fn scene_labels(path: &Path, shots: &[Shot]) -> (Vec<bool>, Vec<Vec<(String, f64)>>) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let header: Vec<&str> = text.lines().next().expect("scene labels").split('\t').collect();
    // Scenes with the speakers' seconds (Serial Speakers), or scenes alone (`frame sure note`).
    let with_speakers = header == ["frame", "source_s", "speakers"];
    assert!(with_speakers || header == ["frame", "sure", "note"], "{}: scene labels", path.display());
    let mut cuts = vec![false; shots.len() - 1];
    let mut rows = Vec::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let frame: u32 = f[0].trim().parse().expect("scene labels: a frame first");
        let s = (0..shots.len()).min_by_key(|&i| shots[i].start.abs_diff(frame)).expect("shots");
        assert!(shots[s].start.abs_diff(frame) <= 12, "scene at frame {frame}: no shot starts there");
        if s > 0 {
            assert!(!cuts[s - 1], "two scenes start at frame {frame}");
            cuts[s - 1] = true;
        }
        let said = if with_speakers { f.get(2).copied().unwrap_or_default() } else { "" };
        let said: Vec<(String, f64)> = said
            .split(';')
            .filter(|x| !x.trim().is_empty())
            .map(|x| {
                let (who, secs) = x.rsplit_once(':').expect("name:seconds");
                (who.to_owned(), secs.parse().expect("seconds"))
            })
            .collect();
        rows.push((s, said));
    }
    let ids = scene_ids(&cuts);
    let mut speech = vec![Vec::new(); ids[shots.len() - 1] + 1];
    assert_eq!(speech.len(), rows.len(), "the scenes do not start at shots of their own");
    for (s, said) in rows {
        speech[ids[s]] = said;
    }
    (cuts, speech)
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

/// Someone the labels name (`?` is unnamed, `x` nobody could tell).
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
/// frames away) overlaps it most, by 0.3 at least. (As `scenes_bench`.)
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

/// Which face labels each speaker is (`speaker label sure note`; `-` for a
/// voice without a face). `sure_only` leaves the unsure rows out.
fn speaker_labels(path: &Path, sure_only: bool) -> HashMap<String, Vec<String>> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let faces = out.entry(f[0].to_owned()).or_default();
        if f[1] != "-" && !(sure_only && f[2] == "no") {
            faces.push(f[1].to_owned());
        }
    }
    out
}

// -- labels two readers made ---------------------------------------------------

/// The readers' label for someone who is not on their cast sheet: never
/// credited, as `?` in the hand labels.
const OTHER: &str = "other";

/// What two readers wrote for one shot: the seconds of speech heard in it,
/// whose face each of them sees, and who of those each takes to speak.
#[derive(Clone, Debug, Default, PartialEq)]
struct Read {
    speech: f64,
    faces: [Vec<String>; 2],
    speaks: [Vec<String>; 2],
}

/// A file of two readers' shot labels (`frame speech_s faces_a faces_b
/// speaks_a speaks_b`; names with commas between them, `-` for nobody): each
/// shot's row, `None` for a shot that was not read.
fn shot_labels(path: &Path, shots: &[Shot]) -> Vec<Option<Read>> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let header: Vec<&str> = text.lines().next().expect("shot labels").split('\t').collect();
    assert_eq!(header, ["frame", "speech_s", "faces_a", "faces_b", "speaks_a", "speaks_b"], "{}", path.display());
    let list = |x: &str| -> Vec<String> {
        let mut v: Vec<String> =
            x.split(',').map(str::trim).filter(|n| !n.is_empty() && *n != "-").map(str::to_owned).collect();
        v.sort();
        v.dedup();
        v
    };
    let mut out = vec![None; shots.len()];
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 6, "shot labels: six columns, not `{line}`");
        let frame: u32 = f[0].parse().expect("shot labels: a frame first");
        let s = shots
            .binary_search_by_key(&frame, |s| s.start)
            .unwrap_or_else(|_| panic!("shot labels: no shot starts at frame {frame}"));
        assert!(out[s].is_none(), "shot labels: two rows for the shot at frame {frame}");
        let read = Read {
            speech: f[1].parse().expect("seconds of speech"),
            faces: [list(f[2]), list(f[3])],
            speaks: [list(f[4]), list(f[5])],
        };
        for r in 0..2 {
            assert!(
                read.speaks[r].iter().all(|p| read.faces[r].contains(p)),
                "frame {frame}: a reader's speaker is not among the faces that reader sees"
            );
        }
        out[s] = Some(read);
    }
    out
}

/// The readers' labels as what is true (row 29). A shot shows the names both
/// readers give. A person speaks in a scene when each reader marks them
/// speaking in a shot of it (not needfully the same shot). Their seconds are
/// the heard speech of the shots in which both readers mark them, split
/// evenly where several are marked; the rest of the scene's heard speech
/// belongs to nobody known. A shot that was not read shows nobody.
/// `number` gives each cast sheet label its number.
fn readers_truth(read: &[Option<Read>], scene: Vec<usize>, number: &HashMap<&str, usize>, secs: Vec<f64>) -> Truth {
    let known = |name: &String| -> Option<usize> {
        (name != OTHER)
            .then(|| *number.get(name.as_str()).unwrap_or_else(|| panic!("{name}: not a label of the cast sheet")))
    };
    let both =
        |a: &[String], b: &[String]| -> Vec<usize> { a.iter().filter(|p| b.contains(p)).filter_map(known).collect() };
    let n = read.len();
    let scenes = scene.last().map_or(0, |&s| s + 1);
    let mut faces = vec![Vec::new(); n];
    let mut any_face = vec![false; n];
    // Per scene: who each reader marks speaking, the seconds of those both
    // mark in one shot, and the heard speech of nobody known.
    let mut marked: Vec<[BTreeSet<usize>; 2]> = vec![Default::default(); scenes];
    let mut theirs: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); scenes];
    let mut unknown = vec![0.0; scenes];
    for s in 0..n {
        let Some(r) = &read[s] else { continue };
        faces[s] = both(&r.faces[0], &r.faces[1]);
        any_face[s] = !r.faces[0].is_empty() && !r.faces[1].is_empty();
        for k in 0..2 {
            marked[scene[s]][k].extend(r.speaks[k].iter().filter_map(known));
        }
        let speaking = both(&r.speaks[0], &r.speaks[1]);
        if speaking.is_empty() {
            unknown[scene[s]] += r.speech;
        }
        for &p in &speaking {
            *theirs[scene[s]].entry(p).or_default() += r.speech / speaking.len() as f64;
        }
    }
    let speech = (0..scenes)
        .map(|c| {
            let mut rows: Vec<(Vec<usize>, f64)> = marked[c][0]
                .intersection(&marked[c][1])
                .map(|&p| (vec![p], theirs[c].get(&p).copied().unwrap_or(0.0)))
                .collect();
            if unknown[c] > 0.0 {
                rows.push((Vec::new(), unknown[c]));
            }
            rows
        })
        .collect();
    Truth { scene, faces, any_face, speech, secs }
}

/// The readers' cast sheet (`label keys note`): each label with the stored
/// characters that are that person (`c<number>`, commas between them).
fn cast_sheet(path: &Path, ep: &Episode) -> Vec<(String, Vec<usize>)> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let header: Vec<&str> = text.lines().next().expect("cast sheet").split('\t').collect();
    assert_eq!(header, ["label", "keys", "note"], "{}", path.display());
    let mut taken = HashSet::new();
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let characters = f[1]
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(|k| {
                    let number: u32 = k
                        .strip_prefix('c')
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| panic!("cast sheet: {k} is not c<number>"));
                    let c = ep
                        .numbers
                        .iter()
                        .position(|&x| x == number)
                        .unwrap_or_else(|| panic!("cast sheet: no character {number} in the project"));
                    assert!(taken.insert(c), "cast sheet: {k} stands under two labels");
                    c
                })
                .collect();
            (f[0].to_owned(), characters)
        })
        .collect()
}

// -- who is where ------------------------------------------------------------

/// Who is where, by one source: the stored cast, or the labels.
struct Who {
    /// Per shot, the people seen in it.
    shots: Vec<Vec<usize>>,
    /// Per person, when a face of theirs is on screen: `[start, end)`.
    spans: Vec<Vec<(u32, u32)>>,
}

impl Who {
    /// The people `me` shares frames with: a face of each on screen at the
    /// same time for `frames` at least, once, anywhere.
    fn partners(&self, me: usize, frames: u32) -> HashSet<usize> {
        (0..self.spans.len())
            .filter(|&p| p != me)
            .filter(|&p| {
                self.spans[me]
                    .iter()
                    .any(|&(a, b)| self.spans[p].iter().any(|&(c, d)| b.min(d).saturating_sub(a.max(c)) >= frames))
            })
            .collect()
    }

    fn own(&self, me: usize) -> Vec<bool> {
        self.shots.iter().map(|s| s.contains(&me)).collect()
    }
}

/// The stored cast as a pack sees it: the faces the cast is sure of, the
/// hidden characters left out.
fn stored(ep: &Episode, shots: &[Shot]) -> Who {
    let mut who = Who { shots: vec![Vec::new(); shots.len()], spans: vec![Vec::new(); ep.characters.len()] };
    for t in &ep.tracks {
        let Some(c) = t.character.filter(|&c| !ep.characters[c].1) else { continue };
        who.spans[c].push((t.start, t.end));
        if let Ok(s) = shots.binary_search_by_key(&t.shot, |s| s.start)
            && !who.shots[s].contains(&c)
        {
            who.shots[s].push(c);
        }
    }
    who
}

/// The shots a label lies in.
fn shots_under(shots: &[Shot], l: &Label) -> impl Iterator<Item = usize> {
    let end = l.end.max(l.start + 1);
    (0..shots.len()).filter(move |&s| shots[s].start < end && l.start < shots[s].end)
}

// -- the sets ------------------------------------------------------------------

/// A fade or black in the gap `[end, start)` between two shots, or touching
/// it (`as_project::pack::fade_between`).
fn fade_between(fades: &[(u32, u32)], end: u32, start: u32) -> bool {
    let first = fades.partition_point(|f| f.1 < end);
    fades.get(first).is_some_and(|f| f.0 <= start)
}

/// The own shots and what lies between two of them that share one of the
/// app's shot groups (`as_project::pack::group_lengths`, which is private:
/// the same rule is written out here).
fn groups(own: &[bool], shots: &[Shot], fades: &[(u32, u32)], rate: (i64, i64)) -> Vec<bool> {
    let gap = (GROUP_GAP * rate.0 + rate.1 - 1) / rate.1;
    let mine: Vec<usize> = (0..own.len()).filter(|&s| own[s]).collect();
    let mut out = own.to_vec();
    for pair in mine.windows(2) {
        let (end, start) = (shots[pair[0]].end, shots[pair[1]].start);
        if i64::from(start.saturating_sub(end)) <= gap && !fade_between(fades, end, start) {
            out[pair[0]..pair[1]].fill(true);
        }
    }
    out
}

/// 0.1's conversation (`_grow_conversation`): from each own shot out both
/// ways while the next shot is in the same scene and `keeps` (it holds the
/// character or a partner). With `cap`, each own shot's stretch is cut to
/// that many seconds as 0.1 cut it, from the end farther from the own shot.
fn walk(own: &[bool], keeps: &[bool], scene: &[usize], cap: Option<(&[Shot], f64, f64)>) -> Vec<bool> {
    let n = own.len();
    let mut out = vec![false; n];
    for seed in (0..n).filter(|&s| own[s]) {
        let (mut lo, mut hi) = (seed, seed);
        while lo > 0 && scene[lo - 1] == scene[seed] && keeps[lo - 1] {
            lo -= 1;
        }
        while hi + 1 < n && scene[hi + 1] == scene[seed] && keeps[hi + 1] {
            hi += 1;
        }
        if let Some((shots, fps, secs)) = cap {
            while hi > lo && f64::from(shots[hi].end - shots[lo].start) / fps > secs {
                if seed - lo >= hi - seed {
                    lo += 1;
                } else {
                    hi -= 1;
                }
            }
        }
        out[lo..=hi].fill(true);
    }
    out
}

/// The walk with the shot groups' bridge (row 27): as [`walk`], and it steps
/// over shots that `keeps` not when the next shot that does is in the same
/// scene and starts at most [`GROUP_GAP`] after the last kept shot ends, with
/// no fade or black between. The shots stepped over are in the stretch.
fn bridged_walk(
    own: &[bool],
    keeps: &[bool],
    scene: &[usize],
    shots: &[Shot],
    fades: &[(u32, u32)],
    rate: (i64, i64),
) -> Vec<bool> {
    let n = own.len();
    let gap = (GROUP_GAP * rate.0 + rate.1 - 1) / rate.1;
    // From the kept shot `from`, the next kept shot of the scene on `side`, when the walk reaches it.
    let step = |from: usize, forward: bool| -> Option<usize> {
        let mut at = from;
        loop {
            at = if forward { at + 1 } else { at.checked_sub(1)? };
            if at >= n || scene[at] != scene[from] {
                return None;
            }
            if keeps[at] {
                break;
            }
        }
        let (end, start) =
            if forward { (shots[from].end, shots[at].start) } else { (shots[at].end, shots[from].start) };
        let near =
            at.abs_diff(from) == 1 || (i64::from(start.saturating_sub(end)) <= gap && !fade_between(fades, end, start));
        near.then_some(at)
    };
    let mut out = vec![false; n];
    for seed in (0..n).filter(|&s| own[s]) {
        let (mut lo, mut hi) = (seed, seed);
        while let Some(next) = step(lo, false) {
            lo = next;
        }
        while let Some(next) = step(hi, true) {
            hi = next;
        }
        out[lo..=hi].fill(true);
    }
    out
}

/// Every shot of the scenes with an own shot.
fn whole_scenes(own: &[bool], scene: &[usize]) -> Vec<bool> {
    let with: HashSet<usize> = (0..own.len()).filter(|&s| own[s]).map(|s| scene[s]).collect();
    scene.iter().map(|s| with.contains(s)).collect()
}

/// The sets of `me`, in the order of [`SETS`].
fn sets_of(
    who: &Who,
    me: usize,
    scene: &[usize],
    shots: &[Shot],
    fades: &[(u32, u32)],
    rate: (i64, i64),
    capped: bool,
) -> [Vec<bool>; 8] {
    let fps = rate.0 as f64 / rate.1 as f64;
    let own = who.own(me);
    let mut keep = who.partners(me, (TOGETHER_SEC * fps).ceil() as u32);
    keep.insert(me);
    let keeps: Vec<bool> = who.shots.iter().map(|s| s.iter().any(|p| keep.contains(p))).collect();
    let cap = capped.then_some((shots, fps, CAP_SEC));
    let bridged = bridged_walk(&own, &keeps, scene, shots, fades, rate);
    let kept = (0..own.len()).map(|s| bridged[s] && keeps[s]).collect();
    let whole = whole_scenes(&own, scene);
    // Row 28: the scenes' shots in which the cast shows anyone, and those that hold the
    // character or a partner.
    let anyone = (0..own.len()).map(|s| whole[s] && !who.shots[s].is_empty()).collect();
    let partners = (0..own.len()).map(|s| whole[s] && keeps[s]).collect();
    [
        own.clone(),
        groups(&own, shots, fades, rate),
        walk(&own, &keeps, scene, cap),
        bridged,
        kept,
        anyone,
        partners,
        whole,
    ]
}

// -- scores ------------------------------------------------------------------

/// What the labels say: the scene of each shot, who is truly in it, who
/// speaks in each scene.
struct Truth {
    scene: Vec<usize>,
    /// Per shot, the named people whose face is labelled in it.
    faces: Vec<Vec<usize>>,
    /// Per shot, whether any face is labelled in it, named or not.
    any_face: Vec<bool>,
    /// Per scene, each speaker: the face labels they are, and their seconds.
    speech: Vec<Vec<(Vec<usize>, f64)>>,
    /// Each shot's length in seconds.
    secs: Vec<f64>,
}

impl Truth {
    /// Whether the shot shows someone who speaks in its scene.
    fn shows_speaker(&self, s: usize) -> bool {
        self.speech[self.scene[s]].iter().any(|(who, _)| who.iter().any(|w| self.faces[s].contains(w)))
    }
}

/// One set of one character, counted against the labels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Tally {
    shots: u32,
    secs: f64,
    /// The shots the set adds to the own ones.
    added: u32,
    added_secs: f64,
    /// Added shots that show a speaker of their scene.
    speaker: u32,
    speaker_secs: f64,
    /// Added shots in a labelled scene without an own shot (a set that runs
    /// into a scene the character is not in), and the added shots that lie
    /// in a scene of the character's and show a speaker of it. Added after
    /// the review: `speaker` also counts a speaker of someone else's scene.
    outside: u32,
    speaker_theirs: u32,
    /// Added shots of scenes in which someone with a face label speaks.
    added_spoken: u32,
    /// Added shots with faces, none of them a speaker's; and with no face.
    quiet: u32,
    empty: u32,
    /// Added shots that truly show the character (the cast missed them).
    own_face: u32,
    /// Seconds of speech of the others in the character's scenes; of those,
    /// by speakers seen in the added shots, and seen in the set at all.
    others: f64,
    heard_added: f64,
    heard_set: f64,
    /// Shots of the character's scenes that show a speaker and not the
    /// character (by the cast, nor by the labels); and those of them the
    /// set holds.
    wanted: u32,
    held: u32,
}

/// Counts `set` of the character `me` (a label's number), whose own shots
/// are `own` (by the cast the set was made with).
fn tally(set: &[bool], own: &[bool], me: usize, t: &Truth) -> Tally {
    let n = set.len();
    let mut x = Tally::default();
    // In order, so that the seconds below are added up the same way in every run.
    let mine: BTreeSet<usize> = (0..n).filter(|&s| own[s]).map(|s| t.scene[s]).collect();
    for s in 0..n {
        let speaker = t.shows_speaker(s);
        if set[s] {
            x.shots += 1;
            x.secs += t.secs[s];
        }
        if set[s] && !own[s] {
            x.added += 1;
            x.added_secs += t.secs[s];
            x.added_spoken += u32::from(t.speech[t.scene[s]].iter().any(|(who, _)| !who.is_empty()));
            x.own_face += u32::from(t.faces[s].contains(&me));
            x.outside += u32::from(!mine.contains(&t.scene[s]));
            if speaker {
                x.speaker += 1;
                x.speaker_secs += t.secs[s];
                x.speaker_theirs += u32::from(mine.contains(&t.scene[s]));
            } else if t.any_face[s] {
                x.quiet += 1;
            } else {
                x.empty += 1;
            }
        }
        if mine.contains(&t.scene[s]) && !own[s] && !t.faces[s].contains(&me) && speaker {
            x.wanted += 1;
            x.held += u32::from(set[s]);
        }
    }
    let seen = |scene: usize, added: bool| -> HashSet<usize> {
        (0..n)
            .filter(|&s| t.scene[s] == scene && set[s] && !(added && own[s]))
            .flat_map(|s| t.faces[s].iter().copied())
            .collect()
    };
    for &scene in &mine {
        let (in_added, in_set) = (seen(scene, true), seen(scene, false));
        for (who, secs) in t.speech[scene].iter().filter(|(who, _)| !who.contains(&me)) {
            x.others += secs;
            if who.iter().any(|w| in_added.contains(w)) {
                x.heard_added += secs;
            }
            if who.iter().any(|w| in_set.contains(w)) {
                x.heard_set += secs;
            }
        }
    }
    x
}

fn share(a: f64, b: f64) -> Option<f64> {
    (b > 0.0).then(|| a / b)
}

fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

/// The 95 % interval of the mean of `v` when its members are drawn anew
/// with replacement (10,000 draws, always the same ones).
fn interval(v: &[f64]) -> Option<(f64, f64)> {
    if v.len() < 2 {
        return None;
    }
    let mut state = 25u64;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut means: Vec<f64> = (0..10_000)
        .map(|_| (0..v.len()).map(|_| v[(next() % v.len() as u64) as usize]).sum::<f64>() / v.len() as f64)
        .collect();
    means.sort_by(f64::total_cmp);
    Some((means[250], means[9749]))
}

fn show(v: Option<f64>) -> String {
    v.map_or_else(|| "    –".to_owned(), |v| format!("{v:.3}"))
}

// -- the run -------------------------------------------------------------------

/// A main character: the label's name and number, and the stored character
/// most of their faces are on.
struct Main {
    name: String,
    label: usize,
    stored: Option<usize>,
}

/// One character's sets in one reading.
struct Line {
    name: String,
    /// The character's number among the labels' names.
    label: usize,
    sets: [Vec<bool>; 8],
    tallies: [Tally; 8],
}

struct Bench<'a> {
    ep: &'a Episode,
    shots: &'a [Shot],
    fades: Vec<(u32, u32)>,
    truth: &'a Truth,
    main: &'a [Main],
}

impl Bench<'_> {
    /// Every main character's sets with `who` as the cast and `scene` as the
    /// scenes. `by_label`: `who` numbers people as the labels do.
    fn reading(&self, who: &Who, scene: &[usize], by_label: bool, capped: bool) -> Vec<Line> {
        self.main
            .iter()
            .filter_map(|m| {
                let me = if by_label { m.label } else { m.stored? };
                let sets = sets_of(who, me, scene, self.shots, &self.fades, self.ep.rate, capped);
                for s in 0..self.shots.len() {
                    assert!(!sets[0][s] || sets.iter().all(|set| set[s]), "the own shots are in every set");
                    assert!(!sets[WALK][s] || sets[KEPT][s], "the bridged walk holds the walk");
                    assert!(!sets[KEPT][s] || sets[BRIDGED][s]);
                    assert!(!sets[BRIDGED][s] || sets[SCENES][s], "the walks stay inside their scenes");
                    assert!(!sets[KEPT][s] || sets[PARTNERS][s], "a walk holds no shot the filter drops");
                    assert!(!sets[PARTNERS][s] || sets[ANY][s]);
                    assert!(!sets[ANY][s] || sets[SCENES][s]);
                }
                let tallies = std::array::from_fn(|k| tally(&sets[k], &sets[0], m.label, self.truth));
                Some(Line { name: m.name.clone(), label: m.label, sets, tallies })
            })
            .collect()
    }
}

/// One measure of one set over the characters: its mean over the characters
/// that have it, the 95 % interval of that mean, and the pooled share.
struct Stat {
    /// How many characters have the measure.
    n: usize,
    mean: Option<f64>,
    ci: Option<(f64, f64)>,
    pooled: Option<f64>,
}

impl Stat {
    /// `part` of `whole`, per character and pooled.
    fn of(t: &[&Tally], part: &dyn Fn(&Tally) -> f64, whole: &dyn Fn(&Tally) -> f64) -> Self {
        let each: Vec<f64> = t.iter().filter_map(|x| share(part(x), whole(x))).collect();
        let sum = |f: &dyn Fn(&Tally) -> f64| t.iter().map(|x| f(x)).sum::<f64>();
        Self { n: each.len(), mean: mean(&each), ci: interval(&each), pooled: share(sum(part), sum(whole)) }
    }

    /// `0.985 (0.964 to 1.000) 0.965`: the mean, its interval, the pooled share.
    fn cell(&self) -> String {
        let ci = self.ci.map_or_else(|| "      –       ".to_owned(), |(a, b)| format!("{a:.3} to {b:.3}"));
        format!("{} ({ci}) {}", show(self.mean), show(self.pooled))
    }
}

/// The measures of one set over the characters.
struct Summary {
    shots: f64,
    secs: f64,
    added: f64,
    added_secs: f64,
    /// (1): added shots that show a speaker of their scene; by seconds; over
    /// the scenes in which someone with a face label speaks; and counting
    /// only the shots that lie in a scene of the character's.
    speaker: Stat,
    speaker_secs: Stat,
    speaker_spoken: Stat,
    speaker_theirs: Stat,
    /// Added shots in a scene the character has no own shot in, over the cast.
    outside: u32,
    /// (3): added shots with faces that do not speak, and with no face.
    quiet: Stat,
    empty: Stat,
    /// (2): the others' speech whose speaker is seen in the added shots, in the set.
    heard_added: Stat,
    heard_set: Stat,
    /// (4): the speaker shots of the character's scenes the set holds.
    held: Stat,
}

fn summary(lines: &[Line], k: usize) -> Summary {
    let t: Vec<&Tally> = lines.iter().map(|l| &l.tallies[k]).collect();
    let n = t.len().max(1) as f64;
    let sum = |f: &dyn Fn(&Tally) -> f64| t.iter().map(|x| f(x)).sum::<f64>();
    let added = |x: &Tally| f64::from(x.added);
    Summary {
        shots: sum(&|x| x.shots.into()) / n,
        secs: sum(&|x| x.secs) / n,
        added: sum(&added) / n,
        added_secs: sum(&|x| x.added_secs) / n,
        speaker: Stat::of(&t, &|x| x.speaker.into(), &added),
        speaker_secs: Stat::of(&t, &|x| x.speaker_secs, &|x| x.added_secs),
        speaker_spoken: Stat::of(&t, &|x| x.speaker.into(), &|x| x.added_spoken.into()),
        speaker_theirs: Stat::of(&t, &|x| x.speaker_theirs.into(), &added),
        outside: t.iter().map(|x| x.outside).sum(),
        quiet: Stat::of(&t, &|x| x.quiet.into(), &added),
        empty: Stat::of(&t, &|x| x.empty.into(), &added),
        heard_added: Stat::of(&t, &|x| x.heard_added, &|x| x.others),
        heard_set: Stat::of(&t, &|x| x.heard_set, &|x| x.others),
        held: Stat::of(&t, &|x| x.held.into(), &|x| x.wanted.into()),
    }
}

fn print_reading(what: &str, lines: &[Line]) {
    println!("\n== {what} ==");
    println!(
        "{:10} {:7} {:>5} {:>7} {:>5} {:>7} | {:>7} {:>7} {:>7} {:>7} {:>4} | {:>6} {:>6} {:>4} | {:>7} {:>7} {:>7} | {:>9}",
        "character",
        "set",
        "shots",
        "secs",
        "added",
        "secs",
        "speaker",
        "by secs",
        "spoken",
        "theirs",
        "out",
        "quiet",
        "empty",
        "own",
        "others",
        "in add.",
        "in set",
        "held"
    );
    for l in lines {
        for (k, x) in l.tallies.iter().enumerate() {
            println!(
                "{:10} {:7} {:>5} {:>7.1} {:>5} {:>7.1} | {:>7} {:>7} {:>7} {:>7} {:>4} | {:>6} {:>6} {:>4} | {:>7.0} {:>7} {:>7} | {:>4}/{:<4}",
                if k == 0 { l.name.as_str() } else { "" },
                SETS[k],
                x.shots,
                x.secs,
                x.added,
                x.added_secs,
                show(share(x.speaker.into(), x.added.into())),
                show(share(x.speaker_secs, x.added_secs)),
                show(share(x.speaker.into(), x.added_spoken.into())),
                show(share(x.speaker_theirs.into(), x.added.into())),
                x.outside,
                show(share(x.quiet.into(), x.added.into())),
                show(share(x.empty.into(), x.added.into())),
                x.own_face,
                x.others,
                show(share(x.heard_added, x.others)),
                show(share(x.heard_set, x.others)),
                x.held,
                x.wanted
            );
        }
    }
    let all: Vec<Summary> = (0..SETS.len()).map(|k| summary(lines, k)).collect();
    println!(
        "mean over the main cast ({} characters); each share as `mean (95 % interval) pooled`, over the characters that have added shots:",
        lines.len()
    );
    println!(
        "{:7} {:>6} {:>7} {:>6} {:>7} {:>3} | {:<28} | {:<28} | {:<28} | {:<28} | {:>4}",
        "set",
        "shots",
        "secs",
        "added",
        "secs",
        "n",
        "(1) speaker",
        "(1) by seconds",
        "(1) scenes with named speech",
        "(1) in a scene of theirs",
        "out"
    );
    for (set, s) in SETS.iter().zip(&all) {
        println!(
            "{:7} {:>6.1} {:>7.1} {:>6.1} {:>7.1} {:>3} | {:<28} | {:<28} | {:<28} | {:<28} | {:>4}",
            set,
            s.shots,
            s.secs,
            s.added,
            s.added_secs,
            s.speaker.n,
            s.speaker.cell(),
            s.speaker_secs.cell(),
            s.speaker_spoken.cell(),
            s.speaker_theirs.cell(),
            s.outside
        );
    }
    println!(
        "{:7} | {:<28} | {:<28} | {:<28} | {:<28} | {:<28}",
        "set",
        "(3) faces, no speaker",
        "(3) no labelled face",
        "(2) speech seen in added",
        "(2) speech seen in the set",
        "(4) speaker shots held"
    );
    for (set, s) in SETS.iter().zip(&all) {
        println!(
            "{:7} | {:<28} | {:<28} | {:<28} | {:<28} | {:<28}",
            set,
            s.quiet.cell(),
            s.empty.cell(),
            s.heard_added.cell(),
            s.heard_set.cell(),
            s.held.cell()
        );
    }
}

/// How two sets differ, character by character: the mean of the differences
/// of a share (over the characters that have it in both sets) and its 95 %
/// interval.
fn print_pairs(what: &str, lines: &[Line]) {
    type Part = fn(&Tally) -> f64;
    let measures: [(&str, Part, Part); 3] = [
        ("(1) speaker", |x| x.speaker.into(), |x| x.added.into()),
        ("(1) in a scene of theirs", |x| x.speaker_theirs.into(), |x| x.added.into()),
        ("(4) speaker shots held", |x| x.held.into(), |x| x.wanted.into()),
    ];
    println!("\n{what}: one set against another, character by character (mean difference, 95 % interval, characters):");
    for (a, b) in [(KEPT, WALK), (PARTNERS, KEPT), (ANY, PARTNERS), (ANY, KEPT), (ANY, WALK), (SCENES, ANY)] {
        let cells: Vec<String> = measures
            .iter()
            .map(|(name, part, whole)| {
                let d: Vec<f64> = lines
                    .iter()
                    .filter_map(|l| {
                        let of = |k: usize| share(part(&l.tallies[k]), whole(&l.tallies[k]));
                        Some(of(a)? - of(b)?)
                    })
                    .collect();
                let ci = interval(&d).map_or_else(|| "–".to_owned(), |(lo, hi)| format!("{lo:+.3} to {hi:+.3}"));
                format!("{name} {:+.3} ({ci}; {})", mean(&d).unwrap_or(f64::NAN), d.len())
            })
            .collect();
        println!("  {} minus {}: {}", SETS[a], SETS[b], cells.join("; "));
    }
}

/// Why a walk (the set `k`) leaves out shots that show a speaker of the
/// character's scenes: counted over the main cast.
fn print_misses(what: &str, k: usize, lines: &[Line], who: &Who, main: &[usize], fps: f64, truth: &Truth) {
    let (mut scene, mut nobody, mut others, mut behind) = (0, 0, 0, 0);
    for (l, &me) in lines.iter().zip(main) {
        let mut keep = who.partners(me, (TOGETHER_SEC * fps).ceil() as u32);
        keep.insert(me);
        let mine: HashSet<usize> = (0..l.sets[0].len()).filter(|&s| l.sets[0][s]).map(|s| truth.scene[s]).collect();
        for s in 0..l.sets[0].len() {
            let wanted = mine.contains(&truth.scene[s])
                && !l.sets[0][s]
                && !truth.faces[s].contains(&l.label)
                && truth.shows_speaker(s);
            if !wanted || l.sets[k][s] {
                continue;
            }
            if !l.sets[SCENES][s] {
                scene += 1;
            } else if who.shots[s].is_empty() {
                nobody += 1;
            } else if !who.shots[s].iter().any(|p| keep.contains(p)) {
                others += 1;
            } else {
                behind += 1;
            }
        }
    }
    println!(
        "{what}, {}: speaker shots left out: {} the cast shows nobody in, {} it shows only people who never share a frame with the character in, {} behind such a shot, {} in a scene found apart from the own shots",
        SETS[k], nobody, others, behind, scene
    );
}

fn tsv(out: &mut String, reading: &str, lines: &[Line]) {
    for l in lines {
        for (k, x) in l.tallies.iter().enumerate() {
            writeln!(
                out,
                "{reading}\t{}\t{}\t{}\t{:.1}\t{}\t{:.1}\t{}\t{:.1}\t{}\t{}\t{}\t{}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}\t{}\t{}",
                l.name,
                SETS[k],
                x.shots,
                x.secs,
                x.added,
                x.added_secs,
                x.speaker,
                x.speaker_secs,
                x.added_spoken,
                x.quiet,
                x.empty,
                x.own_face,
                x.others,
                x.heard_added,
                x.heard_set,
                x.wanted,
                x.held,
                x.outside,
                x.speaker_theirs
            )
            .unwrap();
        }
    }
}

/// What the hand labels (`bench/identity`) and the speakers' seconds
/// (`bench/scenes`) say of an episode.
struct Gold {
    /// The people the labels name, in the order of their names.
    names: Vec<String>,
    by_label: Who,
    truth: Truth,
    /// Whether a labelled scene starts after each shot but the last.
    cuts: Vec<bool>,
}

fn gold(shots: &[Shot], fps: f64, scenes: &Path, labels: &[Label], speakers_path: &Path, sure_only: bool) -> Gold {
    let n = shots.len();
    let (cuts, said) = scene_labels(scenes, shots);
    let speakers = speaker_labels(speakers_path, sure_only);

    // The people the labels name, numbered by name.
    let names: Vec<String> = labels
        .iter()
        .map(|l| l.name.as_str())
        .filter(|x| named(x))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let number: HashMap<&str, usize> = names.iter().enumerate().map(|(i, x)| (x.as_str(), i)).collect();
    let mut by_label = Who { shots: vec![Vec::new(); n], spans: vec![Vec::new(); names.len()] };
    let mut any_face = vec![false; n];
    for l in labels {
        for s in shots_under(shots, l) {
            any_face[s] = true;
            if let Some(&p) = number.get(l.name.as_str())
                && !by_label.shots[s].contains(&p)
            {
                by_label.shots[s].push(p);
            }
        }
        if let Some(&p) = number.get(l.name.as_str()) {
            by_label.spans[p].push((l.start, l.end.max(l.start + 1)));
        }
    }

    let speech: Vec<Vec<(Vec<usize>, f64)>> = said
        .iter()
        .map(|scene| {
            scene
                .iter()
                .map(|(who, secs)| {
                    let faces = speakers
                        .get(who)
                        .unwrap_or_else(|| panic!("{who}: a speaker without a row in {}", speakers_path.display()));
                    let faces = faces
                        .iter()
                        .map(|f| *number.get(f.as_str()).unwrap_or_else(|| panic!("{f}: no such face label")))
                        .collect();
                    (faces, *secs)
                })
                .collect()
        })
        .collect();
    let truth = Truth {
        scene: scene_ids(&cuts),
        faces: by_label.shots.clone(),
        any_face,
        speech,
        secs: shots.iter().map(|s| f64::from(s.end - s.start) / fps).collect(),
    };
    Gold { names, by_label, truth, cuts }
}

/// The scenes the app would find: its live-action recipe on its pictures of
/// the shots.
fn found_scenes(pictures: &Path, shots: &[Shot], between: &[Between], fps: f64) -> Vec<usize> {
    let vectors = pictures_of(pictures, shots);
    let secs: Vec<f32> = shots.iter().map(|s| (f64::from(s.end - s.start) / fps) as f32).collect();
    scenes::scenes(shots, between, &secs, None, &vectors, &Recipe::LIVE)
}

/// What every run prints first: the shots, the scenes on both sides, the
/// cast, and what any shot scores on measure (1) with no rule at all.
fn print_head(ep: &Episode, shots: &[Shot], truth: &Truth, cuts: &[bool], found: &[usize], sure_only: bool) {
    let n = shots.len();
    let found_cuts: Vec<bool> = found.windows(2).map(|w| w[0] != w[1]).collect();
    if let Ok(path) = std::env::var("FOUND") {
        let mut out = String::from("shot\tstart\tend\tlabelled_scene\tfound_scene\n");
        for s in 0..n {
            writeln!(out, "{s}\t{}\t{}\t{}\t{}", shots[s].start, shots[s].end, truth.scene[s], found[s]).unwrap();
        }
        std::fs::write(&path, out).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    let both = (0..n - 1).filter(|&g| found_cuts[g] && cuts[g]).count();
    let (cut, labelled) = (found_cuts.iter().filter(|&&c| c).count(), cuts.iter().filter(|&&c| c).count());
    println!(
        "{n} shots, {:.1} s; {} labelled scenes, {} of them with speech of someone who has a face label; the app's recipe finds {} scenes ({both} of its {cut} boundaries are labelled ones, of {labelled})",
        truth.secs.iter().sum::<f64>(),
        labelled + 1,
        truth.speech.iter().filter(|s| s.iter().any(|(who, _)| !who.is_empty())).count(),
        cut + 1
    );
    let hidden = ep.characters.iter().filter(|c| c.1).count();
    println!(
        "{} stored characters, {hidden} of them hidden (left out, as a pack leaves them out)",
        ep.characters.len()
    );
    if sure_only {
        println!("SURE=1: the speakers whose face label is unsure have no face here");
    }
    // What any shot with a face scores on measure (1), with no rule at all.
    let rate = |of: &dyn Fn(usize) -> bool| {
        let (with, all) =
            ((0..n).filter(|&s| of(s) && truth.shows_speaker(s)).count(), (0..n).filter(|&s| of(s)).count());
        format!("{with} of {all} ({:.3})", with as f64 / all.max(1) as f64)
    };
    println!(
        "a speaker of their labelled scene is shown by {} shots in which the cast is sure of someone, by {} shots with a named face label, by {} shots in all",
        rate(&|s| !shots[s].cast.is_empty()),
        rate(&|s| !truth.faces[s].is_empty()),
        rate(&|_| true)
    );
}

/// A run's readings with the stored cast: printed, and written as a table.
struct Run<'a> {
    ep: &'a Episode,
    shots: &'a [Shot],
    truth: &'a Truth,
    /// The names the truth numbers people by.
    names: &'a [&'a str],
    who: &'a Who,
    main: &'a [Main],
    found: &'a [usize],
}

impl Run<'_> {
    /// `by_label`: who is where by labels that carry times, for the reading
    /// with labels for both; shot labels carry none.
    fn report(&self, by_label: Option<&Who>, out: Option<&String>) {
        let (ep, shots, truth, who, main, found) = (self.ep, self.shots, self.truth, self.who, self.main, self.found);
        let n = shots.len();
        let fps = ep.fps();
        let bench = Bench { ep, shots, fades: fades_of(ep), truth, main };
        let app = bench.reading(who, found, false, false);
        let cast_labelled = bench.reading(who, &truth.scene, false, false);
        let labelled_both = by_label.map(|w| bench.reading(w, &truth.scene, true, false));
        print_reading("as the app has it: the stored cast, the app's scenes", &app);
        print_reading("the stored cast, the labelled scenes", &cast_labelled);
        if let Some(lines) = &labelled_both {
            print_reading("the labels: who is where, and the scenes", lines);
        }

        println!();
        let stored_of: Vec<usize> = main.iter().filter_map(|m| m.stored).collect();
        let label_of_main: Vec<usize> = main.iter().map(|m| m.label).collect();
        for k in [WALK, KEPT] {
            print_misses("as the app has it", k, &app, who, &stored_of, fps, truth);
            print_misses("the stored cast, the labelled scenes", k, &cast_labelled, who, &stored_of, fps, truth);
            if let (Some(lines), Some(w)) = (&labelled_both, by_label) {
                print_misses("the labels", k, lines, w, &label_of_main, fps, truth);
            }
        }

        print_pairs("as the app has it", &app);
        print_pairs("the stored cast, the labelled scenes", &cast_labelled);

        // Side figures: 0.1's cap, and groups that leave their scenes.
        let capped = bench.reading(who, found, false, true);
        let cut_off: usize =
            app.iter().zip(&capped).map(|(a, b)| (0..n).filter(|&s| a.sets[WALK][s] && !b.sets[WALK][s]).count()).sum();
        let s = summary(&capped, WALK);
        println!(
            "\nwith 0.1's cap of {CAP_SEC} s per clip (as the app has it): {cut_off} shots fewer over the main cast; speaker {} ({}), pooled {}",
            show(s.speaker.mean),
            s.speaker.ci.map_or_else(|| "–".to_owned(), |(a, b)| format!("{a:.3} to {b:.3}")),
            show(s.speaker.pooled)
        );
        let outside: usize =
            app.iter().map(|l| (0..n).filter(|&s| l.sets[GROUPS][s] && !l.sets[SCENES][s]).count()).sum();
        let in_groups: usize = app.iter().map(|l| l.tallies[GROUPS].added as usize).sum();
        println!(
            "shot groups (as the app has it): {outside} of their {in_groups} added shots lie outside the character's scenes"
        );

        if let Ok(name) = std::env::var("DETAIL") {
            let l = app.iter().find(|l| l.name == name).expect("DETAIL=<a main character's label, one the cast holds>");
            println!("\n{name}, as the app has it: every shot of the labelled and found scenes with an own shot");
            println!("shot\tframe\tsecs\tscene\tfound\tsets\tspeaker\tcast\tlabels");
            let mine: HashSet<usize> = (0..n).filter(|&s| l.sets[0][s]).map(|s| truth.scene[s]).collect();
            for s in (0..n).filter(|&s| mine.contains(&truth.scene[s]) || l.sets[SCENES][s]) {
                // own, groups, walk, bridged, kept of the bridged, scenes with anyone, with a partner, scenes
                let flags: String =
                    (0..SETS.len()).map(|k| if l.sets[k][s] { b"ogwbkaps"[k] as char } else { '-' }).collect();
                let cast: Vec<&str> = who.shots[s].iter().map(|&c| ep.characters[c].0.as_str()).collect();
                let seen: Vec<&str> = truth.faces[s].iter().map(|&p| self.names[p]).collect();
                println!(
                    "{s}\t{}\t{:.1}\t{}\t{}\t{flags}\t{}\t{}\t{}",
                    shots[s].start,
                    truth.secs[s],
                    truth.scene[s],
                    found[s],
                    if truth.shows_speaker(s) {
                        "speaker"
                    } else if truth.any_face[s] {
                        "quiet"
                    } else {
                        "empty"
                    },
                    cast.join(","),
                    seen.join(",")
                );
            }
        }

        if let Some(path) = out {
            let mut out = String::from(
                "reading\tcharacter\tset\tshots\tseconds\tadded_shots\tadded_seconds\tspeaker_shots\tspeaker_seconds\tadded_in_scenes_with_speech\tfaces_no_speaker\tno_face\town_face_missed\tothers_speech_s\tothers_speech_seen_in_added_s\tothers_speech_seen_in_set_s\tspeaker_shots_of_their_scenes\tof_them_held\tadded_outside_their_scenes\tspeaker_shots_added_in_their_scenes\n",
            );
            tsv(&mut out, "app", &app);
            tsv(&mut out, "cast_labelled_scenes", &cast_labelled);
            if let Some(lines) = &labelled_both {
                tsv(&mut out, "labels", lines);
            }
            std::fs::write(path, out).unwrap_or_else(|e| panic!("{path}: {e}"));
        }
    }
}

/// The run of rows 25, 27 and 28: hand labels say who is where, and a file
/// of speakers' seconds who speaks in each scene.
fn by_labels(args: &[String]) {
    let ep = load(Path::new(&args[0]));
    let (shots, between) = shots_of(&ep);
    let fps = ep.fps();
    let labels = face_labels(Path::new(&args[2]));
    let sure_only = std::env::var("SURE").is_ok();
    let Gold { names, by_label, truth, cuts } =
        gold(&shots, fps, Path::new(&args[1]), &labels, Path::new(&args[3]), sure_only);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let number: HashMap<&str, usize> = names.iter().enumerate().map(|(i, &x)| (x, i)).collect();

    let found = found_scenes(Path::new(&args[4]), &shots, &between, fps);
    print_head(&ep, &shots, &truth, &cuts, &found, sure_only);

    // The main cast, and the stored character most of each one's faces are on.
    let who = stored(&ep, &shots);
    let mut screen: BTreeMap<&str, u32> = BTreeMap::new();
    for l in labels.iter().filter(|l| named(&l.name)) {
        *screen.entry(&l.name).or_default() += l.end - l.start;
    }
    let mut cast: Vec<(&str, u32)> = screen.into_iter().filter(|&(_, f)| f64::from(f) / fps >= MAIN_SEC).collect();
    cast.sort_by_key(|&(name, f)| (std::cmp::Reverse(f), name));
    let mut held: HashMap<(&str, usize), u32> = HashMap::new();
    for t in &ep.tracks {
        if let (Some(c), Some((name, dets))) = (t.character.filter(|&c| !ep.characters[c].1), label_of(t, &labels)) {
            *held.entry((name, c)).or_default() += dets;
        }
    }
    println!(
        "\nmain cast (at least {MAIN_SEC} s on screen), the stored character most of their faces are on, who they share frames with:"
    );
    let together = (TOGETHER_SEC * fps).ceil() as u32;
    let mut main = Vec::new();
    for &(name, frames) in &cast {
        let theirs =
            held.iter().filter(|((who, _), _)| *who == name).max_by_key(|&((_, c), &d)| (d, std::cmp::Reverse(*c)));
        let stored = theirs.map(|(&(_, c), _)| c);
        let label = number[name];
        let partners = |w: &Who, me: usize, of: &dyn Fn(usize) -> String| {
            let mut p: Vec<String> = w.partners(me, together).into_iter().map(of).collect();
            p.sort();
            p
        };
        let by_cast = stored.map_or_else(Vec::new, |c| partners(&who, c, &|p| ep.characters[p].0.clone()));
        let by_labels = partners(&by_label, label, &|p| names[p].to_owned());
        println!(
            "  {name:9} {:6.1} s, in {:3} shots by the labels and {:3} by the cast ({}); partners by the cast: {}; by the labels: {} ({})",
            f64::from(frames) / fps,
            by_label.shots.iter().filter(|s| s.contains(&label)).count(),
            stored.map_or(0, |c| who.shots.iter().filter(|s| s.contains(&c)).count()),
            stored.map_or("nobody", |c| ep.characters[c].0.as_str()),
            by_cast.len(),
            by_labels.len(),
            by_labels.join(", ")
        );
        if stored.is_none() {
            println!("    the cast holds none of their faces: left out of the readings with the stored cast");
        }
        main.push(Main { name: name.to_owned(), label, stored });
    }

    let run = Run { ep: &ep, shots: &shots, truth: &truth, names: &names, who: &who, main: &main, found: &found };
    run.report(Some(&by_label), args.get(5));
}

/// The run of row 29, on a title without hand labels: two readers' shot
/// labels say who is seen and who speaks, a cast sheet which stored
/// characters each of their names is.
fn by_readers(args: &[String]) {
    let ep = load(Path::new(&args[0]));
    let (shots, between) = shots_of(&ep);
    let fps = ep.fps();
    let (cuts, _) = scene_labels(Path::new(&args[1]), &shots);
    let cast = cast_sheet(Path::new(&args[3]), &ep);
    let names: Vec<&str> = cast.iter().map(|c| c.0.as_str()).collect();
    let number: HashMap<&str, usize> = names.iter().enumerate().map(|(i, &x)| (x, i)).collect();
    assert_eq!(number.len(), names.len(), "a label stands twice on the cast sheet");
    let read = shot_labels(Path::new(&args[2]), &shots);
    assert!(read.iter().all(Option::is_some), "a shot without a row in {}", args[2]);
    let secs: Vec<f64> = shots.iter().map(|s| f64::from(s.end - s.start) / fps).collect();
    let truth = readers_truth(&read, scene_ids(&cuts), &number, secs);

    let found = found_scenes(Path::new(&args[4]), &shots, &between, fps);
    print_head(&ep, &shots, &truth, &cuts, &found, false);
    let heard: f64 = read.iter().flatten().map(|r| r.speech).sum();
    let known: f64 = truth.speech.iter().flatten().filter(|(who, _)| !who.is_empty()).map(|(_, secs)| secs).sum();
    println!(
        "{heard:.0} s of speech heard; {known:.0} s of it in shots in which both readers mark the same person speaking"
    );
    print_readers(&read);

    // The main cast: the people of the cast sheet whose stored character has
    // faces for long enough; of two stored characters the one with more.
    let who = stored(&ep, &shots);
    let on_screen = |c: usize| who.spans[c].iter().map(|&(a, b)| f64::from(b - a)).sum::<f64>() / fps;
    let mut rows: Vec<(f64, usize, usize)> = cast
        .iter()
        .enumerate()
        .filter_map(|(label, (_, characters))| {
            let (secs, c) = characters
                .iter()
                .filter(|&&c| !ep.characters[c].1)
                .map(|&c| (on_screen(c), c))
                .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))?;
            (secs >= MAIN_SEC).then_some((secs, label, c))
        })
        .collect();
    rows.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    println!(
        "\nmain cast (the people of the cast sheet whose stored character has at least {MAIN_SEC} s of faces), who they share frames with by the cast:"
    );
    let together = (TOGETHER_SEC * fps).ceil() as u32;
    let mut main = Vec::new();
    for (secs, label, c) in rows {
        println!(
            "  {:12} {secs:6.1} s, in {:3} shots by both readers and {:3} by the cast ({}); partners by the cast: {}",
            names[label],
            truth.faces.iter().filter(|s| s.contains(&label)).count(),
            who.shots.iter().filter(|s| s.contains(&c)).count(),
            ep.characters[c].0,
            who.partners(c, together).len()
        );
        main.push(Main { name: names[label].to_owned(), label, stored: Some(c) });
    }

    let run = Run { ep: &ep, shots: &shots, truth: &truth, names: &names, who: &who, main: &main, found: &found };
    run.report(None, args.get(5));
}

/// How far the two readers agree with each other, over the shots they read.
fn print_readers(read: &[Option<Read>]) {
    let rows: Vec<&Read> = read.iter().flatten().collect();
    let same = |of: &dyn Fn(&Read) -> &[Vec<String>; 2]| {
        let equal = rows.iter().filter(|r| of(r)[0] == of(r)[1]).count();
        let (mut common, mut either) = (0, 0);
        for r in &rows {
            let [a, b] = of(r);
            let shared = a.iter().filter(|p| b.contains(p)).count();
            common += shared;
            either += a.len() + b.len() - shared;
        }
        format!(
            "the same list in {equal} of {} shots ({:.3}); of {either} names either gives, {common} are given by both ({:.3})",
            rows.len(),
            equal as f64 / rows.len().max(1) as f64,
            common as f64 / either.max(1) as f64
        )
    };
    println!("the two readers, who is seen: {}", same(&|r| &r.faces));
    println!("the two readers, who speaks: {}", same(&|r| &r.speaks));
}

/// Two yes-or-no answers over the same shots: both yes, only the first, only
/// the second, both no.
fn answers(pairs: impl Iterator<Item = (bool, bool)>) -> [u32; 4] {
    let mut c = [0; 4];
    for (a, b) in pairs {
        c[match (a, b) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 3,
        }] += 1;
    }
    c
}

/// The share of equal answers, and Cohen's kappa (the agreement beyond what
/// two answerers with these yes rates reach by chance).
fn agreement(c: [u32; 4]) -> (f64, f64) {
    let n = f64::from(c.iter().sum::<u32>());
    let same = f64::from(c[0] + c[3]) / n;
    let (first, second) = (f64::from(c[0] + c[1]) / n, f64::from(c[0] + c[2]) / n);
    let chance = first * second + (1.0 - first) * (1.0 - second);
    (same, if chance < 1.0 { (same - chance) / (1.0 - chance) } else { 1.0 })
}

/// Row 29's check of the readers: their shot labels against the hand labels
/// and the speakers' seconds, on the shots they read.
fn agree(args: &[String]) {
    let ep = load(Path::new(&args[0]));
    let (shots, _) = shots_of(&ep);
    let n = shots.len();
    let fps = ep.fps();
    let labels = face_labels(Path::new(&args[2]));
    let sure_only = std::env::var("SURE").is_ok();
    let gold = gold(&shots, fps, Path::new(&args[1]), &labels, Path::new(&args[3]), sure_only);
    let number: HashMap<&str, usize> = gold.names.iter().enumerate().map(|(i, x)| (x.as_str(), i)).collect();
    let read = shot_labels(Path::new(&args[4]), &shots);
    let readers = readers_truth(&read, gold.truth.scene.clone(), &number, gold.truth.secs.clone());
    let at: Vec<usize> = (0..n).filter(|&s| read[s].is_some()).collect();
    let scenes: BTreeSet<usize> = at.iter().map(|&s| gold.truth.scene[s]).collect();
    assert!(
        (0..n).all(|s| read[s].is_some() == scenes.contains(&gold.truth.scene[s])),
        "the readers read whole labelled scenes"
    );
    println!("{} shots of {} labelled scenes read by both readers ({n} shots in the episode)", at.len(), scenes.len());
    if sure_only {
        println!("SURE=1: the speakers whose face label is unsure have no face here");
    }

    // The atom of measures (1) and (4): does the shot show a speaker of its scene?
    let print = |what: &str, of: &dyn Fn(usize) -> bool| {
        let c =
            answers(at.iter().filter(|&&s| of(s)).map(|&s| (gold.truth.shows_speaker(s), readers.shows_speaker(s))));
        let (same, kappa) = agreement(c);
        let all: u32 = c.iter().sum();
        println!(
            "{what}: {all} shots; shows a speaker of its scene: both yes {}, only the labels {}, only the readers {}, both no {}; the same answer in {same:.3}, kappa {kappa:.3}; yes by the labels {:.3}, by the readers {:.3}",
            c[0],
            c[1],
            c[2],
            c[3],
            f64::from(c[0] + c[1]) / f64::from(all.max(1)),
            f64::from(c[0] + c[2]) / f64::from(all.max(1))
        );
    };
    print("all shots read", &|_| true);
    print("shots in which the cast is sure of someone", &|s| !shots[s].cast.is_empty());
    print("shots with a named face label", &|s| !gold.truth.faces[s].is_empty());

    // Who speaks in each scene.
    let (mut asked, mut found, mut asked_secs, mut found_secs, mut named_by, mut right) = (0, 0, 0.0, 0.0, 0, 0);
    for &scene in &scenes {
        let theirs: HashSet<usize> = readers.speech[scene].iter().flat_map(|(who, _)| who.iter().copied()).collect();
        let truly: HashSet<usize> = gold.truth.speech[scene].iter().flat_map(|(who, _)| who.iter().copied()).collect();
        for (who, secs) in gold.truth.speech[scene].iter().filter(|(who, _)| !who.is_empty()) {
            let hit = who.iter().any(|w| theirs.contains(w));
            asked += 1;
            asked_secs += secs;
            found += u32::from(hit);
            found_secs += if hit { *secs } else { 0.0 };
        }
        named_by += theirs.len();
        right += theirs.iter().filter(|w| truly.contains(w)).count();
    }
    println!(
        "speakers of the scenes: the labels name {asked} (with a face label), the readers find {found} of them ({:.3}; {:.3} of their {asked_secs:.0} s of speech); the readers name {named_by}, {right} of them speak by the labels ({:.3})",
        f64::from(found) / f64::from(asked.max(1)),
        found_secs / asked_secs.max(1e-9),
        right as f64 / named_by.max(1) as f64
    );

    // Who is seen in each shot.
    let (mut seen, mut both, mut given) = (0, 0, 0);
    for &s in &at {
        seen += gold.truth.faces[s].len();
        given += readers.faces[s].len();
        both += readers.faces[s].iter().filter(|p| gold.truth.faces[s].contains(p)).count();
    }
    println!(
        "names per shot: the labels give {seen}, both readers {given}, {both} of them the same: precision {:.3}, recall {:.3}",
        both as f64 / given.max(1) as f64,
        both as f64 / seen.max(1) as f64
    );

    print_readers(&read);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("shots") if (6..=7).contains(&args.len()) => by_readers(&args[1..]),
        Some("agree") if args.len() == 6 => agree(&args[1..]),
        Some(first) if first != "shots" && first != "agree" && (5..=6).contains(&args.len()) => by_labels(&args),
        _ => {
            eprintln!(
                "usage: conversation_bench <project_copy_dir> <scenes.tsv> <identity labels.tsv> <speakers.tsv> <pictures.emb> [out.tsv]\n       conversation_bench shots <project_copy_dir> <scenes.tsv> <shot labels.tsv> <cast.tsv> <pictures.emb> [out.tsv]\n       conversation_bench agree <project_copy_dir> <scenes.tsv> <identity labels.tsv> <speakers.tsv> <shot labels.tsv>"
            );
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shots(n: u32) -> Vec<Shot> {
        (0..n).map(|i| Shot::new(i * 100, i * 100 + 100)).collect()
    }

    /// Six shots, two scenes of three; who is in which shot.
    fn who(cast: &[&[usize]], spans: &[&[(u32, u32)]]) -> Who {
        Who { shots: cast.iter().map(|c| c.to_vec()).collect(), spans: spans.iter().map(|s| s.to_vec()).collect() }
    }

    #[test]
    fn partners_share_frames_long_enough() {
        // 0 and 1 are on screen together for 10 frames, 0 and 2 for 4, 0 and 3 never.
        let w = who(&[], &[&[(0, 50)], &[(40, 90)], &[(46, 60)], &[(50, 99)]]);
        assert_eq!(w.partners(0, 5), HashSet::from([1]));
        assert_eq!(w.partners(0, 4), HashSet::from([1, 2]));
        assert_eq!(w.partners(3, 1), HashSet::from([1, 2]));
    }

    #[test]
    fn the_walk_goes_over_partners_and_stops() {
        // Shots: me | partner | me | nobody | partner | stranger | me (other scene).
        let own = [true, false, true, false, false, false, true];
        let keeps = [true, true, true, false, true, false, true];
        let scene = [0, 0, 0, 0, 0, 0, 1];
        assert_eq!(walk(&own, &keeps, &scene, None), [true, true, true, false, false, false, true]);
        // A partner's shot before the scene's first own shot joins too, and
        // the scene's edge stops the walk though the next shot holds them.
        let own = [false, true, false, false];
        let keeps = [true, true, true, true];
        assert_eq!(walk(&own, &keeps, &[0, 0, 0, 1], None), [true, true, true, false]);
        assert_eq!(whole_scenes(&own, &[0, 0, 0, 1]), [true, true, true, false]);
        // Capped: 100 frames a shot at 100 fps is 1 s a shot; 2.5 s hold two.
        let s = shots(4);
        let all = [true; 4];
        // Cut from the end farther from the own shot; a tie cuts the start.
        let capped = walk(&[false, true, false, false], &all, &[0; 4], Some((&s, 100.0, 2.5)));
        assert_eq!(capped, [false, true, true, false]);
    }

    #[test]
    fn the_bridged_walk_steps_over_a_short_stretch() {
        // 100 fps, 1 s a shot: me | nobody | partner | 7 x nobody | partner | nobody | partner (other scene).
        let s = shots(13);
        let mut own = [false; 13];
        let mut keeps = [false; 13];
        (own[0], keeps[0], keeps[2], keeps[10], keeps[12]) = (true, true, true, true, true);
        let mut scene = [0; 13];
        scene[12] = 1;
        let bridged = bridged_walk(&own, &keeps, &scene, &s, &[], (100, 1));
        // One shot (1 s) is stepped over, seven (7 s) are not, nor is the scene's edge.
        let mut want = [false; 13];
        want[..3].fill(true);
        assert_eq!(bridged, want);
        assert_eq!(walk(&own, &keeps, &scene, None)[..3], [true, false, false]);
        // It goes back as well as on, and a fade in the stretch stops it.
        own[0] = false;
        own[2] = true;
        assert_eq!(bridged_walk(&own, &keeps, &scene, &s, &[], (100, 1)), want);
        want[..2].fill(false);
        assert_eq!(bridged_walk(&own, &keeps, &scene, &s, &[(150, 160)], (100, 1)), want);
        // Shots that follow each other need no bridge, whatever lies between them.
        keeps[1] = true;
        want[..2].fill(true);
        assert_eq!(bridged_walk(&own, &keeps, &scene, &s, &[(100, 100)], (100, 1)), want);
    }

    #[test]
    fn groups_bridge_a_short_gap_without_a_fade() {
        let s = shots(11);
        let own = [true, false, true, false, false, false, false, false, false, false, true];
        // 1 fps: a shot is 100 s long; a gap of 6 s holds none of them.
        assert_eq!(groups(&own, &s, &[], (1, 1)), own);
        // 100 fps: a shot is 1 s; shot 1 is bridged (a gap of 1 s), shots 3 to 9 are not (7 s).
        let mut bridged = own;
        bridged[1] = true;
        assert_eq!(groups(&own, &s, &[], (100, 1)), bridged);
        // A gap of 4 s is bridged whole.
        let s = shots(8);
        let own = [true, false, false, false, false, true, false, false];
        assert_eq!(groups(&own, &s, &[], (100, 1))[1..5], [true; 4]);
        // A fade in the gap parts them.
        assert_eq!(groups(&own, &s, &[(250, 260)], (100, 1)), own);
        assert!(fade_between(&[(250, 260)], 100, 500) && !fade_between(&[(250, 260)], 300, 500));
        assert!(fade_between(&[(250, 260)], 260, 500), "a fade that touches the gap parts them too");
    }

    #[test]
    fn a_set_is_counted_against_the_labels() {
        // Two scenes of three shots, 1 s a shot. Scene 0: 0 speaks 10 s, 1
        // speaks 6 s, an unnamed voice 2 s. Scene 1: nobody speaks.
        // Faces: 0 | 1 | 2 | 0 | 1 | none.
        let t = Truth {
            scene: vec![0, 0, 0, 1, 1, 1],
            faces: vec![vec![0], vec![1], vec![2], vec![0], vec![1], vec![]],
            any_face: vec![true, true, true, true, true, false],
            speech: vec![vec![(vec![0], 10.0), (vec![1], 6.0), (vec![], 2.0)], vec![]],
            secs: vec![1.0; 6],
        };
        let own = [true, false, false, true, false, false];
        let x = tally(&[true; 6], &own, 0, &t);
        assert_eq!((x.shots, x.added, x.speaker, x.quiet, x.empty), (6, 4, 1, 2, 1));
        assert_eq!(x.added_spoken, 2, "the added shots of the scene with speech");
        assert_eq!((x.others, x.heard_added, x.heard_set), (8.0, 6.0, 6.0));
        assert_eq!((x.wanted, x.held), (1, 1));
        assert_eq!((x.outside, x.speaker_theirs), (0, 1), "both scenes are theirs");
        // The own shots alone: nothing added, nobody else's speech is seen.
        let x = tally(&own, &own, 0, &t);
        assert_eq!((x.shots, x.added, x.heard_set, x.wanted, x.held), (2, 0, 0.0, 1, 0));
        // The cast missed the character in shot 0: there it is an added shot
        // that shows a speaker, the character. Scene 0 is not theirs by the
        // cast, so nothing of it is asked for.
        let missed = [false, false, false, true, false, false];
        let x = tally(&[true, false, false, true, false, false], &missed, 0, &t);
        assert_eq!((x.added, x.speaker, x.own_face), (1, 1, 1));
        assert_eq!((x.others, x.wanted), (0.0, 0));
        // That shot lies outside their scenes, so the stricter count leaves it out.
        assert_eq!((x.outside, x.speaker_theirs), (1, 0));
        // The cast shows them in shot 2 and not in shot 0: scene 0 is theirs,
        // and of its other shots only shot 1 shows a speaker and not them.
        let x = tally(&[true; 6], &[false, false, true, true, false, false], 0, &t);
        assert_eq!((x.wanted, x.held, x.own_face), (1, 1, 1));
    }

    #[test]
    fn the_interval_holds_the_mean() {
        let v = [0.2, 0.4, 0.9, 1.0, 0.5];
        let (a, b) = interval(&v).unwrap();
        assert!(a < 0.6 && 0.6 < b && a >= 0.2 && b <= 1.0, "{a} {b}");
        assert_eq!(interval(&v), interval(&v));
        assert!(interval(&[0.5]).is_none());
    }

    #[test]
    fn two_readers_make_one_truth() {
        // Six shots: a scene of two and a scene of four. Labels: 0 is ann, 1 is bob.
        let number: HashMap<&str, usize> = [("ann", 0), ("bob", 1)].into_iter().collect();
        let read = |speech: f64, faces: [&[&str]; 2], speaks: [&[&str]; 2]| {
            let v = |x: &[&str]| x.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
            Some(Read { speech, faces: [v(faces[0]), v(faces[1])], speaks: [v(speaks[0]), v(speaks[1])] })
        };
        let rows = vec![
            // Both see ann and mark her speaking: the 2 s are hers. Someone unnamed is never credited.
            read(2.0, [&["ann"], &["ann", "other"]], [&["ann"], &["ann"]]),
            // Only the first reader sees bob and marks him: he is not shown, and the 1 s is nobody's.
            read(1.0, [&["ann", "bob"], &["ann"]], [&["bob"], &[]]),
            // The readers mark different people: nobody's 3 s.
            read(3.0, [&["ann", "bob"], &["ann", "bob"]], [&["ann"], &["bob"]]),
            // The second reader marks ann in another shot of the scene than the first did:
            // she speaks in the scene, with no seconds of her own.
            read(0.5, [&["ann"], &["ann"]], [&[], &["ann"]]),
            // bob alone: seen by both, a speaker by one reader only.
            read(0.0, [&["bob"], &["bob"]], [&[], &[]]),
            None,
        ];
        let t = readers_truth(&rows, vec![0, 0, 1, 1, 1, 1], &number, vec![1.0; 6]);
        assert_eq!(t.faces, vec![vec![0], vec![0], vec![0, 1], vec![0], vec![1], vec![]]);
        assert_eq!(t.any_face, vec![true, true, true, true, true, false]);
        assert_eq!(t.speech[0], vec![(vec![0], 2.0), (vec![], 1.0)]);
        assert_eq!(t.speech[1], vec![(vec![0], 0.0), (vec![], 3.5)]);
        let shown: Vec<bool> = (0..6).map(|s| t.shows_speaker(s)).collect();
        assert_eq!(shown, vec![true, true, true, true, false, false]);
    }

    #[test]
    fn agreement_beyond_chance() {
        let c = answers([(true, true), (true, true), (false, false), (true, false)].into_iter());
        assert_eq!(c, [2, 1, 0, 1]);
        let (same, kappa) = agreement(c);
        assert!((same - 0.75).abs() < 1e-12);
        // By chance: 0.75 × 0.5 + 0.25 × 0.5 = 0.5.
        assert!((kappa - 0.5).abs() < 1e-12);
        assert_eq!(agreement([3, 0, 0, 0]), (1.0, 1.0));
    }
}
