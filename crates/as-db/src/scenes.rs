//! The scenes pass in `project.db` (an optional mode): the pictures of every
//! shot, as the pass hands them out. Which shots make a scene is not kept:
//! it is worked out from the pictures when asked for
//! (`as_project::scenes`), since it also reads the cast, which changes.

use as_api::{StageState, TrackAssign};
use as_core::{FrameIdx, MediaId, Msg};
use rusqlite::{Transaction, params};

use crate::cast::parse_id;
use crate::project::ProjectDb;
use crate::{DbResult, enum_to_sql, json_to_sql};

/// Forget a media's pictures: they belong to shots that go.
pub(crate) fn forget_pictures(tx: &Transaction, id: MediaId) -> DbResult<()> {
    let m = id.to_string();
    tx.execute("DELETE FROM shot_pictures WHERE media_id = ?1", [&m])?;
    tx.execute("UPDATE media SET scenes = 'pending', scenes_error = NULL WHERE id = ?1", [&m])?;
    Ok(())
}

fn waiting_scenes(c: &rusqlite::Connection) -> DbResult<Vec<MediaId>> {
    let mut st = c.prepare(
        "SELECT id FROM media WHERE status = 'ready' AND shots = 'ready' AND scenes = 'pending'
         ORDER BY episode IS NULL, COALESCE(season, 0), episode, position",
    )?;
    let ids = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    ids.iter().map(|s| parse_id(s)).collect()
}

impl ProjectDb {
    pub fn set_scenes_state(&self, id: MediaId, state: StageState, error: Option<&Msg>) -> DbResult<()> {
        self.0.write(|tx| {
            tx.execute(
                "UPDATE media SET scenes = ?2, scenes_error = ?3 WHERE id = ?1",
                params![id.to_string(), enum_to_sql(state), error.map(json_to_sql)],
            )?;
            Ok(())
        })
    }

    /// Store the pictures of shots: each shot's first frame and its vectors
    /// (`as_analysis::pictures::to_bytes`). A shot stored before is replaced.
    pub fn put_shot_pictures(&self, id: MediaId, shots: &[(FrameIdx, Vec<u8>)]) -> DbResult<()> {
        self.0.write(|tx| {
            let m = id.to_string();
            let mut st = tx.prepare(
                "INSERT INTO shot_pictures (media_id, start, vectors) VALUES (?1, ?2, ?3)
                 ON CONFLICT(media_id, start) DO UPDATE SET vectors = excluded.vectors",
            )?;
            for (start, vectors) in shots {
                st.execute(params![m, start, vectors])?;
            }
            Ok(())
        })
    }

