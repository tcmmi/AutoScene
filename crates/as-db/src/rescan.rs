//! What Rescan found. Suggestions only: nothing here touches a track or a
//! character until the person answers.

use as_core::CharacterId;
use rusqlite::{OptionalExtension, params};

use crate::cast::{bump_rev, parse_id};
use crate::project::ProjectDb;
use crate::{DbResult, enum_from_sql, enum_to_sql};

pub use as_api::RescanVerdict;

/// A face Rescan has something to say about.
#[derive(Clone, Debug, PartialEq)]
pub struct RescanFaceRow {
    pub track_id: i64,
    pub verdict: RescanVerdict,
    pub target: Option<CharacterId>,
    pub other: Option<CharacterId>,
    pub score: f32,
    pub score2: f32,
    pub looks: u32,
    /// How clear its second reading is, and what the reading said (`None`:
    /// its stored face was clear enough to go by).
    pub read_quality: Option<f32>,
    pub emb: Option<Vec<u8>>,
}

fn opt_id(s: Option<String>) -> DbResult<Option<CharacterId>> {
    s.as_deref().map(parse_id).transpose()
}

fn ordered(a: CharacterId, b: CharacterId) -> (String, String) {
    let (a, b) = (a.to_string(), b.to_string());
    if a < b { (a, b) } else { (b, a) }
}

impl ProjectDb {
    /// Keep what a rescan of `subjects` found. What was said of them before
    /// goes; a track or character gone meanwhile is left out.
    pub fn put_rescan_faces(&self, subjects: &[i64], rows: &[RescanFaceRow]) -> DbResult<()> {
        self.0.write(|tx| {
            let mut drop = tx.prepare("DELETE FROM rescan_faces WHERE track_id = ?1")?;
            for id in subjects {
                drop.execute([id])?;
            }
            let mut put = tx.prepare(
                "INSERT INTO rescan_faces (track_id, verdict, target, other, score, score2, looks, read_quality, emb)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
                  WHERE EXISTS (SELECT 1 FROM tracks WHERE id = ?1)
                    AND (?3 IS NULL OR EXISTS (SELECT 1 FROM characters WHERE id = ?3))
                    AND (?4 IS NULL OR EXISTS (SELECT 1 FROM characters WHERE id = ?4))",
            )?;
            for r in rows {
                put.execute(params![
                    r.track_id,
                    enum_to_sql(r.verdict),
                    r.target.map(|c| c.to_string()),
                    r.other.map(|c| c.to_string()),
                    r.score as f64,
                    r.score2 as f64,
                    r.looks,
                    r.read_quality.map(|q| q as f64),
                    r.emb,
                ])?;
            }
            Ok(())
        })
    }

    /// What Rescan said and the person hasn't answered, and that still
    /// applies: the face isn't confirmed, isn't where it was sent, may go there.
    pub fn rescan_faces(&self) -> DbResult<Vec<RescanFaceRow>> {
        self.0.read(|c| {
            let mut st = c.prepare(
                "SELECT r.track_id, r.verdict, r.target, r.other, r.score, r.score2, r.looks, r.read_quality, r.emb
                   FROM rescan_faces r JOIN tracks t ON t.id = r.track_id
                  WHERE r.dismissed = 0 AND t.not_face = 0
                    AND t.assign <> 'pinned'
                    AND NOT (r.verdict = 'between' AND r.other IS NULL)
                    AND NOT (r.verdict IN ('elsewhere', 'newly') AND t.assign = 'auto' AND t.character_id = r.target)
                    AND NOT EXISTS (SELECT 1 FROM track_forbids f WHERE f.track_id = r.track_id AND f.character_id = r.target)
                  ORDER BY r.target, r.score DESC, r.track_id",
            )?;
            let mut rows = st.query([])?;
            let mut out = Vec::new();
            while let Some(r) = rows.next()? {
                out.push(RescanFaceRow {
                    track_id: r.get(0)?,
                    verdict: enum_from_sql(&r.get::<_, String>(1)?)?,
                    target: opt_id(r.get(2)?)?,
                    other: opt_id(r.get(3)?)?,
                    score: r.get::<_, f64>(4)? as f32,
                    score2: r.get::<_, f64>(5)? as f32,
                    looks: r.get(6)?,
                    read_quality: r.get::<_, Option<f64>>(7)?.map(|q| q as f32),
                    emb: r.get(8)?,
                });
            }
            Ok(out)
        })
    }

    /// The person left these faces as they are: they aren't suggested again
    /// until they are rescanned. No forbid: it would take them out of what
    /// defines their character.
    pub fn dismiss_rescan_faces(&self, ids: &[i64]) -> DbResult<()> {
        self.0.write(|tx| {
            let mut st = tx.prepare("UPDATE rescan_faces SET dismissed = 1 WHERE track_id = ?1")?;
            for id in ids {
                st.execute([id])?;
            }
            Ok(())
        })
    }

