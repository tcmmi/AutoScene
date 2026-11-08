// The settings namespace: the Settings screen, every section of it and the Shortcuts screen's own words.
// (Shortcut names and the reasons a key is refused are in `shortcuts`.) English source, typed: `{{variables}}`
// are checked. Comments are notes for translators.
export default {
  /** The window's Settings: its title, the list on the left and the menu of a right-click on the page. */
  page: {
    /** The page title, the heading of the list and its accessible name. */
    title: "Settings",
  },

  /** The sections in the list on the left; each is also the heading of its page and the end of the window title. */
  section: {
    general: "General",
    discord: "Discord",
    shortcuts: "Shortcuts",
    updates: "Updates",
    about: "About",
    contributors: "Contributors",
  },

  /** Toast title when a setting could not be saved; the reason follows as the description. */
  saveFailed: "Couldn't save the setting",
  /** A button that shows a folder in the system's file manager. */
  reveal: "Reveal",

  /** The path rows (library location, app data, the FFmpeg path): the right-click menu. */
  path: {
    show: "Show in folder",
    copy: "Copy path",
  },

  general: {
    /** The group that chooses the language of AutoScene itself, not of a video's audio. */
    language: {
      title: "Language",
      description: "The words AutoScene uses, and how it writes dates and numbers.",
      app: {
        /** Label of the choice and its accessible name; it must not read like the audio's preferred language. */
        label: "App language",
        /** The first choice of the list. `{{language}}` is the language Automatic comes to here, in that language's own name. */
        automatic: "Automatic ({{language}})",
      },
    },
    storage: {
      title: "Storage",
      description: "Where AutoScene keeps projects, models and logs.",
      library: {
        label: "Library location",
        note: "New projects and default exports go here. Existing projects stay where they are.",
        /** The button that opens a folder picker. */
        change: "Change…",
        /** The title of the folder picker's window. */
        pickTitle: "Choose the library folder",
      },
      appData: "App data",
    },
    characters: {
      title: "Characters",
      description: "How the analysis finds who is in a shot.",
      bodies: {
        /** Label of the switch and its accessible name. */
        label: "Find characters without a visible face",
        description:
          "Also follows people by their body and clothes, so shots where someone faces away or is too small for their face join their scenepack, marked “No face”. Analysis takes about a quarter longer; episodes analysed before are looked through again for their people. Its two models come from InsightFace and may only be used for non-commercial work.",
      },
      fastScan: {
        label: "Fast Scan",
        /** A line saying what Automatic means on this computer follows it. */
        description:
          "Looks at only a few frames of each shot: much faster, but it finds fewer faces and no one without a visible face. Applies to new projects; Edit details… changes one. Its face models may be used for any work, the full scan's only for non-commercial work.",
        /** The accessible name of the three-way choice. */
        aria: "Fast Scan for new projects",
        /** The three choices of the pill switch: short words. */
        auto: "Automatic",
        on: "On",
        off: "Off",
      },
      animeModel: {
        label: "Anime characters",
        /** A line saying what Automatic means on this computer follows it. */
        description:
          "How anime projects tell their characters apart. Best puts far more of each character's scenes on the right character, using a second model from UC Berkeley's research: a one-time download of about 1.2 GB, and analysis takes about a fifth longer. Fast Scan keeps the Standard model. Its model states no licence yet.",
        /** The accessible name of the three-way choice. */
        aria: "Model for anime characters",
        /** The three choices of the pill switch: short words. */
        auto: "Automatic",
        standard: "Standard",
        best: "Best",
        /** What the choice comes to here, under the description. */
        hereBest: "Best on this computer.",
        hereStandard: "Standard on this computer.",
        /** Why Automatic chose: the graphics card. */
        autoBest: "Automatic picks Best here: your graphics card is built for AI work.",
        autoStandard: "Automatic picks Standard here: Best needs graphics built for AI work.",
        /** Toast after a change that switches the model: anime projects are analysed again. */
        switched: "Anime projects find their characters again",
        switchedBody: "Names and your decisions stay. It runs in the background.",
      },
      names: {
        /** Label of the switch and its accessible name. */
        label: "Character names from AniList, TVmaze and Fandom",
        description:
          "Once a project's characters are found, AutoScene looks their names up — an anime's on AniList, an animated series' on TVmaze, an animated film's on its franchise's Fandom wiki (found through Wikidata) — and you give them with one click. Sends the project's title (or the search text you type), and requests for the matching cast and pictures, to them (APIs and image servers); they see your IP address. Animation projects download the anime character model once to compare the pictures. Turning it off deletes every picture kept from them.",
      },
    },
    audio: {
      title: "Audio",
      description: "Many files carry several soundtracks, and dubs are often marked as the default.",
      language: {
        label: "Preferred language",
        description: "Chosen automatically when an episode is added. You can switch the track per episode at any time.",
        /** Accessible name of the choice; it must not read like the app's own language. */
        aria: "Preferred audio language",
      },
    },
    export: {
      title: "Export",
      description: "Defaults for new exports. You can change them per export.",
      format: {
        label: "Format",
        aria: "Default export format",
      },
    },
  },

  hardware: {
    title: "Hardware acceleration",
    description: "Where analysis runs on this computer.",
    /** The first choice of the list: AutoScene picks. */
    automatic: "Automatic",
    /** A choice of the list: no graphics card is used. */
    processorOnly: "Processor only",
    /** The row's label: a Mac has a chip, other computers a graphics card. */
    chip: "Chip",
    graphicsCard: "Graphics card",
    checkFailed: {
      chip: "Couldn't check the chip.",
      cards: "Couldn't check the graphics cards.",
    },
    checking: {
      chip: "Checking the chip…",
      cards: "Looking for graphics cards…",
    },
    /** One sentence for where analysis runs. `name` is the card's own name, as the maker writes it. */
    line: {
      pending: "Applies from the next analysis.",
      /** The card the person chose is not there. Three whole sentences: what it uses instead differs. */
      missing: {
        processor: "The graphics card you chose isn't connected, so analysis uses the processor.",
        card: "The graphics card you chose isn't connected, so analysis uses the {{name}}.",
        generic: "The graphics card you chose isn't connected, so analysis uses this computer's graphics card.",
      },
      fellBack: "In the last analysis some steps ran on the processor, which is slower.",
      processorChosen: "Analysis runs on the processor. That is much slower.",
      /** Where it runs now; "Accel" says NVIDIA's acceleration is on as well. */
      runs: {
        processor: "Analysis runs on the processor.",
        processorAccel: "Analysis runs on the processor, with NVIDIA acceleration.",
        card: "Analysis runs on the {{name}}.",
        cardAccel: "Analysis runs on the {{name}}, with NVIDIA acceleration.",
        generic: "Analysis runs on this computer's graphics card.",
        genericAccel: "Analysis runs on this computer's graphics card, with NVIDIA acceleration.",
      },
    },
    accel: {
      /** The row's label and the accessible name of its switch. */
      label: "NVIDIA acceleration",
      /** `mb` is the download's size in megabytes; <go> is the link to NVIDIA's licence. */
      available:
        "NVIDIA's engine for GeForce RTX cards, so anime analysis runs faster. AutoScene downloads NVIDIA's files (about {{mb}} MB) from the Python Package Index, which sees your IP address. By downloading you agree to <go>NVIDIA's licence</go>.",
      agree: "Agree and download",
      downloading: "Downloading NVIDIA's files.",
      /** `done` and `total` are megabytes, under the progress bar. */
      progress: "{{done}} of {{total}} MB",
      removing: "Removing after the current analysis.",
      notUsedProcessor: "Not used: analysis runs on the processor.",
      notUsedOtherCard: "Not used: another graphics card is chosen.",
      stopped: "NVIDIA acceleration stopped working, so analysis runs without it.",
      needsDriver: "NVIDIA acceleration couldn't use this card. A newer NVIDIA driver may help.",
      getDriver: "Get the driver",
      ready: "NVIDIA's engine for GeForce RTX cards, so anime analysis runs faster.",
      /** `mb` is the size in megabytes. */
      update: "Download the update (about {{mb}} MB)",
      removeFiles: "Remove NVIDIA's files",
      /** Toast titles when a step fails; the reason follows as the description. */
      installFailed: "Couldn't download NVIDIA's files",
      cancelFailed: "Couldn't cancel the download",
      retryFailed: "Couldn't try again",
      remove: {
        title: "Remove NVIDIA acceleration?",
        /** Analysis is using the files now. */
        slower: "Analysis keeps running on this graphics card, more slowly. You can download it again any time.",
        /** Nothing is using the files: only the disk changes. */
        plain: "You can download it again any time.",
        /** The confirming button. */
        answer: "Remove",
        failed: "Couldn't remove NVIDIA's files",
      },
    },
  },

  discord: {
    status: {
      title: "Your status",
      description: "Let people on Discord see that you're working in AutoScene.",
    },
    show: {
      /** Label of the switch and its accessible name. */
      label: "Show on Discord",
      description: "Goes through the Discord app on this computer. Nothing is sent until you turn this on.",
    },
    /** The label of the row that shows what the status looks like: while Discord is on, and while it is off. */
    seeing: {
      on: "What people see",
      off: "What people would see",
      description: "Always in English, whatever language AutoScene speaks. Plus how long AutoScene has been open and a link to it.",
    },
    names: {
      title: "Names",
      description: "Off, your status says what you're doing without naming it.",
      /** The three things a status may name; each has a switch. */
      project: "Project",
      episode: "Episode or file",
      character: "Character",
    },
  },

  /** The Shortcuts screen. Names of shortcuts and the reasons a key is refused are in `shortcuts`. */
  keymap: {
    title: "Keymap",
    description: "Click a key below to change it, then press the new one.",
    /** The editor whose keys are used: the row's label and its accessible name; the choices are program names. */
    likeIn: "Keys like in",
    yourChanges: "Your changes",
    /** `preset` is the editor's name. */
    none: "None: every key is {{preset}}'s.",
    differs_one: "{{count}} key differs from {{preset}}'s.",
    differs_other: "{{count}} keys differ from {{preset}}'s.",
    resetAll: "Reset all",
    /** Toast title after choosing another editor's keys. */
    switched: "Keys like in {{preset}}",
    /** Its description when the person's own changes were dropped by it. */
    letGo_one: "Your change was let go.",
    letGo_other: "Your {{count}} changes were let go.",
    /** Toast title after "Reset all". */
    backAll: "Back to {{preset}}'s keys",
    /** `names` is a list of shortcut names in quotation marks. */
    tookBack: "Took them back from {{names}}",
    /** `caps` is the key ("Ctrl U"); `names` as above. */
    tookKey: "Took {{caps}} from {{names}}",
    /** The title of the card under an editor's option: its keys that differ from AutoScene's. */
    unlike: "Unlike AutoScene's keys",
    /** Tooltip of the reset button of a row; the number is how many keys the row has. */
    backTo_one: "Back to {{preset}}'s key",
    backTo_other: "Back to {{preset}}'s keys",
    /** Accessible name of that button; `name` is a shortcut's name. */
    reset: "Reset “{{name}}”",
    /** Tooltip of a key button, for a shortcut that has one key. */
    change: "Change the key",
    clickToChange: "Click to change",
    /** Accessible name of a key button that is listening. */
    pressNew: "Press the new key for “{{name}}”",
    /** Accessible name of a key button: what it does, its key, then the action. */
    slot: "{{name}}: {{keys}}. Change",
    slotNone: "{{name}}: no key. Change",
    /** Inside a key button that is listening. */
    pressKey: "Press a key",
    /** A key button of a shortcut that has no key. */
    noKey: "No key",
    /** Shown when a key can't be recorded. */
    useCmd: "Use ⌘ instead of Control",
    windowsKey: "The Windows key is Windows'",
  },

  updates: {
    title: "Updates",
    description: "AutoScene updates itself in place. Your projects and models are kept.",
    /** The button, a context-menu item of Settings and the words the line below its switch points at. */
    check: "Check for updates",
    checkingLine: "Checking…",
    autoLine: "Checks automatically at launch and every few hours.",
    manualLine: "Checks only when you ask.",
    upToDate: "You're up to date",
    /** Follows "You're up to date" after a separator: when the last check ended. */
    checked: {
      justNow: "checked just now",
      minutes_one: "checked {{count}} min ago",
      minutes_other: "checked {{count}} min ago",
      hours_one: "checked {{count}} h ago",
      hours_other: "checked {{count}} h ago",
    },
    /** `version` is a number like 1.0.3. */
    ready: "Version {{version}} is ready to install.",
    /** Where the update feed names no version. */
    readyNoVersion: "An update is ready to install.",
    /** A badge: the update feed is a test one. */
    simulated: "Simulated feed",
    view: "View update",
    auto: {
      /** Label of the switch and its accessible name. */
      label: "Check for updates automatically",
      /** `check` is the button's label (`updates.check`), filled in by the code. */
      description: "At launch and every few hours. Off, AutoScene only looks when you click {{check}}.",
    },
    whatsNew: {
      label: "What's new",
      description: "What changed in AutoScene {{version}}, on GitHub.",
      button: "Release notes",
    },
  },

  about: {
    /** `version` is a number like 1.0.3; <mono> sets it in the app's monospace face. */
    version: "Version <mono>{{version}}</mono>",
    openLogs: "Open logs",
    checking: "Checking…",
    media: {
      title: "Media engine",
      description: "Decoding, indexing and encoding run on FFmpeg.",
    },
    ai: {
      title: "AI engine",
      description: "Analysis runs on ONNX Runtime.",
      /** The ONNX Runtime row: with its version, and with the way it computes on this system (a whole label each). */
      onnx: {
        plain: "ONNX Runtime",
        plainVersion: "ONNX Runtime {{version}}",
        directMl: "ONNX Runtime with DirectML",
        directMlVersion: "ONNX Runtime {{version}} with DirectML",
        coreMl: "ONNX Runtime with Core ML",
        coreMlVersion: "ONNX Runtime {{version}} with Core ML",
      },
      nvidiaDescription: "NVIDIA's own files, downloaded from Settings → General.",
      nvidiaLicence: "NVIDIA's licence",
      notices: "Acknowledgements",
      noticesFailed: "Couldn't open the notices",
    },
    models: {
      title: "AI models",
      description:
        "Downloaded the first time an analysis needs them. Models marked non-commercial may only be used for non-commercial work; Fast Scan uses none of them.",
      downloaded: "Downloaded",
      notDownloaded: "Not downloaded yet",
      /** A badge on a model that may not be used for paid work. */
      nonCommercial: "Non-commercial",
      /**
       * What each model is for, the row's title; one per model of `models/manifest.toml`, said in the same words
       * (a test holds them to it). A model a later version adds shows the manifest's English until it has a key.
       */
      purpose: {
        transnetv2: "Shot boundaries (hard cuts and gradual transitions)",
        scrfd: "Face detection with five landmarks (live action)",
        arcface: "Face identity embeddings (live action)",
        yunet: "Face detection with five landmarks (Fast Scan)",
        sface: "Face identity embeddings (Fast Scan)",
        personDetector: "People (whole bodies), to follow characters while their face doesn't show",
        personLook: "How a person looks (clothes, build), to find them again without a face",
        animeHead: "Head detection (anime characters)",
        animePerson: "Whole-person detection (anime), to catch heads the head model misses",
        animeIdentity: "Character identity embeddings (anime), 768-d CCIP vectors",
        animeIdentityBest: "Character identity embeddings (anime, Best scan), 1024-d",
      },
    },
    licenses: {
      title: "Open-source licenses",
      description: "AutoScene is built on these projects.",
    },
    stills: {
      title: "Example stills",
      description: "The footage examples in a new project are stills from these films and series, cropped and scaled. They belong to their owners.",
    },
  },

  contributors: {
    banner: {
      title: "Community Contributors",
      description: "AutoScene is made possible by open-source contributors and community feedback.",
      action: "Contribute on GitHub",
    },
    title: "Contributors",
    description: "People who have shaped AutoScene with code, design, and ideas.",
    /** Accessible name of the button that opens a contributor's page; `name` is theirs. */
    open: "Open {{name}}'s GitHub profile",
    /** What each person did, under their name; one per person of `contributors.json`, in the same words (a test holds them to it). */
    role: {
      tcmmi: "Creator, media engine, models, and core architecture",
      "0nsku": "Core engineering, anime benchmarks, and analysis",
      moongetsu: "UI design, custom context menus, and visual polish",
      ihyledd: "macOS port, settings redesign, and project layout",
    },
  },
} as const