    /// The pictures of a media's shots `(first frame, vectors)`, in order.
    pub fn shot_pictures(&self, id: MediaId) -> DbResult<Vec<(FrameIdx, Vec<u8>)>> {
        self.0.read(|c| {
            let mut st = c.prepare("SELECT start, vectors FROM shot_pictures WHERE media_id = ?1 ORDER BY start")?;
            let rows = st.query_map([id.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// The first frames of the shots of a media that have their pictures, in
    /// order.
    pub fn pictured_shots(&self, id: MediaId) -> DbResult<Vec<FrameIdx>> {
        self.0.read(|c| {
            let mut st = c.prepare("SELECT start FROM shot_pictures WHERE media_id = ?1 ORDER BY start")?;
            let rows = st.query_map([id.to_string()], |r| r.get(0))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Who the cast shows in the shots of a media: the first frame of the
    /// shot and the character, for every face the cast is sure of (found by
    /// the analysis or decided by the user).
    pub fn shot_cast(&self, id: MediaId) -> DbResult<Vec<(FrameIdx, String)>> {
        self.0.read(|c| {
            let mut st = c.prepare(
                "SELECT DISTINCT shot, character_id FROM tracks
                  WHERE media_id = ?1 AND not_face = 0 AND character_id IS NOT NULL AND assign IN (?2, ?3)
                  ORDER BY shot",
            )?;
            let sure = [TrackAssign::Auto, TrackAssign::Pinned].map(enum_to_sql);
            let rows = st.query_map(params![id.to_string(), sure[0], sure[1]], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// How many characters the cast shows (hidden ones left out) are to be
    /// seen in the shots of a media, by a face the cast is sure of: the
    /// shot's first frame and their number, for the shots that have any.
    pub fn shot_shown(&self, id: MediaId) -> DbResult<Vec<(FrameIdx, u32)>> {
        self.0.read(|c| {
            let mut st = c.prepare(
                "SELECT t.shot, COUNT(DISTINCT t.character_id) FROM tracks t
                   JOIN characters ch ON ch.id = t.character_id
                  WHERE t.media_id = ?1 AND t.not_face = 0 AND t.assign IN (?2, ?3) AND ch.hidden = 0
                  GROUP BY t.shot ORDER BY t.shot",
            )?;
            let sure = [TrackAssign::Auto, TrackAssign::Pinned].map(enum_to_sql);
            let rows = st.query_map(params![id.to_string(), sure[0], sure[1]], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// The sprites of a media's shots: the shot's first frame, the shot end
    /// its sprite was made for, and its cells.
    pub fn shot_sprites(&self, id: MediaId) -> DbResult<Vec<(FrameIdx, FrameIdx, u32)>> {
        self.0.read(|c| {
            let mut st = c.prepare("SELECT shot, end, cells FROM sprites WHERE media_id = ?1 ORDER BY shot")?;
            let rows = st.query_map([id.to_string()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Media whose shots are found and whose pictures aren't read (an
    /// interrupted run counts as pending again: for a launch, when nothing
    /// runs yet), in episode order.
    pub fn pending_scenes(&self) -> DbResult<Vec<MediaId>> {
        self.0.write(|tx| {
            tx.execute("UPDATE media SET scenes = 'pending' WHERE scenes = 'running'", [])?;
            waiting_scenes(tx)
        })
    }

    /// Media whose shots are found and whose pictures wait to be read, in
    /// episode order — those being read now left as they are.
    pub fn waiting_scenes(&self) -> DbResult<Vec<MediaId>> {
        self.0.read(waiting_scenes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast::tests::ready_media;

    #[test]
    fn pictures_are_kept_per_shot_and_go_with_the_shots() {
        let db = ProjectDb::memory();
        let m = ready_media(&db, "/v/a.mkv");
        assert_eq!(db.waiting_scenes().unwrap(), [m], "its shots are found, its pictures not read");
        db.put_shot_pictures(m, &[(0, vec![1; 8]), (240, vec![2; 8])]).unwrap();
        db.put_shot_pictures(m, &[(240, vec![3; 8]), (500, vec![4; 8])]).unwrap();
        assert_eq!(db.pictured_shots(m).unwrap(), [0, 240, 500]);
        assert_eq!(db.shot_pictures(m).unwrap()[1], (240, vec![3; 8]), "a shot read again is replaced");

        db.set_scenes_state(m, StageState::Running, None).unwrap();
        assert!(db.waiting_scenes().unwrap().is_empty());
        assert_eq!(db.pending_scenes().unwrap(), [m], "a run cut short waits again at the next launch");
        db.set_scenes_state(m, StageState::Ready, None).unwrap();
        assert_eq!(db.media(m).unwrap().expect("the media").scenes, StageState::Ready);
        assert!(db.pending_scenes().unwrap().is_empty());

        // Its shots found again: the pictures belonged to the old ones.
        db.reset_shots(m).unwrap();
        assert!(db.pictured_shots(m).unwrap().is_empty());
        assert_eq!(db.media(m).unwrap().expect("the media").scenes, StageState::Pending);
    }

    #[test]
    fn a_project_of_schema_16_gains_the_scenes_columns() {
        let db = ProjectDb::memory_at(16);
        db.0.conn
            .lock()
            .execute_batch("INSERT INTO media (id, path, file_name, size_bytes, position, status, added_at) VALUES ('m', '/v/a.mkv', 'a.mkv', 1, 0, 'ready', 0)")
            .unwrap_or_else(|e| panic!("a media row of schema 16: {e}"));
        db.migrate();
        let state: String =
            db.0.conn.lock().query_row("SELECT scenes FROM media WHERE id = 'm'", [], |r| r.get(0)).unwrap();
        assert_eq!(state, "pending");
        let rows: i64 = db.0.conn.lock().query_row("SELECT COUNT(*) FROM shot_pictures", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0);
    }
}