    /// Everything still suggested goes.
    pub fn dismiss_all_rescan(&self) -> DbResult<()> {
        self.0.write(|tx| {
            tx.execute("UPDATE rescan_faces SET dismissed = 1", [])?;
            tx.execute("DELETE FROM rescan_pairs", [])?;
            Ok(())
        })
    }

    /// The person accepted these faces: their sharper reading becomes the
    /// face the cast goes by, and the suggestion has been answered.
    pub fn adopt_reads(&self, ids: &[i64]) -> DbResult<()> {
        self.0.write(|tx| {
            let mut adopt = tx.prepare(
                "UPDATE tracks SET
                    emb = COALESCE((SELECT emb FROM rescan_faces r WHERE r.track_id = tracks.id), emb),
                    quality = MAX(quality, COALESCE((SELECT read_quality FROM rescan_faces r WHERE r.track_id = tracks.id), 0))
                  WHERE id = ?1",
            )?;
            let mut done = tx.prepare("DELETE FROM rescan_faces WHERE track_id = ?1")?;
            for id in ids {
                adopt.execute([id])?;
                done.execute([id])?;
            }
            bump_rev(tx)
        })
    }

    /// The person said these two are two people: they aren't asked about again.
    pub fn set_character_apart(&self, a: CharacterId, b: CharacterId) -> DbResult<()> {
        let (a, b) = ordered(a, b);
        self.0.write(|tx| {
            tx.execute("INSERT OR IGNORE INTO character_apart (a, b) VALUES (?1, ?2)", [&a, &b])?;
            tx.execute("DELETE FROM rescan_pairs WHERE a = ?1 AND b = ?2", [&a, &b])?;
            Ok(())
        })
    }

    pub fn characters_apart(&self, a: CharacterId, b: CharacterId) -> DbResult<bool> {
        let (a, b) = ordered(a, b);
        self.0.read(|c| {
            let hit = c
                .query_row("SELECT 1 FROM character_apart WHERE a = ?1 AND b = ?2", [&a, &b], |_| Ok(()))
                .optional()?;
            Ok(hit.is_some())
        })
    }

    /// The question about these two goes (it may be asked anew).
    pub fn drop_rescan_pair(&self, a: CharacterId, b: CharacterId) -> DbResult<()> {
        let (a, b) = ordered(a, b);
        self.0.write(|tx| {
            tx.execute("DELETE FROM rescan_pairs WHERE a = ?1 AND b = ?2", [&a, &b])?;
            Ok(())
        })
    }

    /// Keep a "same person?" question.
    pub fn put_rescan_pair(
        &self,
        a: CharacterId,
        b: CharacterId,
        main: f32,
        second: Option<f32>,
        together: u32,
        verdict: as_api::RescanLikeness,
    ) -> DbResult<()> {
        let (x, y) = ordered(a, b);
        self.0.write(|tx| {
            tx.execute(
                "INSERT OR REPLACE INTO rescan_pairs (a, b, main_sim, second_sim, together, verdict)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6
                  WHERE EXISTS (SELECT 1 FROM characters WHERE id = ?1)
                    AND EXISTS (SELECT 1 FROM characters WHERE id = ?2)",
                params![x, y, main as f64, second.map(|s| s as f64), together, enum_to_sql(verdict)],
            )?;
            Ok(())
        })
    }

    /// The "same person?" questions waiting.
    #[allow(clippy::type_complexity)]
    pub fn rescan_pairs(
        &self,
    ) -> DbResult<Vec<(CharacterId, CharacterId, f32, Option<f32>, u32, as_api::RescanLikeness)>> {
        self.0.read(|c| {
            let mut st = c.prepare(
                "SELECT a, b, main_sim, second_sim, together, verdict FROM rescan_pairs ORDER BY main_sim DESC",
            )?;
            let mut rows = st.query([])?;
            let mut out = Vec::new();
            while let Some(r) = rows.next()? {
                out.push((
                    parse_id(&r.get::<_, String>(0)?)?,
                    parse_id(&r.get::<_, String>(1)?)?,
                    r.get::<_, f64>(2)? as f32,
                    r.get::<_, Option<f64>>(3)?.map(|s| s as f32),
                    r.get(4)?,
                    enum_from_sql(&r.get::<_, String>(5)?)?,
                ));
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (CharacterId, CharacterId) {
        (CharacterId::new(), CharacterId::new())
    }

    #[test]
    fn apart_doesnt_depend_on_the_order_asked() {
        let (a, b) = pair();
        assert_eq!(ordered(a, b), ordered(b, a));
    }
}
