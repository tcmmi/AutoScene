// The player namespace: the player page, its transport, timeline and details panel (docs/DESIGN.md, Copy).
// English source, typed: `{{variables}}` are checked. Comments are notes for translators.
export default {
  /** A number of frames: a clip, a shot, an episode. `{{count}}` is grouped by the language ("172,799"). */
  frames_one: "{{count}} frame",
  frames_other: "{{count}} frames",
  /** A level as a percentage, with a space before the sign: the volume, how far into the episode. `{{value}}` is already written ("12.3", "40"). */
  percent: "{{value}} %",
  /** A character nobody has named, as the timeline and the cast list show them. `{{number}}` is their number, not a count: no grouping. */
  character: {
    unnamed: "Character {{number}}",
  },

  /** The page: the bar above the picture and its menu. */
  page: {
    /** The bar's back link to a project of episodes, and to one of films. */
    episodes: "Episodes",
    files: "Files",
    /** Tooltip of the "Preview" badge: the picture is a smaller copy made for playback. */
    previewTip: "Playing a preview copy; exports always cut from the original file.",
    preview: "Preview",
    /** Name of the button that opens the panel beside the picture, and of the panel itself. */
    details: "Details",
    hideDetails: "Hide details",
    showDetails: "Show details",
    /** Buttons that move to the episode before and after this one, and the same with its title in the tooltip. */
    previousEpisode: "Previous episode",
    nextEpisode: "Next episode",
    previousEpisodeTitled: "Previous episode: {{title}}",
    nextEpisodeTitled: "Next episode: {{title}}",
  },

  /** What stands in the picture's place while the episode can't play yet. */
  notReady: {
    loading: "Loading…",
    /** Title and reason: the project's episodes couldn't be loaded. The reason comes from the app. */
    listFailed: "This episode couldn't be loaded",
    goneTitle: "This episode isn't in the project any more",
    goneBody: "It may have been removed from the project.",
    indexingTitle: "Indexing",
    indexingBody: "Every frame is being indexed so cuts land exactly. Playback starts when it's done.",
    indexFailed: "This file couldn't be indexed",
    missingTitle: "File not found",
    /** `{{path}}` is the file's path as the person has it. */
    missingBody: "{{path}} isn't there any more. Moved it? Show AutoScene where it is: its analysis and your decisions stay.",
    /** A preview copy is a lightweight file made so every frame can be reached exactly. */
    previewing: "Preparing preview",
    previewFailed: "Preview failed",
    previewMissing: "Preview not prepared",
    previewBody: "This format needs a lightweight preview copy for frame-accurate playback. Exports still cut from the original.",
    cantPlay: "Can't play this file",
    preparePreview: "Prepare preview",
    locate: "Locate file…",
    backToEpisodes: "Back to the episodes",
    /**
     * How many frames indexing has read. `<x>` is the number read, `<mono>` the file's own count, both in
     * the timecode font; `{{done}}` is written in the language's digits. The plural follows `{{count}}`, the file's count.
     */
    framesOfAbout_one: "<x>{{done}}</x> of about <mono>{{count}}</mono> frame",
    framesOfAbout_other: "<x>{{done}}</x> of about <mono>{{count}}</mono> frames",
    /** Frames read, when the file's count is unknown or lower than what was read. The plural follows `{{count}}`, the frames read. */
    framesRead_one: "<x>{{count}}</x> frame",
    framesRead_other: "<x>{{count}}</x> frames",
    measuring: "Measuring speed…",
    /** The speed indexing reads at: frames a second, then (when the file's frame rate is known) how many times real time. `<mono>` is a number in the timecode font. */
    pace: "<mono>{{pace}}</mono> fps",
    paceReal: "<mono>{{pace}}</mono> fps · <mono>{{speed}}×</mono> real time",
  },

  /** The row of controls under the picture. */
  transport: {
    firstFrame: "First frame",
    previousFrame: "Previous frame",
    nextFrame: "Next frame",
    lastFrame: "Last frame",
    play: "Play",
    pause: "Pause",
    mute: "Mute",
    unmute: "Unmute",
    volume: "Volume",
    /** Tooltip of the clip's length (the shortest of the row's labels); its hint is the length in frames. */
    clipLength: "Length of the clip",
    /** Button and tooltip: removes both marks of the clip. */
    clearMarks: "Clear in and out",
    exportClip: "Export clip",
    /** Label of the Export button on a wide row. */
    export: "Export",
    /** Tooltip hint of Export while nothing is marked. `{{keys}}` is the key that marks the shot, as written on the keyboard. */
    exportHint: "Mark an in or out point, or a whole shot",
    exportHintKeys: "Mark an in or out point, or a whole shot ({{keys}})",
    /** The chips that mark where a clip starts and ends: "In" and "Out" before they are set. */
    in: "In",
    out: "Out",
    markIn: "Mark in",
    markOut: "Mark out",
    /** Once a mark is set the chip goes to it; the second form names where, for a screen reader. `{{time}}` is a timecode. */
    goToIn: "Go to the in point",
    goToOut: "Go to the out point",
    goToInAt: "Go to the in point, {{time}}",
    goToOutAt: "Go to the out point, {{time}}",
  },

  /** The timecode of the frame on screen, the go-to field and the frame counter. */
  readout: {
    /** The four things the readout can show, as menu items. Capitalised words in English. */
    elapsed: "Elapsed Timecode",
    remaining: "Remaining Timecode",
    frameNumber: "Frame Number",
    percentage: "Percentage",
    /** What the readout is called while it shows the frame number. */
    currentFrame: "Current Frame",
    /** The readout's name for a screen reader: `{{label}}` is one of the names above, `{{value}}` what it shows ("00:12:03:05"). */
    aria: "{{label}}: {{value}}",
    /** Name of the field that takes a timecode or a frame number, and the tooltip and menu item that open it. */
    goTo: "Go to timecode or frame",
    goToMenu: "Go to timecode or frame…",
    goToHint: "Right-click to show the time left, frames or percent",
    counterTip: "This frame / the last frame",
    /** The unit after the frame counter in percentage mode: "1,200 / 34,000 f". */
    frameUnit: "f",
    /** A timecode that can't be read. */
    badTimecode: "That isn't a timecode or frame number",
    badTimecodeTry: "Try 00:12:03:05, a frame like 17099, or +48 to jump ahead.",
  },

  /** What the clip's marks say when they reach the export dialog. */
  clip: {
    /** One line about the clip: `{{start}}` and `{{end}}` are timecodes, `{{duration}}` a time ("0:04"), `{{length}}` the frames. */
    summary_one: "{{start}} – {{end}} · {{duration}} ({{length}} frame)",
    summary_other: "{{start}} – {{end}} · {{duration}} ({{length}} frames)",
    /** Shown in the export dialog when only one mark is set. `{{keys}}` is the key that sets the other mark. */
    onlyIn: "Only an in point is marked, so the clip runs to the end of the episode. To end it sooner, close this and press {{keys}} where it should stop.",
    onlyInNoKey: "Only an in point is marked, so the clip runs to the end of the episode. To end it sooner, close this and mark the out point where it should stop.",
    onlyOut: "Only an out point is marked, so the clip starts at the beginning of the episode. To start it later, close this and press {{keys}} where it should begin.",
    onlyOutNoKey: "Only an out point is marked, so the clip starts at the beginning of the episode. To start it later, close this and mark the in point where it should begin.",
  },

  /** The right-click menu over the picture and the transport (more of its items are in `transport`, `page`, `shot` and `file`). */
  menu: {
    exportClip: "Export clip…",
    copyInOut: "Copy in and out",
    copyTimecode: "Copy timecode",
    goToTimecode: "Go to a timecode…",
    playerKeys: "Player keys…",
  },

  /** The timeline under the transport. */
  timeline: {
    zoomIn: "Zoom in",
    zoomOut: "Zoom out",
    /** Zooms out until the whole episode shows. */
    wholeEpisode: "The whole episode",
    /** The row of shots, and the name of the group of characters' rows. */
    shots: "Shots",
    characters: "Characters on screen",
    /** Name of the thin bar that shows which part of the episode the timeline shows while zoomed in. */
    inView: "Part of the episode in view",
    playhead: "Playhead",
    goHere: "Go here",
    markInHere: "Mark in here",
    markOutHere: "Mark out here",
    copyTimecode: "Copy this timecode",
    /** The shot's length in the tooltip over it. `{{start}}` is a timecode, `{{seconds}}` the length in seconds ("2.5"). */
    shotDetail: "{{start}} · {{seconds}} s",
    shotDetailTiny: "{{start}} · {{seconds}} s · tiny",
    /** A character's row in the menu and the tooltip: `<b>` their name, `<x>` where they appear (a timecode); the second form says that only their body shows. */
    appearance: "<b>{{name}}</b> · <x>{{start}}</x>",
    appearanceNoFace: "<b>{{name}}</b> · <x>{{start}}</x> · no face",
    goToAppearance: "Go to this appearance",
    markAppearance: "Mark it as the clip",
    nextAppearance: "Next appearance",
    stopHighlighting: "Stop highlighting",
    highlight: "Highlight",
    openInCast: "Open in the cast",
  },

  /** The kinds of stretch the shot detection finds. "Shot" is the ordinary one; the others are transitions between shots. */
  segment: {
    shot: "Shot",
    dissolve: "Dissolve",
    fadeOut: "Fade out",
    fadeIn: "Fade in",
    black: "Black",
    flash: "Flash",
  },

  /** A shot: its name on the timeline, in the details panel and in menus. */
  shot: {
    /** The shot's place among the episode's shots, with and without how many there are. */
    number: "Shot {{number}}",
    of: "Shot {{number}} of {{total}}",
    /** The shot's place among the shots when it is very short. */
    ofTiny: "Shot {{number}} of {{total}} · tiny",
    /** How long the shot is. `{{length}}` is the number of frames, written plain: a shot's length is never grouped ("1200"). */
    length_one: "{{length}} frame",
    length_other: "{{length}} frames",
    mark: "Mark shot",
    markAsClip: "Mark the shot as the clip",
    previous: "Previous shot",
    next: "Next shot",
    goTo: "Go to the shot",
    /** Menu item on a stretch that isn't a shot (a transition, a black frame). */
    goThere: "Go there",
    copyTimecodes: "Copy its timecodes",
  },

  /** The panel beside the picture. */
  details: {
    /** The two tabs. */
    cast: "Cast",
    file: "File",
    /** Name of the switch between the tabs. */
    sections: "Details sections",
  },

  /** The Cast tab: where the analysis stands, and who is in the shot. */
  castTab: {
    /** Where the shot and character finding stands, when it isn't done. The reasons come from the app. */
    shotsFailed: "Shot detection failed.",
    shotsStopped: "Finding shots was stopped.",
    shotsMissing: "Shots haven't been detected yet.",
    facesFailed: "Finding characters failed.",
    facesStopped: "Finding characters was stopped.",
    facesMissing: "Characters haven't been found in this episode yet.",
    /** The one button under it, and the same in the tab's menu. */
    findAll: "Find shots and characters",
    findCharacters: "Find characters",
    continue: "Continue",
    shotsAgain: "Find shots again",
    shotsContinue: "Continue finding shots",
    facesAgain: "Find characters again",
    facesContinue: "Continue finding characters",
    /** The stage shown while the analysis runs and the app hasn't named one. */
    findingShots: "Finding shots",
    findingCharacters: "Finding characters",
    startFailed: "Couldn't start the analysis",
    nobody: "Nobody the cast knows appears here.",
    /** Beside a character who is in the shot under the playhead. */
    inShot: "in shot",
    /** Tooltip hint on a character in the list. */
    listHint: "Click to highlight · Double-click for the next appearance",
    showFewer: "Show fewer",
    /** Menu item and list row that show the characters the list left out. */
    showMore_one: "Show {{count}} more",
    showMore_other: "Show {{count}} more",
    moreInEpisode_one: "{{count}} more in this episode",
    moreInEpisode_other: "{{count}} more in this episode",
    openCast: "Open the cast",
  },

  /** The File tab. */
  file: {
    showInFolder: "Show in folder",
    copyPath: "Copy file path",
    audioTrack: "Audio track",
    audioFailed: "Couldn't change the audio track",
    /** The picture: size, frame rate and codec. `{{fps}}` is written in the language's digits. */
    video: "{{width}}×{{height}} · {{fps}} fps · {{codec}}",
    /** The episode's length: `{{time}}` is a timecode, `{{count}}` the frames. */
    length_one: "Length {{time}} · {{count}} frame",
    length_other: "Length {{time}} · {{count}} frames",
  },

  /** The player's keys dialog. */
  keys: {
    title: "Player keys",
    /** A key the person took away. */
    noKey: "No key",
    change: "Change keys…",
  },
} as const
