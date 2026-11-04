// Words every part of the app shares, and the templates behind lib/i18n.ts's `fmt`.
// Owned by phase 0: sweep groups add a shared word to their own namespace instead (docs/DESIGN.md, Copy).
// Comments are notes for translators.
export default {
  /** Buttons and menu items: imperative verbs. */
  action: {
    cancel: "Cancel",
    /** Stops a plugin's run (a job of AutoScene's own is cancelled). */
    stop: "Stop",
    close: "Close",
    undo: "Undo",
    tryAgain: "Try again",
    retry: "Retry",
  },
  /** What a job or a control is doing right now. */
  state: {
    waiting: "Waiting…",
    working: "Working…",
    done: "Done",
  },
  format: {
    /** A file size: `value` is already written in the language's digits ("1.5", "1,5"). Units stay as AutoScene writes them. */
    bytes: {
      b: "{{value}} B",
      kb: "{{value}} KB",
      mb: "{{value}} MB",
      gb: "{{value}} GB",
      tb: "{{value}} TB",
    },
    /** Time left on a progress line, lower case: it follows a stage ("Finding shots · about 3 min left"). */
    eta: {
      almostDone: "almost done",
      seconds: "{{s}} s left",
      minutes: "about {{m}} min left",
      /** `h` may have one decimal: "about 1.5 h left". */
      hours: "about {{h}} h left",
    },
    /** A whole percentage, `value` from 0 to 100. */
    percent: "{{value}}%",
    /** How long a character is on screen: "42 s", "12 min", "1 h 05 min" (`mm` always two digits). */
    screenTime: {
      seconds: "{{s}} s",
      minutes: "{{m}} min",
      hours: "{{h}} h {{mm}} min",
    },
  },
} as const
