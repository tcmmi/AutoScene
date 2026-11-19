//! The few fields of the engine's protocol (`as_engine::protocol`) the cloud
//! reads: which job a request is, the flags it is priced by, where its file
//! is, and how a job ends. Everything else passes through as it came.

use serde_json::Value;

use crate::FacesFlags;

/// The engine jobs a cloud session runs. Stills (`embed`) and rescans stay
/// on the computer; `shutdown` is the worker's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Shots,
    Faces,
    Scenes,
    Cancel,
}

/// What an engine request is, if a cloud session runs it.
pub fn request_op(v: &Value) -> Option<Op> {
    match v.get("op")?.as_str()? {
        "shots" => Some(Op::Shots),
        "faces" => Some(Op::Faces),
        "scenes" => Some(Op::Scenes),
        "cancel" => Some(Op::Cancel),
        _ => None,
    }
}

/// The job id a request or a reply names.
pub fn id(v: &Value) -> Option<u64> {
    v.get("id")?.as_u64()
}

/// The flags of a faces request (missing ones are off, as the engine reads
/// them).
pub fn faces_flags(v: &Value) -> FacesFlags {
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    FacesFlags {
        anime: flag("anime"),
        fast: flag("fast"),
        best: flag("best"),
        animated: flag("animated"),
        bodies: flag("bodies"),
        known: v.get("known").is_some_and(|k| !k.is_null()),
    }
}

/// Point a job at `path`, the worker's copy of the file. False for a request
/// that names no file (`cancel`).
pub fn set_path(v: &mut Value, path: &str) -> bool {
    match v.get_mut("path") {
        Some(p) => {
            *p = Value::String(path.to_string());
            true
        }
        None => false,
    }
}

/// How a reply line bears on a job.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Progress {
        id: u64,
        fraction: f64,
    },
    Done {
        id: u64,
    },
    Failed {
        id: u64,
        cancelled: bool,
    },
    /// The engine can't work at all and exits.
    Fatal,
    /// Anything else: devices, chunks, pictures.
    Other,
}

/// What a reply line says, for pricing; `None` for a line that isn't JSON.
pub fn event(line: &str) -> Option<Event> {
    let v: Value = serde_json::from_str(line).ok()?;
    let id = id(&v);
    Some(match (v.get("event").and_then(Value::as_str), id) {
        (Some("progress"), Some(id)) => {
            Event::Progress { id, fraction: v.get("fraction").and_then(Value::as_f64).unwrap_or(0.0) }
        }
        (Some("done"), Some(id)) => Event::Done { id },
        (Some("failed"), Some(id)) => {
            Event::Failed { id, cancelled: v.get("cancelled").and_then(Value::as_bool).unwrap_or(false) }
        }
        (Some("fatal"), _) => Event::Fatal,
        _ => Event::Other,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn requests_are_read_by_their_op_and_flags() {
        let mut faces = json!({"op": "faces", "id": 4, "path": "D:/show/01.mkv", "anime": true, "known": null});
        assert_eq!(request_op(&faces), Some(Op::Faces));
        assert_eq!(id(&faces), Some(4));
        assert_eq!(faces_flags(&faces), FacesFlags { anime: true, ..Default::default() });
        assert!(set_path(&mut faces, "/work/in.mkv"));
        assert_eq!(faces["path"], "/work/in.mkv");
        let known = json!({"op": "faces", "id": 5, "known": "AQI=", "bodies": true});
        assert!(faces_flags(&known).known && faces_flags(&known).bodies);
        assert_eq!(request_op(&json!({"op": "embed", "id": 1})), None);
        assert_eq!(request_op(&json!({"op": "shutdown"})), None);
        assert!(!set_path(&mut json!({"op": "cancel", "id": 4}), "x"));
    }

    #[test]
    fn replies_are_read_for_how_jobs_end() {
        assert_eq!(
            event(r#"{"event":"progress","id":3,"fraction":0.5}"#),
            Some(Event::Progress { id: 3, fraction: 0.5 })
        );
        assert_eq!(event(r#"{"event":"done","id":3}"#), Some(Event::Done { id: 3 }));
        assert_eq!(
            event(r#"{"event":"failed","id":3,"message":"x","detail":"y","cancelled":true}"#),
            Some(Event::Failed { id: 3, cancelled: true })
        );
        assert_eq!(event(r#"{"event":"fatal","message":"x","detail":"y"}"#), Some(Event::Fatal));
        assert_eq!(event(r#"{"event":"face_chunk","id":3,"start":0,"end":9,"tracks":""}"#), Some(Event::Other));
        assert_eq!(event("not json"), None);
    }
}
