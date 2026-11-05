// The shortcuts namespace: the name of every shortcut and of each of its keys, the groups Settings → Shortcuts
// lists them in, the editors' presets, key names and the reasons a key is refused. English source, typed:
// `{{variables}}` are checked. Comments are notes for translators.
//
// A shortcut is `<scope>.<id>`: `.name` is what it does, as a row of Settings → Shortcuts and a tooltip read it.
// A shortcut with several keys also names each one, `.slots.<n>` (n from 0, in the order of its keys).
// Keep the names short: they sit beside the key caps.
export default {
  /** Where a shortcut works: the heading and the line under it in Settings → Shortcuts. */
  group: {
    global: {
      title: "Everywhere",
      description: "Work faster without leaving the keyboard.",
    },
    player: {
      title: "Player",
      description: "While an episode is open.",
    },
    review: {
      title: "Review faces",
      description: "Answer with one key; every answer is kept.",
    },
    cast: {
      title: "Cast",
      /** `mod` is the Ctrl key (⌘ on a Mac); "Shift" is the key's cap. */
      description: "On the Cast page. {{mod}}-click selects a character too, Shift-click every one between.",
    },
    faces: {
      title: "A character's faces",
      description: "In a character's Faces view.",
    },
    shots: {
      title: "A character's shots",
      description: "In a character's Shots view.",
    },
    pack: {
      title: "Scenepack",
      description: "While picking a character's shots.",
    },
  },

  global: {
    palette: { name: "Command palette" },
    activity: { name: "Show activity" },
    sidebar: { name: "Show or hide the rail" },
    settings: { name: "Settings" },
    library: { name: "Library" },
    newProject: { name: "New project" },
  },

  player: {
    playPause: { name: "Play / pause" },
    shuttle: {
      /** Three keys in one row: J, K and L, in the order of the slots. */
      name: "Backwards / stop / forwards (again: faster; stop held: a frame)",
      slots: { "0": "Backwards", "1": "Stop", "2": "Forwards" },
    },
    frame: {
      name: "Previous / next frame",
      slots: { "0": "Previous frame", "1": "Next frame" },
    },
    second: {
      name: "Back / forward one second",
      slots: { "0": "Back one second", "1": "Forward one second" },
    },
    shot: {
      name: "Previous / next shot",
      slots: { "0": "Previous shot", "1": "Next shot" },
    },
    ends: {
      name: "First / last frame",
      slots: { "0": "First frame", "1": "Last frame" },
    },
    mark: {
      /** "In" and "Out" are the two ends of a clip being marked on the timeline. */
      name: "Mark in / out",
      slots: { "0": "Mark in", "1": "Mark out" },
    },
    goToMark: {
      name: "Go to in / out",
      slots: { "0": "Go to in", "1": "Go to out" },
    },
    markShot: { name: "Mark the shot as the clip" },
    clearMarks: { name: "Clear in and out" },
    goTo: { name: "Go to a timecode" },
    mute: { name: "Mute" },
    zoomIn: { name: "Zoom the timeline in" },
    zoomOut: { name: "Zoom the timeline out" },
    zoomFit: { name: "The whole episode on the timeline" },
    export: { name: "Export the marked clip" },
  },

  review: {
    yes: { name: "Yes, it's them" },
    no: { name: "No, someone else" },
    notFace: { name: "Not a face" },
    skip: { name: "Skip for now" },
    /** The keys 1 to 9 each pick the character in that place on screen. */
    pick: { name: "It's character 1 to 9" },
  },

  faces: {
    selectAll: { name: "Select every face" },
    clearSelection: { name: "Clear the selection" },
  },

  shots: {
    toggle: { name: "Show or hide the shots of a stack" },
    watch: { name: "Watch from the stack's first shot" },
    close: { name: "Close the open stack" },
  },

  cast: {
    selectAll: { name: "Select every character" },
    clearSelection: { name: "Clear the selection" },
  },

  pack: {
    toggle: { name: "Take the shot in or out (Shift: every shot since the last)" },
    watch: { name: "Watch the shot" },
    takeAll: { name: "Take every shot (when all are in: leave them out)" },
    leaveAll: { name: "Leave every shot out" },
    invert: { name: "Invert the pack: what's in goes out, what's out comes in" },
    export: { name: "Export the pack" },
  },

  /** Keys for editors who know another program's. The names of the programs are not translated. */
  preset: {
    autoscene: { description: "The keys AutoScene comes with." },
    premiere: {
      /** `mod` is the Ctrl key; "Shift" and "X" are key caps. The symbols are the keys themselves. */
      description: "Export with {{mod}} M, zoom with = − \\, clear in and out with {{mod}} Shift X.",
      /** The same preset on a Mac: only the zoom keys differ. */
      descriptionMac: "Zoom with = − \\, the rest like AutoScene.",
    },
    resolve: {
      /** `mod` is the Ctrl key (⌘ on a Mac). */
      description: "Zoom with {{mod}} = and {{mod}} −.",
    },
  },

  /** Names of keys, as a key cap shows them. Arrows and the minus sign are drawn as symbols and are not here. */
  key: {
    space: "Space",
    esc: "Esc",
    enter: "Enter",
    backspace: "Backspace",
    del: "Del",
    ins: "Ins",
    pageUp: "Page Up",
    pageDown: "Page Down",
  },

  /** A shortcut's name in quotation marks, as a sentence quotes it. */
  quote: "“{{name}}”",

  /** Why a key can't be used, shown under a key being recorded. `caps` is the key as caps ("Ctrl K"). */
  verdict: {
    editing: "{{caps}} stays with editing text",
    menuBar: "{{caps}} belongs to the menu bar",
    closesWindow: "{{caps}} closes the window",
    /** `mod` is the Ctrl key (⌘ on a Mac). */
    needsMod: "Keys that work everywhere need {{mod}}",
    caret: "{{caps}} moves the caret in text",
    /** `name` is a shortcut whose keys can't be changed. */
    fixed: "{{caps}} is “{{name}}”",
    /** Toast title: the keys could not be saved. */
    keepFailed: "Couldn't keep the keys",
  },
} as const
