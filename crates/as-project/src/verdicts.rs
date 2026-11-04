//! What plugins' shot filters said of a character's shots: for each filter,
//! character and episode, the shots it keeps and the ones it leaves out,
//! by their first frame, with the basis they were judged on.
//!
//! Kept as meta rows (`as_db::VERDICTS_PREFIX`), one per filter, character
//! and episode (about 2 KiB for a lead's episode), so a project keeps its
//! schema and nothing is ever evicted. A row of another basis (the filter's
//! settings or revision changed since) still applies, so a pack doesn't
//! jump back to "every shot in" while new verdicts come, but its shots
//! count as not judged; new verdicts replace the old ones shot by shot.
//! Finding an episode's shots again deletes its rows: the shots they name
//! are gone.
//!
//! The app hands a pack its plugins' filters ([`PluginFilters`]): which
//! there are, what they are called, the basis their verdicts need now and
//! what their check is doing. This crate never reads a plugin.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use as_api::PackFilterState;
use as_core::{CharacterId, FrameIdx, MediaId};
use as_db::VERDICTS_PREFIX;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Project, ProjectResult};

/// The version of a verdict row this AutoScene writes; a row of another is
/// read as none.
const ROW_VERSION: u32 = 1;

/// A plugin's shot filter, as the app hands it to a pack.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginFilter {
    /// `<pluginId>/<filterId>`.
    pub key: String,
    pub title: String,
    pub description: Option<String>,
    /// What a shot it leaves out says.
    pub reason: String,
    /// Its plugin's name.
    pub plugin: String,
    /// What its verdicts are judged on now ([`basis`]): a verdict of
    /// another counts as not judged.
    pub basis: String,
    /// What it is doing; `Off` while its plugin isn't active, and then the
    /// pack ignores it.
    pub state: PackFilterState,
}

/// The plugins' shot filters a pack can show: every one of an active
/// plugin, and those its spec turns on whose plugin isn't (`Off`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginFilters(pub Vec<PluginFilter>);

impl PluginFilters {
    /// No filters: a pack of AutoScene's own filters alone.
    pub const NONE: Self = Self(Vec::new());

    /// The filter `key`, and where it is in the list.
    pub fn find(&self, key: &str) -> Option<(usize, &PluginFilter)> {
        self.0.iter().enumerate().find(|(_, f)| f.key == key)
    }
}

