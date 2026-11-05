// The shell namespace: the rail, the command palette, the app's menus, Activity, the update dialog, the window's
// caption and title, and the toasts of the app-wide actions and events (docs/DESIGN.md, Copy).
// English source, typed: `{{variables}}` are checked. Comments are notes for translators.
export default {
  /** The names of the app's main screens: rail tiles and their tooltips, menu entries. */
  place: {
    library: "Library",
    settings: "Settings",
    plugins: "Plugins",
    activity: "Activity",
  },
  /** Entries of the window's right-click menu and of the rail's menus: imperative verbs and screen names. */
  menu: {
    back: "Back",
    copy: "Copy",
    /** Opens the command palette. The ellipsis says it asks for more. */
    commandPalette: "Command palette…",
    newProject: "New project…",
    showRail: "Show the rail",
    hideRail: "Hide the rail",
    /** Opens Settings at the keyboard shortcuts. */
    shortcuts: "Keyboard shortcuts",
    reloadWindow: "Reload window",
    checkUpdates: "Check for updates",
    about: "About AutoScene",
  },

  /** The left-hand rail. */
  rail: {
    /** Screen-reader name of the rail's navigation. */
    label: "Main",
    /** The AutoScene mark at the top, a link to the library. `AutoScene` is the product name. */
    mark: "AutoScene library",
    newProject: "New project",
    /** Rail button that opens the command palette. */
    search: "Search",
    /** Menu of the Activity button, when the Activity drawer is open. */
    hideActivity: "Hide activity",
    showActivity: "Show activity",
    /** Menu of the Settings tile: the sections it jumps to. */
    settings: {
      general: "General",
      shortcuts: "Shortcuts",
      updates: "Updates",
    },
    /** Menu of a plugin page's tile. */
    page: {
      open: "Open",
      /** The plugin's own settings. */
      settings: "Its settings",
      log: "Show its log",
    },
    /** A project tile's tooltip, second line. `size` is "Movie" or "3 episodes". */
    indexing: "{{size}} · indexing",
  },

  /** What a project is made of: the rail tile's tooltip and the command palette's hint. */
  project: {
    movie: "Movie",
    /** `n` is the number written as it is ("1204", no thousands separator); `count` only picks the form. */
    episodes_one: "{{n}} episode",
    episodes_other: "{{n}} episodes",
  },
  /** A project's one-line facts under its title: kind, footage type (`footage`, already translated) and size. */
  projectFacts: {
    movie_one: "Movie · {{footage}} · {{n}} file",
    movie_other: "Movie · {{footage}} · {{n}} files",
    series_one: "Series · {{footage}} · {{n}} episode",
    series_other: "Series · {{footage}} · {{n}} episodes",
  },

  /** The command palette (Ctrl/⌘+K). A command is also found by its English name. */
  palette: {
    /** Screen-reader title of the dialog. */
    title: "Command palette",
    /** Placeholder of the search field while a project is open. */
    searchInProject: "Search characters, episodes, projects and commands",
    search: "Search projects, places and commands",
    /** Shown when the search finds nothing. */
    empty: "Nothing matches.",
    /** Group headings. */
    group: {
      projects: "Projects",
      plugins: "Plugins",
      actions: "Actions",
      goTo: "Go to",
      characters: "Characters",
      episodes: "Episodes",
    },
    /** Places inside the open project. */
    here: {
      overview: "Overview",
      /** A movie's list of video files. */
      files: "Files",
      episodes: "Episodes",
      cast: "Cast",
      exports: "Exports",
    },
    /** Places anywhere in the app. */
    go: {
      library: "Library",
      settings: "Settings",
      shortcuts: "Keyboard shortcuts",
      plugins: "Plugins",
      browsePlugins: "Browse plugins",
      about: "About AutoScene",
    },
    /** Commands. */
    action: {
      newProject: "New project…",
      activity: "Show activity",
      rail: "Show or hide the rail",
      updates: "Check for updates",
      pluginUpdates: "Check for plugin updates",
      logs: "Open logs folder",
    },
    /** A plugin's page; `title` is the plugin's own page title. */
    openPage: "Open {{title}}",
    /** A character without a name; `number` is the one AutoScene counted. */
    unnamed: "Character {{number}}",
    /** Extra word the palette finds an unnamed character by. */
    unnamedWord: "unnamed",
    /** What a character shows beside its name: `time` ("12 min") on screen and its shots. */
    character_one: "{{time}} · {{count}} shot",
    character_other: "{{time}} · {{count}} shots",
    /** An episode whose file can't be found. */
    fileMissing: "File missing",
    /** Extra words an episode is found by ("Episode 3"). */
    episodeWord: "Episode {{number}}",
  },

  /** The right-click menu of a text field. */
  textMenu: {
    /** Screen-reader name of the menu. */
    label: "Edit",
    undo: "Undo",
    redo: "Redo",
    cut: "Cut",
    copy: "Copy",
    paste: "Paste",
    delete: "Delete",
    selectAll: "Select all",
  },

  /** The window's caption buttons (Windows) and the menu of its title bar. */
  window: {
    minimize: "Minimize",
    maximize: "Maximize",
    restore: "Restore",
  },
  /** The window's title, assembled from the screen's names, most specific first: "Cast · Walter — Dark — AutoScene". */
  crumb: {
    plugins: "Plugins",
    cast: "Cast",
    review: "Cast · Review",
    exports: "Exports",
    /** `section` is the name of the Settings section. */
    settings: "Settings · {{section}}",
  },

  /** The Activity drawer: background work. */
  activity: {
    title: "Activity",
    clearFinished: "Clear finished",
    clearFailed: "Couldn't clear the list",
    empty: {
      title: "Nothing running",
      description: "Indexing, previews and exports show up here with live progress while they run.",
    },
    group: {
      inProgress: "In progress",
      finished: "Finished",
    },
    /** Names of how a finished job ended (screen readers). */
    done: "Done",
    failed: "Failed",
    cancelled: "Cancelled",
    /** A job's second line while it waits or has no stage yet. */
    waiting: "Waiting",
    working: "Working",
    /** Button and aria-label that stop one job; `title` is the job's own title. Plugin runs are stopped, other work cancelled. */
    stopJob: "Stop {{title}}",
    cancelJob: "Cancel {{title}}",
    /** Toast title when stopping a job fails. */
    stopFailed: "Couldn't stop the job",
    cancelFailed: "Couldn't cancel the job",
    /** Menu entries of a job. */
    showExports: "Show the exports",
    openPlayer: "Open in the player",
    openCast: "Open the cast",
    openProject: "Open the project",
    copyError: "Copy the error",
    copyMessage: "Copy the message",
    /** What the Activity button says to screen readers: `running` and `waiting` counts, joined by a comma. */
    nothingRunning: "Nothing running",
    running_one: "{{n}} running",
    running_other: "{{n}} running",
    waitingCount_one: "{{n}} waiting",
    waitingCount_other: "{{n}} waiting",
  },

  /** Cancelling work that belongs to one episode (menu and buttons). */
  jobs: {
    cancelIndexing: "Cancel indexing",
    cancelAnalysis: "Cancel analysis",
    cancelPreview: "Cancel preview",
    /** The sentence a removal dialog ends with; `list` names what stops, in a list ("indexing, analysing and an export"). */
    stopsFirst: "Work running on it stops first: {{list}}.",
    /** Items of that list: what runs, named by what it does. */
    stops: {
      indexing: "indexing",
      analysing: "analysing",
      preview_one: "building a preview",
      preview_other: "building previews",
      export_one: "an export",
      export_other: "{{n}} exports",
      pluginRun_one: "a plugin run",
      pluginRun_other: "{{n}} plugin runs",
    },
  },

  /** Copying text to the clipboard: what a toast says, per kind of text. */
  copy: {
    /** Toast title after a copy; the text itself is the description. */
    copied: {
      timecode: "Copied timecode",
      timecodes: "Copied timecodes",
      filePath: "Copied file path",
      folderPath: "Copied folder path",
      path: "Copied path",
      error: "Copied error",
      message: "Copied message",
      pluginId: "Copied plugin id",
    },
    /** Toast title when the copy fails. */
    failed: {
      timecode: "Couldn't copy the timecode",
      timecodes: "Couldn't copy the timecodes",
      filePath: "Couldn't copy the file path",
      folderPath: "Couldn't copy the folder path",
      path: "Couldn't copy the path",
      error: "Couldn't copy the error",
      message: "Couldn't copy the message",
      pluginId: "Couldn't copy the plugin id",
      other: "Couldn't copy",
    },
  },
  /** Showing a file in Explorer or Finder; opening a link in the browser. */
  os: {
    revealFailed: "Couldn't show it",
    linkFailed: "Couldn't open link",
  },

  /** Adding video files to a project (a drop, a picker, the menu). */
  files: {
    /** `n` is the number written as it is; `count` only picks the form. */
    added_one: "Added {{n}} file",
    added_other: "Added {{n}} files",
    nothingNew: "Nothing new to add",
    /** Toast of the New Project dialog's picker when every file was there already. */
    alreadyIn: "Those files are already in this project",
    addFailed: "Couldn't add files",
  },

  /** App-wide actions and the events the backend announces. */
  events: {
    logsFailed: "Couldn't open logs",
    /** Toast actions. */
    showInFolder: "Show in folder",
    show: "Show",
    /** AutoScene's own words: the plugin is named in its own words inside quotes; `name` is the plugin's. */
    withdrew: "AutoScene withdrew Plugin “{{name}}”",
    nvidiaFailed: "NVIDIA acceleration couldn't be downloaded",
    nvidiaReady: "NVIDIA acceleration is ready",
    exportDone: "Export finished",
    exportErrors: "Export finished with errors",
    /** A plugin's job failed or saved a pack: the words after the plugin's name (the toast starts "Plugin “name”: "). `title` is the plugin's own. */
    pluginFailed: "{{title}} failed",
    /** `file` is the saved file's name. */
    pluginSaved: "saved {{file}}",
  },

  /** The update dialog and the update checks that announce themselves. */
  update: {
    /** `version` is AutoScene's new version. */
    available: "AutoScene {{version}} is available",
    /** Badge on a pretend update (development). */
    simulated: "Simulated",
    /** `current` is the version in use. */
    youHave: "You have {{current}}",
    /** `date` is when the new version came out. */
    youHaveDated: "You have {{current}} · released {{date}}",
    /** Download progress: both are sizes ("1.5 MB"). */
    downloading: "{{done}} of {{total}}",
    installing: "Installing…",
    ready: "Installed. Restart to finish.",
    restart: "Restart now",
    skip: "Skip this version",
    later: "Remind me later",
    updating: "Updating…",
    install: "Update now",
    upToDate: "AutoScene is up to date",
    /** Description of the "up to date" toast. */
    version: "Version {{version}}",
    checkFailed: "Couldn't check for updates",
  },

  /** Files dragged over the window. */
  drop: {
    /** Toasts when files or plugin packages are dropped. */
    finishReview: "Finish the plugin review first.",
    packagesNotFiles: "Plugin packages are no project files. Close this window and drop them on their own.",
    packagesAlone: "Drop plugin packages on their own.",
    /** The overlay: a heading and a line under it. */
    mixed: {
      heading: "Drop plugin packages on their own",
      hint: "Packages and other files together are not taken",
    },
    plugin: {
      heading: "Review a plugin",
      /** `.asp` is the file extension of a plugin package. */
      hint: "Drop its .asp package",
    },
    addHere: "Add to this project",
    create: "Create a project",
    media: "Drop a season folder or video files",
  },
  /** The plugin install review's own wait. */
  pluginReview: {
    tookTooLong: "Reading it took too long. Is the file on a drive that is not connected?",
    /** Toast title when a plugin package can't be read; `label` names the package (a file name or the plugin's own title). */
    openFailed: "Couldn't open {{label}}",
  },

  /** The notice, at the start, that new projects use Fast Scan on this computer. */
  fastScan: {
    title: "Fast Scan is on",
    never: "Don't show this again",
    turnOff: "Turn off Fast Scan",
    continue: "Continue",
    /** Toast after Turn off, with its description and an Undo. */
    off: "Fast Scan is off",
    offDescription: "New projects start with the full scan.",
    offFailed: "Couldn't turn off Fast Scan",
    onFailed: "Couldn't turn Fast Scan back on",
  },

  /** A route that doesn't exist, and a view that failed. */
  notFound: {
    title: "Nothing here",
    description: "This page doesn't exist anymore. The project or episode may have been removed.",
    back: "Back to Library",
  },
  routeError: {
    title: "Something went wrong here",
  },
} as const
