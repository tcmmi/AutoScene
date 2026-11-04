//! An AutoScene plugin that runs in AutoScene's sandbox. AutoScene calls
//! `run` once for each run of one of its actions, and `on_event` for each of
//! its events that the person turned on (`plugin/plugin.json`).

wit_bindgen::generate!({ world: "plugin", path: "wit" });

use autoscene::plugin::host::{self, LogLevel};

struct Plugin;

impl Guest for Plugin {
    fn run(request: RunRequest) -> Result<Option<String>, RunError> {
        match request.action.id.as_str() {
            "hello" => {
                host::log(LogLevel::Info, "Saying hello.");
                Ok(Some(format!("Hello from {}.", request.plugin.id)))
            }
            other => Err(RunError {
                message: format!("{other} isn't an action of this plugin."),
                detail: None,
            }),
        }
    }

    fn on_event(request: EventRequest) -> Result<Option<String>, RunError> {
        Err(RunError {
            message: format!("{} isn't an event of this plugin.", request.id),
            detail: None,
        })
    }
}

export!(Plugin);
