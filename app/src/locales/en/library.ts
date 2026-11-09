// The library namespace: the library, new projects, a project's details and removal, what the footage can be,
// Fast Scan's sentences, and the cards a character's shots are shown on (docs/DESIGN.md, Copy).
// English source, typed: `{{variables}}` are checked. Comments are notes for translators.
export default {
  /** The library page: the project touched last, then every project. */
  page: {
    /** The page's title in the top bar. */
    title: "Library",
    /** A button, a card in the grid and the welcome screen's button: starts a project. */
    newProject: "New project",
    /** The line under "New project" on the card and next to the welcome button. */
    dropHint: "or drop a season folder anywhere",
    /** The heading over every project's card. */
    allProjects: "All projects",
    /** The title of the error shown in place of the library. */
    loadFailed: "The library couldn't be loaded",
    /** Small line over the big title of the project touched last: pick up where you left off. */
    continue: "Continue",
  },
  /** The first screen of a library with nothing in it. */
  welcome: {
    title: "Build your first scenepack",
    intro:
      "AutoScene finds every shot, learns the cast across episodes, and cuts clean, frame-exact clips of the character you choose.",
    /** Three steps, each a title and a line. */
    steps: {
      add: {
        title: "Add episodes",
        body: "Drop a season folder or a movie. Episodes are recognised and put in order automatically.",
      },
      cast: {
        title: "Meet the cast",
        body: "Every character is found across all episodes. Name, merge or correct them in a keystroke.",
      },
      export: {
        title: "Export packs",
        body: "Frame-exact clips in ProRes, DNxHR or H.264, named by character and episode, ready for any editor.",
      },
    },
  },
  /** What a project offers in its card's menu, its right-click menu and the rail's. */
  project: {
    /** Both the button over the project touched last and a menu entry. */
    open: "Open project",
    /** A movie's list of videos. */
    files: "Files",
    /** A series' list of videos. */
    episodes: "Episodes",
    /** A button and a menu entry that go to the project's characters. */
    cast: "Cast",
    exports: "Exports",
    showFolder: "Show project folder",
    editDetails: "Edit details…",
    remove: "Remove…",
    /** The label of the ⋯ button on a project's card: the project's title, then what the button is. */
    options: "{{title}} options",
  },
  /** Menu entries that start a project. */
  menu: {
    newProject: "New project…",
    /** In the menu of the library's empty space, where "New project…" is already listed. */
    newFromFolderPage: "New project from a folder…",
    newFromFilesPage: "New project from files…",
    fromFolder: "From a folder…",
    fromFiles: "From files…",
  },
  /** What the footage of a project can be. The pictures' credits stay as they are (names and licences). */
  footage: {
    /** The label of the choice and of its field. */
    label: "Footage",
    live: {
      label: "Live action",
      blurb: "Filmed with real actors. Characters are found by their faces.",
    },
    animation: {
      label: "Animation",
      blurb:
        "Computer-animated (3D) films and series. Characters are found by their faces, tuned for animated characters drawn in one style.",
    },
    anime: {
      label: "Anime",
      blurb:
        "Anime and other hand-drawn 2D animation. Characters are found by their heads and whole look, with detectors made for anime.",
    },
    mixed: {
      label: "Mixed",
      blurb: "Live action and animation in one project. Characters are found by their faces, whichever they are.",
    },
    /** Under the example pictures: `credit` is the pictures' source and licence. */
    stills: "Stills: {{credit}}",
  },
  /** The New project dialog. */
  newProject: {
    title: "New project",
    description: "A project is one series or movie. Episodes are recognised and ordered from their file names.",
    /** The field for the project's name. */
    titleLabel: "Title",
    titlePlaceholder: "Series or movie title",
    typeLabel: "Type",
    /** The label of the switch between a series and a movie. */
    typeAria: "Project type",
    series: "Series",
    movie: "Movie",
    /** The name of a setting, shown as the label of its switch. */
    fastScan: "Fast Scan",
    /** Over the list while files are dragged onto the dialog. */
    dropToAdd: "Drop to add to this project",
    dropToAddKinds: "Season folders and video files",
    /** In the empty list. */
    dropHere: "Drop a season folder or video files here",
    dropFormats: "MKV, MP4, MOV, TS and more. Sources are never copied or changed.",
    /** Opens the system's folder chooser; this is its window title. */
    chooseFolder: "Choose a season or movie folder",
    addFolder: "Add folder…",
    addFiles: "Add files…",
    /** Beside the list, when it has files. */
    folder: "Folder…",
    files: "Files…",
    /** A tooltip and the label of the button that takes a file out of the list, and back in. */
    include: "Include it",
    skip: "Skip it",
    /** `name` is the file's name. */
    includeFile: "Include {{name}}",
    skipFile: "Skip {{name}}",
    /** Under the list: how many videos are in; `count` is a number. A size follows it. */
    videos_one: "{{count}} video",
    videos_other: "{{count}} videos",
    create: "Create project",
    /** Toast titles; the reason follows. */
    readFailed: "Couldn't read those files",
    createFailed: "Couldn't create the project",
    /** Beside Add folder… and Add files…: opens a field for links (YouTube, TikTok) a plugin downloads. */
    addLinks: "Add links…",
    /** Beside Folder… and Files…, when the list has something. */
    links: "Links…",
    /** In a link's row: a link isn't fetched before the project exists. */
    linkWaits: "Downloads after you create the project",
    /** A tooltip and label of the button that takes a link out of the list; `link` is the link, short. */
    removeLink: "Remove it",
    removeLinkNamed: "Remove {{link}}",
    /** Under the list, after the videos and their size: how many links; `count` is a number. */
    linkCount_one: "{{count}} link",
    linkCount_other: "{{count}} links",
    /** Under the list while it has links. */
    linksNote: "Links download after you create the project.",
  },
  /** The field for links that a plugin's source downloads: in New project, and in a project's Add links…. */
  links: {
    /** The field's label. */
    label: "Links",
    placeholder: "Paste links here, one per line",
    /** `sources` lists what the plugins fetch from ("YouTube and TikTok"), `plugins` their names. */
    takes: "Takes links from {{sources}}, with {{plugins}}.",
    /** Under the settings beside the links. */
    settingsNote: "A change counts for downloads that haven't started.",
    rights: "Download only videos you have the right to use.",
    where: "Videos go to the Downloads folder of your AutoScene library.",
    /** A link the list shows as one a plugin takes; `source` is what it fetches from, `plugin` the plugin's name. */
    takenBy: "{{source}} · {{plugin}}",
    /** The button that adds the links read so far; `count` is a number. */
    add_one: "Add {{count}} link",
    add_other: "Add {{count}} links",
    /** While no link can be added yet. */
    addNone: "Add links",
    /** Toast after links were queued; `count` is a number. Its description follows. */
    queued_one: "Downloading {{count}} video",
    queued_other: "Downloading {{count}} videos",
    queuedNote: "Each one joins the project when it's done.",
    /** Toast after links were added that are downloading already. */
    waiting: "Those links are downloading already",
    /** Toast titles; the reason follows. */
    queueFailed: "Couldn't start the downloads",
    readFailed: "Couldn't read those links",
    /** Toast title when some links couldn't be queued; `count` is a number. Their reasons follow. */
    refused_one: "{{count}} link couldn't be added",
    refused_other: "{{count}} links couldn't be added",
    /** The dialog of a project's Add links…. */
    dialogTitle: "Add videos from links",
    dialogDescription: "Each video joins this project when it's downloaded.",
  },
  /** Edit details and Remove from the library. */
  details: {
    title: "Project details",
    description: "The project folder on disk keeps its name.",
    titleLabel: "Title",
    /** Footage can't be changed to or from anime: this is shown over the choices that can't be picked. */
    animeApart: "Anime is analysed differently from the start. Add the videos as a new anime project instead.",
    /** Under the footage choice, once it was changed. */
    refilmedResearched: "Faces are found again for it, then the characters regrouped. Your names and decisions stay.",
    refilmed: "The characters are grouped again for it. Your names and decisions stay.",
    save: "Save",
    /** Toast titles; the reason follows. */
    partlyChanged: "Some details changed, but not all of them",
    changeFailed: "Couldn't change the project",
  },
  remove: {
    /** `title` is the project's title. */
    title: "Remove “{{title}}”?",
    /** `stops` is a sentence saying what stops with the project; it can be empty (the second text is for that). */
    description: "Your source videos and exported clips are never touched.",
    descriptionStops: "Your source videos and exported clips are never touched. {{stops}}",
    /** The button, when the analysis is deleted too, and when it is kept. */
    answerDelete: "Delete project",
    answerKeep: "Remove from library",
    failed: "Couldn't remove the project",
    /** A toast after it was removed. */
    removed: "Removed “{{title}}”",
    deleteAnalysis: "Delete its analysis too",
    deleteAnalysisHint:
      "Shots, faces, the cast with your names and decisions, posters and previews (the .autoscene folder). Added again later, the videos are analysed from scratch.",
  },
  /** Fast Scan's sentences, so Settings, New project and Edit details say the same thing. Each is one whole sentence. */
  fastScan: {
    /** Settings → General: what the default means on this computer. */
    settings: {
      fast: "Every new project starts with Fast Scan, 3D animation too.",
      full: "Every new project starts with the full scan.",
      integratedMac: "Here, Automatic means Fast Scan: this Mac analyses video on its built-in graphics.",
      integrated: "Here, Automatic means Fast Scan: this computer analyses video on its built-in graphics.",
      notForAi: "Here, Automatic means Fast Scan: this computer's graphics card has no hardware made for AI work.",
      processorMac: "Here, Automatic means Fast Scan: AutoScene analyses video on this Mac's processor.",
      processor: "Here, Automatic means Fast Scan: AutoScene analyses video on this computer's processor.",
      forAi: "Here, Automatic means the full scan: this computer's graphics card is built for AI work.",
      unknown: "Here, Automatic means the full scan: AutoScene couldn't recognise this computer's graphics.",
      /** The sentence that follows when live action starts with Fast Scan too. */
      keeps: "3D animation keeps the full scan.",
      /** The sentence that follows when live action keeps the full scan while people without a visible face are found. */
      keepsWithBodies:
        "3D animation keeps the full scan, and so does live action while Find characters without a visible face is on.",
    },
    /** New project: what Fast Scan costs this footage. */
    cost: {
      anime: "Much faster, finds fewer characters.",
      animationBodies: "Much faster, but misses many animated faces and anyone without a visible face.",
      animation: "Much faster, but misses many animated faces.",
      liveBodies: "Much faster, finds fewer faces and no one without a visible face.",
      live: "Much faster, finds fewer faces.",
    },
    /** New project: why Fast Scan is on for this computer, after the cost. */
    suits: {
      integrated: "Suits this computer's built-in graphics.",
      notForAi: "Suits this computer's graphics card.",
      processor: "Suits analysis on the processor.",
    },
    /** Edit details: the same, said with Fast Scan's name, after a sentence about the full scan. */
    fastScanSuits: {
      integrated: "Fast Scan suits this computer's built-in graphics.",
      notForAi: "Fast Scan suits this computer's graphics card.",
      processor: "Fast Scan suits analysis on the processor.",
    },
    /** Edit details: what Fast Scan does to this project. */
    hint: {
      animeRescanned: "Characters are found again this way, then regrouped. Your names and decisions stay.",
      animeFast:
        "Characters are found with the anime models in only a few frames of each shot: much faster, less sure who is who.",
      animeFull: "Characters are found with the anime models every few frames.",
      rescanned: "Faces are found again this way, then the characters regrouped. Your names and decisions stay.",
      fast: "Faces are found the way the Scener app does: much faster, less sure who is who.",
      full: "Faces are found in full: every few frames, with the larger face models (for non-commercial use only).",
    },
    /** The first-start notice: the graphics the analysis runs on. A graphics card's own name is shown as it is. */
    device: {
      processor: "Processor",
      mac: "This Mac",
      integrated: "Built-in graphics",
    },
    /** The first-start notice: why new projects start with Fast Scan here. */
    lead: {
      integratedMac:
        "This Mac analyses video on its built-in graphics. A full scan is heavy work for them, so new projects start with Fast Scan.",
      integrated:
        "This computer analyses video on its built-in graphics. A full scan is heavy work for them, so new projects start with Fast Scan.",
      notForAi:
        "This graphics card has no hardware made for AI work. A full scan is heavy work for it, so new projects start with Fast Scan.",
      processorLinux:
        "On Linux, AutoScene analyses video on the processor. A full scan is heavy work for a processor, so new projects start with Fast Scan.",
      processorMac:
        "This Mac analyses video on its processor. A full scan is heavy work for a processor, so new projects start with Fast Scan.",
      processor:
        "This computer has no graphics card AutoScene can use. A full scan is heavy work for a processor, so new projects start with Fast Scan.",
    },
    /** The first-start notice: what Fast Scan gives and what it costs. */
    notice: {
      faster: { title: "Much faster", body: "It looks at only a few frames of each shot." },
      faces: {
        title: "Less sure who is who",
        body: "It finds fewer faces, so characters get mixed up more often.",
      },
      keeps: "3D animation keeps the full scan.",
      keepsWithBodies:
        "3D animation keeps the full scan, and so does live action while characters without a visible face are found.",
      change: "You can change the default any time in Settings.",
    },
  },
  /** A character's shots as cards: one shot, or shots close together stacked like prints. */
  shots: {
    /** A badge on a shot where the character's face doesn't show but their body does. */
    noFace: "No face",
    watchShot: "Watch shot",
    copyTimecode: "Copy timecode",
    /** Menu entries about the body found in a shot without the character's face: `name` is the character's name. */
    notThem: "Not {{name}}",
    /** The same where the character has no name. */
    notThemUnnamed: "Not them",
    notAPerson: "Not a person",
    /** Tooltips of a shot's card. */
    hintFaceless: "Their face doesn't show: found by their body · Click to watch",
    hintWatch: "Click to watch",
    /** The accessible name of a shot's card. `where` is an episode's label and a timecode ("E01 · 05:13"). */
    label: "Shot",
    labelAt: "Shot at {{where}}",
    /** An addition to a card's accessible name, after the others: ", without their face". */
    withoutFace: "without their face",
    /** A stack of shots close together; `count` is how many. */
    count_one: "{{count}} shot",
    count_other: "{{count}} shots",
    countAt_one: "{{count}} shot at {{where}}",
    countAt_other: "{{count}} shots at {{where}}",
    /** The accessible name of the band an open stack's shots are shown in: `facts` is "5 shots · 11 s". */
    bandAt: "{{facts}} at {{where}}",
    /** The tooltip of a stack. */
    together_one: "{{count}} shot close together · Click to see them",
    together_other: "{{count}} shots close together · Click to see them",
    hide: "Hide its shots",
    show: "Show its shots",
    watchFromFirst: "Watch from the first shot",
    /** The label of the stack's play button, and the button over an open stack. `where` and `clock` are timecodes. */
    watchFromWhere: "Watch from {{where}}",
    watchFromClock: "Watch from {{clock}}",
  },
} as const
