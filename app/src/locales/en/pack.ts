// The pack namespace: a character's scenepack (page, filters, shots, footer) and the dialog that exports it.
// The words the pack's export dialog shares with the clip export dialog (Format, Destination, Change…) are
// `export:…`. English source, typed: `{{variables}}` are checked. Comments are notes for the translator.
export default {
  /** A count of shots wherever the pack says one ("129 shots"). */
  shots_one: "{{count}} shot",
  shots_other: "{{count}} shots",
  /** A count of the project's episodes ("3 episodes"). */
  episodes_one: "{{count}} episode",
  episodes_other: "{{count}} episodes",

  /** Why a shot is left out: a short badge on its card, and the end of its screen reader name. */
  reason: {
    missing: "File missing",
    episode: "Other episode",
    tiny: "Flash cut",
    short: "Too short",
    long: "Too long",
    size: "Other size",
    notSolo: "Not alone",
    faceless: "No face",
    limit: "Past the limit",
    removed: "Taken out",
    /** A plugin's filter left the shot out and gave no words of its own. */
    plugin: "Plugin filter",
    /** A shot is out for a reason the card has no word for. */
    other: "Left out",
  },

  /** Shot sizes, as film people call them. `label` is the filter's and the tooltip's name; `short` is the badge on a card. */
  size: {
    extremeCloseUp: { label: "Extreme close-up", short: "ECU" },
    closeUp: { label: "Close-up", short: "CU" },
    mediumCloseUp: { label: "Medium close-up", short: "MCU" },
    medium: { label: "Medium shot", short: "MS" },
    wide: { label: "Wide shot", short: "WS" },
  },

  /** The shot lengths the Filters menu keeps. `min` and `max` are seconds, a tenth at most ("1.5"). */
  length: {
    any: "Any length",
    between: "{{min}} to {{max}} s",
    orLonger: "{{min}} s or longer",
    upTo: "Up to {{max}} s",
  },

  /** What a plugin's check says while it judges the pack's shots, and when it couldn't. */
  check: {
    /**
     * "Checking 3 of 10 shots". `total` is written as it is (no thousands separator); `count` only picks the
     * plural form, and is the same number.
     */
    some_one: "Checking {{checked}} of {{total}} shot",
    some_other: "Checking {{checked}} of {{total}} shots",
    /** The same before the first shot is judged: "Checking 10 shots". */
    all_one: "Checking {{total}} shot",
    all_other: "Checking {{total}} shots",
    /** The check failed for some shots. */
    failed_one: "Couldn't check {{count}} shot",
    failed_other: "Couldn't check {{count}} shots",
  },

  /** The heading of the group of a plugin's "Save as…" entries: it says what the group waits for. */
  save: {
    heading: "Save with a plugin",
    waitsForCheck: "Save with a plugin waits for the check",
    needsShot: "Save with a plugin needs a shot in the pack",
    waitsForPack: "Save with a plugin waits for the pack",
  },

  /** The name parts a file or chapter name can hold: the button's label and its tooltip. The part itself (`{series}`) is never translated. */
  namePart: {
    series: { label: "Series", hint: "The project's title" },
    character: { label: "Character", hint: "The character's name" },
    episode: { label: "Episode", hint: "The episode, as S01E03, or its file's name" },
    index: { label: "Number", hint: "The shot's place in the pack: 001, 002, …" },
    tc: { label: "Timecode", hint: "Where the shot starts, as 00-12-03-05" },
    frame: { label: "Frame", hint: "The frame the shot starts on, as 17357" },
  },

  /** One shot of the pack: its tooltip, screen reader name and menu. */
  card: {
    /** A badge on a shot someone added to the pack by hand, against the filters. */
    added: "Added",
    /** The tooltip's lines, joined with " · ". */
    hint: {
      faceless: "their face doesn't show",
      /** `title` is a plugin filter's title, which AutoScene doesn't translate. */
      wouldLeaveOut: "“{{title}}” would leave it out",
      takeOut: "Click to leave it out",
      takeIn: "Click to take it in",
    },
    /** A plugin filter has not judged every shot with its settings as they are, so what it said may be from before. */
    before: {
      /** It has judged none of them. */
      sure: "“{{title}}” said so with its settings before",
      maybe: "“{{title}}” may have said so with its settings before",
    },
    /**
     * A screen reader's name for the shot, made of facts joined with ", ": where it is, how long, without their
     * face, then in the pack or left out and why. `at` is a timecode, with the episode's name before it when known.
     */
    aria: {
      shot: "Shot",
      shotAt: "Shot at {{at}}",
      withoutFace: "without their face",
      inPack: "in the pack",
      leftOut: "left out: {{why}}",
    },
    /** The button over the shot that opens it in the player. */
    watchAria: "Watch the shot",
    watchAriaAt: "Watch the shot at {{at}}",
    /** The right-click menu. */
    watch: "Watch shot",
    remove: "Remove from pack",
    add: "Add to pack",
    handBack: "Hand back to the filters",
    /** The bodies in this shot are not the character's. `name` is their name. */
    notThem: "Not {{name}}",
    /** The same where the card doesn't know whose pack it is. */
    notThemAnon: "Not them",
    notPerson: "Not a person",
    copyTimecode: "Copy timecode",
  },

  /** The bar under the top bar: the order, the filters and the handles. */
  filters: {
    /** The screen reader name of the whole bar, and the button's label. */
    title: "Filters",
    /** The button's screen reader name when some filters are on. */
    titleOn_one: "Filters, {{count}} on",
    titleOn_other: "Filters, {{count}} on",
    order: {
      aria: "Order",
      episodes: "Episode order",
      best: "Best first",
    },
    /** A screen reader name for the shot sizes in the menu (no heading shows). */
    sizes: "Shot sizes",
    /** The menu's own lists: the name, and what is chosen beside it. */
    length: "Length",
    episodes: "Episodes",
    limit: "Limit",
    /** What is chosen beside "Episodes": "3 of 12". */
    episodesOf: "{{n}} of {{total}}",
    /** Beside "Episodes" when every episode is in. */
    all: "All",
    allEpisodes: "All episodes",
    noLimit: "No limit",
    /** Beside "Limit": the pack keeps the best 50 shots. */
    best: "Best {{n}}",
    /** In the Limit list. */
    theBest: "The best {{n}}",
    solo: "Only them in the shot",
    skipTiny: "Skip flash cuts",
    withFace: "Only with their face",
    /** A line under "Only with their face". */
    withFaceHint: "Leaves out the shots found by their body",
    noHandles: "No handles",
    /** The Handles button once some are on; `n` frames ("fr") on each side of the shot. */
    handles: "Handles {{n}} fr",
    /** In the Handles list; `count` is 6, 12 or 24. */
    framesAround_one: "{{count}} frame before and after",
    framesAround_other: "{{count}} frames before and after",
    reset: "Reset filters",
    /** A plugin's filter as a chip. `plugin`, `title` and `what` are the plugin's own words. */
    plugin: {
      /** On the chip of a filter no plugin has now. */
      goneTitle: "Removed filter",
      gone: "This filter's plugin is gone, or no longer has it. Turn this filter off.",
      off: "Plugin “{{plugin}}” is off. Turn it on in Plugins, or turn this filter off.",
      says: "Plugin “{{plugin}}”: {{what}}",
      /** On the chip while its plugin is off. */
      pluginOff: "Plugin off",
      checkingAria: "Checking {{title}}",
      failedAria: "Couldn't check {{title}}",
    },
  },

  /** The glass bar at the bottom: how many shots, how long, how big, and Export. */
  footer: {
    /** After the length, " · ": the size an export would have. `format` is the format's short name. */
    about: "about {{size}}",
    aboutIn: "about {{size}} ({{format}})",
    /** The tooltip of "Couldn't check n shots". */
    failedHint:
      "Until they are checked, they keep what the filter said with its settings before, or stay in when it said nothing. Export takes them as the grid shows them.",
    picksByHand_one: "{{count}} pick by hand",
    picksByHand_other: "{{count}} picks by hand",
    handBackAll: "Hand every shot back to the filters",
    options: "Pack options",
    showExport: "Show it in Activity",
    exportQueued: "Export queued",
    /** `percent` is written ("40%"). The tag makes the number stand out. */
    exporting: "Exporting… <x>{{percent}}</x>",
    waitsForCheck: "Waits for the check",
    exportWaitsAria: "Export, waits for the check",
    exportTooltip: "Export the pack",
    export: "Export",
  },

  /** The scenepack page. */
  page: {
    /** The top bar's title and the window's. `name` is the character's. */
    title: "{{name}} · Scenepack",
    titleBare: "Scenepack",
    /** The back button before the character is known. */
    characterFallback: "Character",
    /** The small heading over the character's name. */
    eyebrow: "Scenepack",
    /** Under the name. `shots` and `episodes` are counts already written ("12 shots", "3 episodes"). */
    header: "{{shots}} in {{episodes}} · the pack takes the bright ones",
    gone: {
      title: "This character isn't in the cast any more",
      description: "It may have been merged into someone else.",
      back: "Back to the cast",
    },
    show: {
      aria: "Show",
      all: "All {{n}}",
      in: "In the pack {{n}}",
      out: "Left out {{n}}",
    },
    hint: "Click to take a shot in or out · Shift for every shot since · Enter to watch",
    loadFailed: "The pack couldn't be made",
    reanalysing: "Their episode is being analysed again; the shots come back when it's done.",
    noShots: "No shots of them yet.",
    takesNone: "The pack takes none of the shots. Loosen a filter or click shots to take them.",
    takesAll: "The pack takes every shot.",
    /** The menu of the page's empty space, and the ⋯ beside Export. */
    menu: {
      export: "Export the pack…",
      takeAll: "Take every shot",
      leaveAll: "Leave every shot out",
      invert: "Invert the pack",
      handBackAll: "Hand every shot back to the filters",
      show: "Show",
      showAll: "All shots",
      showIn: "In the pack",
      showOut: "Left out",
    },
  },

  /** Toasts of the page and its hooks. */
  toast: {
    tookAll: "Took every shot in",
    leftAll: "Left every shot out",
    inverted: "Inverted the pack",
    handedBack_one: "Handed {{count}} shot back to the filters",
    handedBack_other: "Handed {{count}} shots back to the filters",
    /** Under the toast, when a limit is on and the shots are picked by hand. */
    limitLeavesAlone: "Picked by hand: the limit leaves them alone.",
    /** The bodies in a shot were told not to be this character. `name` is theirs. */
    notThem: "Not {{name}}: out of the pack",
    notThemAnon: "Not them: out of the pack",
    notPerson: "Not a person: out of the pack",
    /** A plugin filter's check couldn't run; the reason follows under it. */
    checkFailed: "Couldn't check the shots",
    /** A click on the pack couldn't be saved; the reason follows under it. */
    changeFailed: "Couldn't change the pack",
  },

  /** The dialog that exports the pack. Words it shares with the clip export are in `export`. */
  export: {
    /** `name` is the character's; the English possessive needs another form in some languages. */
    title: "Export {{name}}'s scenepack",
    /** Under the title when the pack goes out as one video; `count` is the number of shots. */
    descriptionOneFile_one: "{{count}} shot in one video, each cut frame-exact and checked against the source.",
    descriptionOneFile_other: "{{count}} shots in one video, each cut frame-exact and checked against the source.",
    /** The same when every shot is a file of its own. */
    description_one: "{{count}} shot, each cut frame-exact and checked against the source.",
    description_other: "{{count}} shots, each cut frame-exact and checked against the source.",
    /**
     * What an export started now misses: episodes (or files, for a film) still being analysed, whose shots of
     * the character aren't found yet. `count` is how many are being analysed, `name` the character.
     */
    analysing: {
      episode_one: "{{count}} episode is still being analysed. Shots of {{name}} found in it later won't be in this export.",
      episode_other: "{{count}} episodes are still being analysed. Shots of {{name}} found in them later won't be in this export.",
      file_one: "{{count}} file is still being analysed. Shots of {{name}} found in it later won't be in this export.",
      file_other: "{{count}} files are still being analysed. Shots of {{name}} found in them later won't be in this export.",
    },
    oneFile: {
      label: "All shots in one file",
      description: "One video with a chapter for each shot, in the pack's order.",
    },
    /** The label of the field that names the one file (when all shots go in one file). */
    fileName: "File name",
    chapterNames: "Chapter names",
    /** The label of the field that names a file for each shot. */
    fileNames: "File names",
    /** A screen reader's name for the row of buttons that put a name part in the field. */
    nameParts: "Name parts",
    /** After the format's size, when the shots are rendered first. */
    freeSpace: "The shots are rendered first, so it needs extra free space for a while.",
    /** The switch under the destination; `name` is the character's. */
    folder: "In a folder named after {{name}}",
    /** The submit button; `count` is the number of shots. */
    submit_one: "Export {{count}} shot",
    submit_other: "Export {{count}} shots",
    submitOneFile_one: "Export {{count}} shot into one file",
    submitOneFile_other: "Export {{count}} shots into one file",
    /** The toast that says the export is on its way. `name` is the character's. */
    started_one: "Exporting {{count}} shot of {{name}}",
    started_other: "Exporting {{count}} shots of {{name}}",
    startedOneFile_one: "Exporting {{count}} shot of {{name}} into one file",
    startedOneFile_other: "Exporting {{count}} shots of {{name}} into one file",
    /** Under the first file names, when there are more: "and 12 more". */
    more_one: "and {{count}} more",
    more_other: "and {{count}} more",
  },
} as const