/// What a shot filter's verdicts are judged on: its id (`filter`), its
/// `revision` and the values in `settings` of the settings it `names`, in
/// key order. A setting it doesn't name, a new version of its plugin, or a
/// secret (it has no value here) doesn't change it.
pub fn basis(filter: &str, revision: u32, names: &[String], settings: &BTreeMap<String, Value>) -> String {
    let mut hash = Sha256::new();
    let mut put = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    put(filter.as_bytes());
    put(&revision.to_le_bytes());
    for name in names.iter().collect::<BTreeSet<_>>() {
        put(name.as_bytes());
        put(settings.get(name).unwrap_or(&Value::Null).to_string().as_bytes());
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// One filter's verdicts on one character's shots in one episode, by
/// their first frame, each list sorted.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Row {
    v: u32,
    basis: String,
    #[serde(default)]
    drop: Vec<FrameIdx>,
    #[serde(default)]
    keep: Vec<FrameIdx>,
    /// Shots a verdict of an earlier basis left out that this basis hasn't
    /// judged yet: still out, not judged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stale: Vec<FrameIdx>,
}

impl Row {
    /// The row `json` holds, if it is one this AutoScene reads.
    fn read(json: &str) -> Option<Self> {
        let mut row: Self = serde_json::from_str(json).ok().filter(|r: &Self| r.v == ROW_VERSION)?;
        for list in [&mut row.drop, &mut row.keep, &mut row.stale] {
            list.sort_unstable();
            list.dedup();
        }
        Some(row)
    }

    /// `old` with the verdicts `given` (a later word on a shot counts) on
    /// `basis`: of the same basis, they join it; of another, they replace
    /// it, and what it left out stays out until judged anew.
    fn merged(old: Option<Self>, basis: &str, given: &[(FrameIdx, bool)]) -> Self {
        let set = |v: Vec<FrameIdx>| v.into_iter().collect::<BTreeSet<_>>();
        let (mut drop, mut keep, mut stale) = match old {
            Some(r) if r.basis == basis => (set(r.drop), set(r.keep), set(r.stale)),
            Some(r) => (BTreeSet::new(), BTreeSet::new(), set(r.drop).union(&set(r.stale)).copied().collect()),
            None => Default::default(),
        };
        let judged: BTreeMap<FrameIdx, bool> = given.iter().copied().collect();
        for (start, kept) in judged {
            stale.remove(&start);
            drop.remove(&start);
            keep.remove(&start);
            if kept {
                keep.insert(start)
            } else {
                drop.insert(start)
            };
        }
        let list = |s: BTreeSet<FrameIdx>| s.into_iter().collect();
        Self { v: ROW_VERSION, basis: basis.to_owned(), drop: list(drop), keep: list(keep), stale: list(stale) }
    }
}

/// What one filter said of a character's shots, by episode, and the basis
/// its verdicts need now.
#[derive(Clone, Debug, Default)]
pub(crate) struct Judgement {
    rows: HashMap<MediaId, Row>,
    basis: String,
}

impl Judgement {
    /// Whether it leaves the shot starting at `start` of `media` out, on
    /// whatever basis.
    pub(crate) fn drops(&self, media: MediaId, start: FrameIdx) -> bool {
        self.rows
            .get(&media)
            .is_some_and(|r| r.drop.binary_search(&start).is_ok() || r.stale.binary_search(&start).is_ok())
    }

    /// Whether it judged the shot on the basis it needs now.
    pub(crate) fn current(&self, media: MediaId, start: FrameIdx) -> bool {
        self.rows.get(&media).is_some_and(|r| {
            r.basis == self.basis && (r.drop.binary_search(&start).is_ok() || r.keep.binary_search(&start).is_ok())
        })
    }
}

/// The meta key of the row of `filter` (`<pluginId>/<filterId>`) for `c` in
/// `media`, and the prefix of all its rows for `c`.
fn row_key(filter: &str, c: CharacterId, media: MediaId) -> String {
    format!("{}{media}", rows_of(filter, c))
}

fn rows_of(filter: &str, c: CharacterId) -> String {
    format!("{VERDICTS_PREFIX}{filter}/{c}/")
}

impl Project {
    /// What the filter `filter` said of `c`'s shots, read against `basis`.
    pub(crate) fn judgement(&self, filter: &str, c: CharacterId, basis: &str) -> ProjectResult<Judgement> {
        let prefix = rows_of(filter, c);
        let mut rows = HashMap::new();
        for (key, json) in self.db().meta_range(&prefix)? {
            let media = key.strip_prefix(&prefix).and_then(|m| m.parse::<MediaId>().ok());
            match (media, Row::read(&json)) {
                (Some(media), Some(row)) => {
                    rows.insert(media, row);
                }
                _ => tracing::warn!(key, "a verdict row this AutoScene can't read; ignored"),
            }
        }
        Ok(Judgement { rows, basis: basis.to_owned() })
    }

    /// Keeps the verdicts `given` of the filter `filter` on `c`'s shots in
    /// `media` (by their first frame), judged on `basis`, with those it
    /// gave before. `false` when they were dropped: the episode is gone, or
    /// its shots are being found again.
    pub fn keep_verdicts(
        &self,
        filter: &str,
        c: CharacterId,
        media: MediaId,
        basis: &str,
        given: &[(FrameIdx, bool)],
    ) -> ProjectResult<bool> {
        let key = row_key(filter, c, media);
        let row = |now: Option<String>| {
            let row = Row::merged(now.as_deref().and_then(Row::read), basis, given);
            serde_json::to_string(&row).expect("a verdict row serialises")
        };
        Ok(self.db().update_media_meta(&key, media, row)?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::analysis::tests::project_with_media;

    fn settings(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn the_basis_ignores_settings_the_filter_doesn_t_name() {
        let names = vec!["sensitivity".to_owned()];
        let now = settings(&[("sensitivity", Value::from(0.5)), ("webhook", Value::from("https://a.example"))]);
        let b = basis("speaks", 1, &names, &now);
        assert_eq!(b.len(), 64);
        // Another webhook: the same basis.
        let other = settings(&[("sensitivity", Value::from(0.5)), ("webhook", Value::from("https://b.example"))]);
        assert_eq!(basis("speaks", 1, &names, &other), b);
        // A new threshold, another filter of the plugin: another.
        let louder = settings(&[("sensitivity", Value::from(0.7)), ("webhook", Value::from("https://a.example"))]);
        assert_ne!(basis("speaks", 1, &names, &louder), b);
        assert_ne!(basis("shouts", 1, &names, &now), b);
        // Named twice or in another order, a setting counts once, in key order.
        let both = vec!["webhook".to_owned(), "sensitivity".to_owned(), "webhook".to_owned()];
        let sorted = vec!["sensitivity".to_owned(), "webhook".to_owned()];
        assert_eq!(basis("speaks", 1, &both, &now), basis("speaks", 1, &sorted, &now));
        // A named setting without a value (a secret) is one more name, and stays so.
        let named = vec!["sensitivity".to_owned(), "token".to_owned()];
        assert_ne!(basis("speaks", 1, &named, &now), b);
        assert_eq!(basis("speaks", 1, &named, &now), basis("speaks", 1, &named, &now));
        // Naming none: no setting counts.
        assert_eq!(basis("speaks", 1, &[], &now), basis("speaks", 1, &[], &louder));
    }

    #[test]
    fn a_new_revision_asks_again() {
        let now = settings(&[("sensitivity", Value::from(0.5))]);
        let names = vec!["sensitivity".to_owned()];
        assert_ne!(basis("speaks", 1, &names, &now), basis("speaks", 2, &names, &now));
        // Verdicts of revision 1 count as not judged under revision 2, and still apply.
        let (_dir, project, media) = project_with_media(2400);
        // Verdicts are kept of shots that were found.
        project.db().store_segments(media, &[]).unwrap();
        let c = CharacterId::new();
        let (one, two) = (basis("speaks", 1, &names, &now), basis("speaks", 2, &names, &now));
        assert!(project.keep_verdicts("dev.a.speech/speaks", c, media, &one, &[(0, false), (48, true)]).unwrap());
        let j = project.judgement("dev.a.speech/speaks", c, &two).unwrap();
        assert!(j.drops(media, 0) && !j.drops(media, 48));
        assert!(!j.current(media, 0) && !j.current(media, 48));
        let j = project.judgement("dev.a.speech/speaks", c, &one).unwrap();
        assert!(j.current(media, 0) && j.current(media, 48));
    }

    #[test]
    fn verdicts_merge_and_survive_a_reopen() {
        let (_dir, project, media) = project_with_media(2400);
        // Verdicts are kept of shots that were found.
        project.db().store_segments(media, &[]).unwrap();
        let (c, filter) = (CharacterId::new(), "dev.a.speech/speaks");
        let other = CharacterId::new();
        assert!(project.keep_verdicts(filter, c, media, "b1", &[(0, false), (48, true), (50, true)]).unwrap());
        // A later run of the same basis: its word on a shot counts, the rest stay.
        assert!(project.keep_verdicts(filter, c, media, "b1", &[(48, false), (290, true), (290, false)]).unwrap());
        // Another character's verdicts are their own.
        assert!(project.keep_verdicts(filter, other, media, "b1", &[(0, true)]).unwrap());
        let row = |c| Row::read(&project.db().meta(&row_key(filter, c, media)).unwrap().unwrap()).unwrap();
        assert_eq!((row(c).drop, row(c).keep), (vec![0, 48, 290], vec![50]));
        assert_eq!((row(other).drop, row(other).keep), (vec![], vec![0]));

        // A new basis: what the old one left out stays out until judged anew.
        assert!(project.keep_verdicts(filter, c, media, "b2", &[(0, true)]).unwrap());
        let r = row(c);
        assert_eq!((r.basis.as_str(), r.drop, r.keep, r.stale), ("b2", vec![], vec![0], vec![48, 290]));
        let j = project.judgement(filter, c, "b2").unwrap();
        assert!(!j.drops(media, 0) && j.current(media, 0));
        assert!(j.drops(media, 48) && !j.current(media, 48) && !j.drops(media, 50));
        // And once judged anew, it is the new word.
        assert!(project.keep_verdicts(filter, c, media, "b2", &[(48, true), (290, false)]).unwrap());
        let r = row(c);
        assert!(!serde_json::to_string(&r).unwrap().contains("stale"), "an empty list isn't written");
        assert_eq!((r.drop, r.keep, r.stale), (vec![290], vec![0, 48], vec![]));

        // As a reopened project reads them.
        let (id, folder) = (project.id, project.dir.clone());
        drop(project);
        let project = Project::open(id, folder, Default::default()).unwrap();
        let j = project.judgement(filter, c, "b2").unwrap();
        assert!(j.drops(media, 290) && j.current(media, 0) && !j.drops(media, 48));
        // A row a later AutoScene wrote is read as none.
        project.db().set_meta(&row_key(filter, c, media), r#"{"v":2,"basis":"b2","drop":[0]}"#).unwrap();
        assert!(!project.judgement(filter, c, "b2").unwrap().drops(media, 0));
    }

    #[test]
    fn old_drops_outlast_two_settings_changes() {
        let (_dir, project, media) = project_with_media(2400);
        // Verdicts are kept of shots that were found.
        project.db().store_segments(media, &[]).unwrap();
        let (c, filter) = (CharacterId::new(), "dev.a.speech/speaks");
        assert!(project.keep_verdicts(filter, c, media, "b1", &[(48, false)]).unwrap());
        // Two new bases, each judging another shot: what b1 left out stays out, unjudged.
        assert!(project.keep_verdicts(filter, c, media, "b2", &[(0, true)]).unwrap());
        assert!(project.keep_verdicts(filter, c, media, "b3", &[(290, true)]).unwrap());
        let j = project.judgement(filter, c, "b3").unwrap();
        assert!(j.drops(media, 48) && !j.current(media, 48));
        assert!(!j.drops(media, 0) && !j.current(media, 0) && j.current(media, 290));
    }

    #[test]
    fn a_218_episode_series_keeps_every_verdict() {
        let (_dir, project, media) = project_with_media(2400);
        // Verdicts are kept of shots that were found.
        project.db().store_segments(media, &[]).unwrap();
        let (c, filter) = (CharacterId::new(), "dev.a.speech/speaks");
        // 218 episodes of a lead: about 28 shots each, a third left out.
        let shots: Vec<(FrameIdx, bool)> = (0..28).map(|i| (i * 85, i % 3 != 0)).collect();
        let others: Vec<MediaId> = (1..218).map(|_| MediaId::new()).collect();
        for m in &others {
            project
                .db()
                .set_meta(&row_key(filter, c, *m), &serde_json::to_string(&Row::merged(None, "b", &shots)).unwrap())
                .unwrap();
        }
        assert!(project.keep_verdicts(filter, c, media, "b", &shots).unwrap());
        let j = project.judgement(filter, c, "b").unwrap();
        assert_eq!(j.rows.len(), 218);
        for m in others.iter().chain([&media]) {
            assert!(shots.iter().all(|&(s, keep)| j.current(*m, s) && j.drops(*m, s) != keep));
        }
        let bytes: usize =
            project.db().meta_range(VERDICTS_PREFIX).unwrap().iter().map(|(k, v)| k.len() + v.len()).sum();
        assert!(bytes < 218 * 1024, "{bytes} bytes");
    }
}
