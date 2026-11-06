// The project namespace: a project's overview, its episodes, their state, relinking, and its exports page
// (docs/DESIGN.md, Copy). English source, typed: `{{variables}}` are checked. Comments are notes for translators.
//
// "Episode" is a series' file; a movie's is a "file". The words that differ for a movie have a key each.
export default {
  /** The project's own top bar: sections of one project. */
  nav: {
    /** Accessible name of the row of sections. */
    label: "Project",
    overview: "Overview",
    /** A series' episodes. */
    episodes: "Episodes",
    /** A movie's files. */
    files: "Files",
    cast: "Cast",
    exports: "Exports",
    /** Opens a menu of the pages plugins add to a project. */
    more: "More",
  },
  /** Menu entries that more than one screen of a project has. `…` means a dialog or another step follows. */
  menu: {
    /** Choose video files to put in the project. */
    addFiles: "Add files…",
    /** Choose a folder of videos to put in the project. */
    addFolder: "Add folder…",
    editDetails: "Edit details…",
    /** Opens the folder of the project's data in the system's file manager. */
    showProjectFolder: "Show project folder",
    /** Opens a file's folder in the system's file manager. */
    showInFolder: "Show in folder",
    /** Opens a folder in the system's file manager. */
    showFolder: "Show folder",
    copyFilePath: "Copy file path",
    copyFolderPath: "Copy folder path",
    copyError: "Copy the error",
  },
  /** The button and menu entries that add videos. A button has no `…`, a menu entry has. */
  add: {
    files: "Add files",
    folder: "Add folder",
  },
  /** What the analysis of a whole project is doing right now (a band under the title). */
  analysis: {
    /** The analysis of an episode waits behind another. */
    waiting: "Waiting to analyse",
    /** What it is doing when it hasn't said more. */
    running: "Analysing",
    /** `count` is how many more episodes are waiting behind the one that runs. */
    moreQueued_one: "{{count}} more episode queued",
    moreQueued_other: "{{count}} more episodes queued",
  },
  /** The page a project opens on. */
  overview: {
    /** The white button under the title (a step follows). */
    buildPack: "Build a scenepack",
    /** The same, as an entry of a menu: a step follows. */
    buildPackMenu: "Build a scenepack…",
    /** A series: the folder button. */
    addEpisodes: "Add episodes",
    addEpisodesMenu: "Add episodes…",
    /** A movie: the folder button. */
    addFolder: "Add a folder",
    addFolderMenu: "Add a folder…",
    /** The tooltip and name of the ⋯ button next to them. */
    more: "More",
    /** After the series' facts: how many characters the cast has ("12 characters"). */
    characters_one: "{{count}} character",
    characters_other: "{{count}} characters",
    /** The shelf of characters. */
    cast: "Cast",
    /** A link above the cast shelf: how many faces wait for a decision. */
    facesToReview_one: "{{count}} face to review",
    facesToReview_other: "{{count}} faces to review",
    allCharacters: "All characters",
    /** A series' shelf of episodes. */
    episodes: "Episodes",
    /** A movie's shelf of files. */
    files: "Files",
    /** A link above the episodes: the page that lists them with their details. */
    details: "Details",
    /** The cast shelf while it is empty and the project has no episodes. */
    castEmptyNoEpisodes: "Add episodes, and the characters appear here as they are analysed.",
    /** The cast shelf while it is empty and the first episode is not analysed yet. */
    castEmpty: "Characters appear here as soon as the first episode's faces are analysed.",
    /** The empty shelf of a series: a dashed button. */
    addSeasonFolder: "Add a season folder",
    /** The empty shelf of a movie: a dashed button. */
    addMovieFolder: "Add a folder",
    /** The window that asks whose scenepack to build. */
    whoseTitle: "Whose scenepack?",
    whoseDescription: "Their shots are picked in the next step.",
  },
  /** An episode as a still (the overview's shelf). */
  episode: {
    /** On the still: the file can't be found. */
    missing: "Missing",
    /** On the still: indexing or analysis failed. */
    failed: "Failed",
    /** Under the title: how many shots were found. Both forms read the same in English. */
    shots_one: "{{count}} shots",
    shots_other: "{{count}} shots",
    /** Under the title, after the shots: how many faces were found. */
    faces_one: "{{count}} faces",
    faces_other: "{{count}} faces",
    /** Under the title of an episode that has no shots yet. */
    notAnalysed: "Not analysed yet",
    /** The accessible name of the progress ring when the job gave no words. */
    working: "Working",
  },
  /** Names the files of a project that can't be found. */
  missing: {
    /** A banner: `count` episodes of a series. */
    episodes_one: "{{count}} episode can't be found",
    episodes_other: "{{count}} episodes can't be found",
    /** A banner: `count` files of a movie. */
    files_one: "{{count}} file can't be found",
    files_other: "{{count}} files can't be found",
    /** The banner's second line, for `count` episodes or files. English says it one way; a language that agrees with the number has its forms. */
    description_one: "Moved, renamed or on a drive that isn't connected? Their analysis and your decisions wait for them.",
    description_other: "Moved, renamed or on a drive that isn't connected? Their analysis and your decisions wait for them.",
    /** Asks for the folder they are in now. */
    locateFolder: "Locate folder…",
  },
  /** A project view whose data couldn't be loaded because the project is gone. */
  loadError: {
    goneTitle: "This project isn't here any more",
    backToLibrary: "Back to Library",
  },
  /** Names of the views for the window title. */
  crumbs: {
    /** The project, before its name is known. */
    project: "Project",
    /** The episode, before its name is known. */
    episode: "Episode",
    /** The character, before its name is known. */
    character: "Character",
    /** `name` is the character's name or number: "Character 12 · Scenepack". */
    pack: "{{name}} · Scenepack",
    /** The cast's unplaced faces. */
    everyoneElse: "Cast · Everyone else",
    /** `name` is the character's name or number: "Cast · Character 12". */
    cast: "Cast · {{name}}",
  },
  /** The episodes page. */
  episodes: {
    /** A series' page title. */
    title: "Episodes",
    /** A movie's page title. */
    titleFiles: "Files",
    emptyTitle: "No videos yet",
    emptyDescription: "Drop a season folder or video files anywhere in this window, or add them here.",
    /** The name of an episode's row, which opens it. `name` is its label and title ("E01 Pilot"). */
    open: "Open {{name}}",
    /** The name of the ⋯ button of a row. `name` is the file's name. */
    options: "{{name}} options",
    /** The name of the audio track picker. */
    audioTrack: "Audio track",
    /** The file has no sound. */
    noAudio: "No audio",
    audioFailed: "Couldn't change the audio track",
  },
  /** What an episode offers wherever it is shown: its row, its card, their menus. */
  actions: {
    open: "Open",
    /** Asks for the file when it can't be found. */
    locateFile: "Locate file…",
    /** Indexes a file that failed again. */
    indexAgain: "Index again",
    findShotsAgain: "Find shots again",
    findCharactersAgain: "Find characters again",
    removeFromProject: "Remove from project…",
    retryIndexFailed: "Couldn't index it again",
    analysisFailed: "Couldn't start the analysis",
  },
  /** Where an episode stands (the episodes page, its right-hand cell). */
  status: {
    /** Over the cell: the file isn't where it was. */
    fileMissing: "File missing",
    /** Tooltip of "Failed" when indexing gave no words. */
    indexingFailed: "Indexing failed",
    failed: "Failed",
    /** What is being prepared while a preview is made. */
    preparingPreview: "Preparing preview",
    /** While analysis runs and gave no words. */
    analysing: "Analysing",
    /** While indexing runs and gave no words. */
    indexing: "Indexing",
    queued: "Queued",
    previewQueued: "Preview queued",
    analysisQueued: "Analysis queued",
    analysisFailed: "Analysis failed",
    /** Tooltip of "Analysis failed" when finding shots gave no words. */
    shotsFailed: "Shot detection failed",
    stopped: "Stopped",
    indexed: "Indexed",
    /** A button that goes on with a stopped analysis. */
    continue: "Continue",
    /** A button that starts finding the shots. */
    findShots: "Find shots",
    /** `count` shots found; the number is set in the timecode face. */
    shots_one: "<mono>{{count}}</mono> shots",
    shots_other: "<mono>{{count}}</mono> shots",
    /** `count` faces found; the number is set in the timecode face. */
    faces_one: "<mono>{{count}}</mono> faces",
    faces_other: "<mono>{{count}}</mono> faces",
    charactersFailed: "Characters failed",
    /** Tooltip of "Characters failed" when finding them gave no words. */
    charactersFailedWhy: "Finding characters failed",
    /** A button that goes on with the stopped search for characters. */
    stoppedContinue: "Stopped · continue",
    findCharacters: "Find characters",
    peopleFailed: "People failed",
    /** Tooltip of "People failed" when finding them gave no words. */
    peopleFailedWhy: "Finding people failed",
    /** A button that goes on with the stopped search for people (bodies). */
    peopleStoppedContinue: "People stopped · continue",
    findPeople: "Find people",
    retryFailed: "Couldn't retry",
  },
  /** Asks before a file takes the place of a missing episode, and says what was found. */
  relink: {
    /** The title of the dialog. `file` is the file's name, `episode` the episode's label and title. */
    questionTitle: "Use “{{file}}” for {{episode}}?",
    /** `reason` is a whole sentence saying how the file differs ("It's 40:00 long instead of 52:11."). */
    questionDescription:
      "It doesn't look like the same episode. {{reason}} If you use it anyway, it's analysed again, and your decisions go back onto the faces that look like theirs.",
    useAnyway: "Use it anyway",
    /** The title of the file dialog. `title` is the episode's label and title. */
    whereIs: "Where is {{title}} now?",
    /** The title of the folder dialog: a series. */
    whereAreEpisodes: "Where are the episodes now?",
    /** The title of the folder dialog: a movie. */
    whereAreFiles: "Where are the files now?",
    couldntUse: "Couldn't use that file",
    couldntLook: "Couldn't look in that folder",
    /** A toast: the file was taken. `title` is the episode's label and title. */
    using: "Using it for {{title}}",
    usingDescription: "It's analysed again; your decisions go back onto the faces that look like theirs.",
    /** A toast: the file is the same as before. */
    found: "Found {{title}}",
    /** A toast: the file is another copy. */
    foundAgain: "Found {{title}} — indexing it again",
    foundAgainDescription: "Its analysis stays if its frames are the same.",
    /**
     * What looking in a folder gave, by the project's kind: `episodes` or `files`. relink.ts picks the keys
     * with a template (`relink.folder.${kind}.foundAll`), so a key scan doesn't see them: keep both kinds'
     * key sets equal (language.test.tsx checks it).
     */
    folder: {
      episodes: {
        /** Nothing found. */
        noneTitle: "None of them are in that folder",
        noneDescription: "Pick the folder the episodes are in now.",
        /** Every missing one was found; `count` of them. `_one` is the plural category, not the number 1: it says "the episode", so a language whose `one` also covers 21 needs its own wording. */
        foundAll_one: "Found the episode",
        foundAll_other: "Found all {{count}} episodes",
        /** Some were found: `found` of the `count` that were missing. */
        foundSome_one: "Found {{found, number}} of {{count}} episodes",
        foundSome_other: "Found {{found, number}} of {{count}} episodes",
        /** After some were found: `count` are still missing. */
        stillMissing_one: "{{count}} episode still can't be found.",
        stillMissing_other: "{{count}} episodes still can't be found.",
      },
      files: {
        noneTitle: "None of them are in that folder",
        noneDescription: "Pick the folder the files are in now.",
        foundAll_one: "Found the file",
        foundAll_other: "Found all {{count}} files",
        foundSome_one: "Found {{found, number}} of {{count}} files",
        foundSome_other: "Found {{found, number}} of {{count}} files",
        stillMissing_one: "{{count}} file still can't be found.",
        stillMissing_other: "{{count}} files still can't be found.",
      },
      /** `count` of the found ones were another copy of the file. */
      copies_one: "{{count}} is another copy, indexed again.",
      copies_other: "{{count}} are other copies, indexed again.",
    },
  },
  /** Asks before a file leaves the project. */
  remove: {
    /** `name` is the episode's label and title. */
    title: "Remove “{{name}}”?",
    /** The file was analysed: what is lost. */
    descriptionFound:
      "Its shots and faces go, and with them your decisions about those faces. Characters you haven't named who appear only here leave the cast. The video file itself isn't touched.",
    /** Nothing has been found in the file yet. */
    descriptionNothing: "Nothing has been found in it yet. The video file itself isn't touched.",
    answer: "Remove from project",
    failed: "Couldn't remove it",
  },
  /** The exports page. */
  exports: {
    title: "Exports",
    subtitle: "Every clip is cut frame-exact and checked against the source, frame by frame.",
    emptyTitle: "No exports yet",
    emptyDescription: "Open an episode, mark in and out, and export the clip. Finished exports are listed here.",
    showFolderFailed: "Couldn't show it",
    /** Entry of the page's right-click menu. */
    showExportsFolder: "Show the exports folder",
    /** The name of the ⋯ button of an export. `name` is the export's name. */
    options: "{{name}} options",
    /** Under an export's name: `done` of its `count` clips are made. */
    clipsDone_one: "{{done, number}} of {{count}} clip",
    clipsDone_other: "{{done, number}} of {{count}} clips",
    /** The same for an export that joins its clips into one file. */
    clipsDoneInOneFile_one: "{{done, number}} of {{count}} clip in one file",
    clipsDoneInOneFile_other: "{{done, number}} of {{count}} clips in one file",
    /** A button that folds a long list of clips away. */
    hideClips: "Hide clips",
    /** A button that unfolds the list: `count` clips. */
    showClips_one: "Show {{count}} clips",
    showClips_other: "Show {{count}} clips",
    /** The same while some clips show anyway (the ones that failed). */
    showAllClips_one: "Show all {{count}} clips",
    showAllClips_other: "Show all {{count}} clips",
    /** Menu entries of an export: both ask for a place to save and need a step more. */
    saveEdl: "Save EDL…",
    saveCutList: "Save cut list (CSV)…",
    /** Titles of the save dialogs. */
    saveEdlTitle: "Save EDL",
    saveCutListTitle: "Save cut list",
    /** Toasts after saving. */
    savedEdl: "Saved the EDL",
    savedCutList: "Saved the cut list",
    saveEdlFailed: "Couldn't save the EDL",
    saveCutListFailed: "Couldn't save the cut list",
    /** After saving: `count` clips are not in the list. */
    leftOut_one: "{{count}} clip was left out: its episode was removed.",
    leftOut_other: "{{count}} clips were left out: their episodes were removed.",
    /** Badges on an export. */
    queued: "Queued",
    exporting: "Exporting",
    cancelled: "Cancelled",
    fileNotMade: "File not made",
    someFailed: "Some clips failed",
    /** `count` clips (or the one file) were made but couldn't be checked against the source. */
    unverified_one: "{{count}} unverified",
    unverified_other: "{{count}} unverified",
    /** The one file an export joins its clips into. */
    file: {
      /** Under the file's name: how many chapters it has. */
      chapters_one: "{{count}} chapter",
      chapters_other: "{{count}} chapters",
      /** The file was not made: the export was cancelled. */
      notMade: "Not made",
      couldntBeMade: "Couldn't be made",
      joining: "Joining the clips…",
      waiting: "Made once every clip is rendered",
      /** A button that shows the file. */
      show: "Show",
    },
    /** A clip of an export. */
    clip: {
      /** Menu entry: opens the episode at the clip's start. */
      watchSource: "Watch the source",
      copyTimecodes: "Copy timecodes",
      /** After the timecodes: how many frames the clip has. */
      frames_one: "{{count}} frames",
      frames_other: "{{count}} frames",
      /** A button that shows the clip's file. */
      show: "Show",
    },
    /** Tooltips of a clip's mark. */
    verified: "Verified: every frame matches the source",
    unverifiedWhy: "Exported, but the frames couldn't be verified",
  },
} as const
