// The plugins namespace: the Plugins page and Browse, a plugin's card and settings, its pages, the install
// review, the toasts about plugins (docs/DESIGN.md, Copy). English source, typed: `{{variables}}` are checked.
// Comments are notes for translators.
//
// A plugin's own words are never translated: its name, description, author, action and page titles, settings
// and their help, run messages and errors come in as `{{values}}` of AutoScene's sentences. Every sentence that
// mentions a plugin is one whole message, so a language can word it its own way. The catalog's reasons for a
// withdrawal, a plugin's `warnings` and the facts it names (ids, hosts, keys, folders) are values too.
export default {
  /** One plugin, as a noun: the badge on a plugin's own page and the window title's fallback. No plugin can word it. */
  plugin: "Plugin",
  /** The title of a screen that could not read the list of plugins; the reason follows it. */
  unreadTitle: "Couldn't read the plugins",
  /** A button that leads to the Plugins page. */
  goToPlugins: "Go to Plugins",
  /** A badge: the plugin runs in AutoScene's sandbox, so AutoScene holds it to what it declares. */
  sandboxed: "Sandboxed",
  /** A badge: what an update adds that the installed version did not have. */
  new: "New",
  /** A plugin's version, after a number: "version 1.2.0". */
  version: "version {{version}}",
  /** Who a plugin is by, when AutoScene reviewed the package and stands behind the name. `author` is the plugin's. */
  by: "By {{author}}",
  /** Who a plugin is by, when only the plugin says so. */
  saysBy: "Says it is by {{author}}",
  /** The title of the toast when a plugin setting could not be saved; the reason follows it. */
  saveFailed: "Couldn't save the setting",
  /** The name of a button's busy action for a screen reader: "Cardinal options". `name` is the plugin's. */
  options: "{{name}} options",
  /** A switch's name for a screen reader. `name` is the plugin's. */
  enable: "Enable {{name}}",
  /** The switch of a plugin on its own screen. */
  turnedOn: "Turned on",
  /** Under a plugin's card: it is turned on, but AutoScene has not reviewed it and the setting below the cards is off. */
  notAllowed: "Doesn't run until the setting below allows it.",

  /**
   * A plugin whose name is a plugin's own, said by AutoScene. Each starts with a word the plugin cannot choose
   * ("Plugin" or "A plugin"), because a plugin may call itself "AutoScene": keep that word first in every language.
   * `.named` has `{{name}}`, the plugin's own name; `.anonymous` is for a plugin AutoScene does not know.
   */
  toast: {
    /** `action` is the title of what the plugin was asked to do, the plugin's own words. */
    alreadyWaiting: {
      named: "Plugin “{{name}}”: {{action}} is already waiting or running",
      anonymous: "A plugin: {{action}} is already waiting or running",
    },
    cantStart: {
      named: "Plugin “{{name}}”: {{action}} couldn't start",
      anonymous: "A plugin: {{action}} couldn't start",
    },
    /** A plugin's run ended well and said something; `action` is the run's title. */
    finished: {
      named: "Plugin “{{name}}”: {{action}}",
      anonymous: "A plugin: {{action}}",
    },
    failed: {
      named: "Plugin “{{name}}”: {{action}} failed",
      anonymous: "A plugin: {{action}} failed",
    },
    /** A pack was saved with a plugin's action; `file` is the file's name. */
    saved: {
      named: "Plugin “{{name}}”: saved {{file}}",
      anonymous: "A plugin: saved {{file}}",
    },
    updated: {
      named: "Plugin “{{name}}”: updated to {{version}}",
      anonymous: "A plugin: updated to {{version}}",
    },
    updateFailed: {
      named: "Plugin “{{name}}”: couldn't be updated",
      anonymous: "A plugin: couldn't be updated",
    },
    /** The line under a toast that says a run is already waiting or running. */
    inActivity: "It's in Activity.",
    /** The button on the toast that says a file was saved. */
    showInFolder: "Show in folder",
  },

  /** What AutoScene says of how far it trusts a plugin: a badge. */
  trust: {
    withdrawn: "Withdrawn",
    folder: "Loaded from a folder",
    reviewed: "Reviewed by AutoScene",
    pinned: "Signed, not reviewed",
    unreviewed: "Not reviewed",
  },
  /** What a plugin's trust means, in the install review. */
  trustLine: {
    folder: "It runs from its folder, as the files there are now. AutoScene has not reviewed it.",
    reviewed: "AutoScene reviewed this version.",
    pinnedSame: "Signed with the same key as the installed version. AutoScene has not reviewed it.",
    pinnedNew:
      "Signed with a key that came in the package: nothing shows who holds it. AutoScene has not reviewed it. Its updates must be signed with the same key.",
    unreviewed: "Nobody AutoScene knows signed it, and AutoScene has not reviewed it.",
  },

  /** AutoScene took a plugin back. */
  withdrawn: {
    /** The banner's title and the toast's. `name` is the plugin's. */
    title: "AutoScene withdrew Plugin “{{name}}”",
    /** Under the title. `reason` is the catalog's, a sentence that ends in a full stop. */
    line: "{{reason}} It is turned off.",
    /** The line after a few banners; `count` more withdrawn plugins are marked on their cards. */
    more_one: "and {{count}} more plugin AutoScene withdrew, marked on its card under Installed.",
    more_other: "and {{count}} more plugins AutoScene withdrew, marked on their cards under Installed.",
  },

  /** The size a plugin's large files take, and where they come from. `size` is written already ("312 MB"). */
  resource: {
    /** `hosts` is a list of web sites: "a.example, b.example and c.example". */
    firstRun:
      "It downloads {{size}} at its first run. AutoScene keeps the files in its cache and checks each one by its hash.",
    firstRunFrom:
      "It downloads {{size}} from {{hosts}} at its first run. AutoScene keeps the files in its cache and checks each one by its hash.",
    /** A service starts once and keeps running: its first download is at its first start. */
    firstStart:
      "It downloads {{size}} at its first start. AutoScene keeps the files in its cache and checks each one by its hash.",
    firstStartFrom:
      "It downloads {{size}} from {{hosts}} at its first start. AutoScene keeps the files in its cache and checks each one by its hash.",
    /** An update's plugin has run before: its files come at its next run or start. */
    nextRun:
      "It downloads {{size}} at its next run. AutoScene keeps the files in its cache and checks each one by its hash.",
    nextRunFrom:
      "It downloads {{size}} from {{hosts}} at its next run. AutoScene keeps the files in its cache and checks each one by its hash.",
    nextStart:
      "It downloads {{size}} at its next start. AutoScene keeps the files in its cache and checks each one by its hash.",
    nextStartFrom:
      "It downloads {{size}} from {{hosts}} at its next start. AutoScene keeps the files in its cache and checks each one by its hash.",
    /** The files are in AutoScene's cache already; only where they come from is left to say. */
    kept: "The large files it names come from {{hosts}}. AutoScene already has them, and checks each one by its hash.",
  },

  /** What each permission lets a plugin do. */
  permission: {
    /** Said of a program: what it says it does. */
    says: {
      exportsRead: "Reads the exported clips",
      exportsWrite: "Changes or adds files beside the exported clips",
      projectRead: "Reads the project: its episodes and characters",
      network: "Talks to the internet",
    },
    /** Said of a plugin in AutoScene's sandbox: what the sandbox lets it do. */
    walled: {
      exportsRead: "Reads the files in the export's folder",
      exportsWrite: "Reads, changes and deletes files in the export's folder",
      projectRead: "Reads the project's folder",
      /** Followed by the list of web sites it may reach. */
      network: "Talks to these sites, over https only:",
      networkNone: "May use the network, but names no site: every request is refused.",
    },
  },

  /**
   * The limits of a plugin in the sandbox. `memory` is a number of megabytes, written as it is. `n` is the number
   * of minutes or hours, written as it is; `count` is the same number and picks the plural form.
   */
  limits: {
    minutes_one: "It may use up to {{memory}} MB of memory. A run stops after {{n}} minute.",
    minutes_other: "It may use up to {{memory}} MB of memory. A run stops after {{n}} minutes.",
    hours_one: "It may use up to {{memory}} MB of memory. A run stops after {{n}} hour.",
    hours_other: "It may use up to {{memory}} MB of memory. A run stops after {{n}} hours.",
  },

  /** When AutoScene holds a plugin to its permissions. */
  checked: {
    sandbox: "It runs in a sandbox. AutoScene enforces these.",
    pages: "AutoScene checks these each time its pages ask.",
    pagesAndProgram: "AutoScene checks these each time its pages or its program ask it for something.",
    program: "AutoScene checks these each time its program asks it for something.",
  },

  /** The menu of a plugin (⋯ and right-click). */
  menu: {
    update: "Update…",
    settings: "Its settings",
    showSource: "Show its source",
    /** Opens the source a plugin names itself, which AutoScene did not check. */
    showNamedSource: "Show the source it names",
    showFolder: "Show its folder",
    showLog: "Show its log",
    copyId: "Copy its id",
    checkFolder: "Check its folder for changes",
    loadFolderAgain: "Load its folder again…",
    uninstall: "Uninstall…",
    checkFailed: "Couldn't check its folder",
    folderUnchanged: "Its folder is as it was",
    folderChanged: "Read the changes in its folder",
    linkFailed: "Couldn't open the link",
    turnOnFailed: "Couldn't turn the plugin on",
    turnOffFailed: "Couldn't turn the plugin off",
  },

  /** Installing a plugin: the menu entries and buttons that start it. */
  installFromFile: "Install from a file…",
  loadFolder: "Load a folder…",
  /** The title of the OS dialog that picks a plugin's folder. */
  chooseFolder: "Choose the plugin's folder",

  /** Plugin packages dropped on AutoScene or opened from outside. `file` is a file's name. */
  packages: {
    alreadyOne: "{{file}} is already being reviewed",
    alreadyMany: "These plugins are already being reviewed",
    /** `count` is the number of plugins waiting; one names its file. */
    waits_one: "{{file}} waits for the review open now",
    waits_other: "{{count}} plugins wait for the review open now",
    reading: "Reading {{file}}…",
    /** `rest` is how many more wait; `count` is the same number. */
    readingMore_one: "Reading {{file}}… {{rest}} more wait their turn",
    readingMore_other: "Reading {{file}}… {{rest}} more wait their turn",
  },

  /** The Plugins page. */
  page: {
    title: "Plugins",
    /** Under the title: how many plugins, and how many run. `running` is a number. */
    summary_one: "{{count}} plugin · {{running}} running",
    summary_other: "{{count}} plugins · {{running}} running",
    /** Names the choice between the installed plugins and the catalog. */
    show: "Plugins to show",
    installed: "Installed",
    browse: "Browse",
    find: "Find a plugin",
    empty: {
      title: "No plugins yet",
      /** When plugins that AutoScene has not reviewed are allowed, a folder may be loaded too. */
      allowed:
        "Browse the plugins AutoScene reviewed, install one from its .asp file, or load one from its folder. Its actions then show in each export's menu, and its commands in the palette.",
      notAllowed:
        "Browse the plugins AutoScene reviewed, or install one from its .asp file. One AutoScene hasn't reviewed needs the setting below first. Its actions then show in each export's menu, and its commands in the palette.",
      browse: "Browse plugins",
    },
    noMatch: {
      title: "Nothing matches",
      /** `query` is what the person typed. */
      description: "No plugin's name, author, description or id has “{{query}}”.",
      clear: "Clear the search",
    },
    /** A heading above the plugins the catalog has a newer version of. */
    updates: "Updates",
    /** Updates every plugin whose update asks for nothing new. `n` is how many. */
    updateAll: "Update all ({{n}})",
    /** A heading above the other plugins, when there are updates. */
    others: "Installed",
    run: {
      title: "Who may run code here",
      allow: "Allow plugins that AutoScene has not reviewed",
      description:
        "Unsigned plugins, and ones only their author signed, and plugins loaded from a folder. AutoScene can't check what they do.",
    },
    /** The window's title for a plugin's settings: "Plugins · Discord Notify". `name` is the plugin's. */
    crumb: "Plugins · {{name}}",
    /** A plugin's page in a menu: "Plugin “Discord Notify” · Dashboard". Keep the fixed first word. */
    label: "Plugin “{{name}}” · {{title}}",
    /** The tooltip of a plugin's page in the rail. Keep the fixed first word. */
    railTooltip: "Plugin “{{name}}”: {{title}}",
  },

  /** Browse: what AutoScene's catalog offers. */
  browse: {
    readFailed: "Couldn't read the plugin catalog",
    newer: {
      title: "Update AutoScene to use the plugin catalog",
      description: "A later AutoScene checked the catalog, or its index needs one. Plugins still install from their .asp file.",
      check: "Check for updates",
    },
    closed: {
      title: "The plugin catalog isn't open yet",
      description: "This AutoScene has no plugin catalog. Plugins still install from their .asp file.",
    },
    outOfDate: {
      title: "The plugin catalog is out of date",
      /** `date` is the day it was last fetched, written already. */
      since: "AutoScene couldn't check it since {{date}}. Plugins can't be installed from it until it can.",
      never: "AutoScene couldn't check it yet. Plugins can't be installed from it until it can.",
      tryAgain: "Try again",
    },
    group: "Catalog",
    catalog: {
      label: "AutoScene's plugin catalog",
      description: "Plugins AutoScene reviewed, each built from its public source.",
      checkNow: "Check now",
    },
    daily: {
      label: "Check the plugin catalog every day",
      on: "AutoScene asks GitHub once a day for new versions and withdrawn plugins.",
      off: "Off: AutoScene doesn't learn of withdrawn plugins.",
    },
    none: {
      title: "The catalog lists no plugins yet",
      description: "Check again later.",
    },
    noMatch: {
      title: "Nothing matches",
      description: "No plugin in the catalog has “{{query}}” in its name, author, summary or id.",
      clear: "Clear the search",
    },
  },

  /** The line under the catalog's title. */
  catalog: {
    /** The day the catalog held was published, written already. */
    of: "Catalog of {{date}}.",
    noneYet: "No catalog yet.",
    notChecked: "Not checked yet.",
    checkedToday: "Checked today.",
    checkedYesterday: "Checked yesterday.",
    checkedOn: "Checked on {{date}}.",
    failedToday: "Last check failed today.",
    failedYesterday: "Last check failed yesterday.",
    failedOn: "Last check failed on {{date}}.",
    /** `hosts` is the list of web sites, separated by commas. */
    talksTo: "Talks to {{hosts}}",
    gpu: "Uses the graphics card",
    nothing: "Names no permissions",
    /** On an entry's card. `size` is written already ("312 MB"). */
    downloadsRun: "Downloads {{size}} at its first run",
    downloadsStart: "Downloads {{size}} at its first start",
    /** An update brings files from a host the installed version had none from; `hosts` is a list. */
    newHosts_one: "Downloads from a new host: {{hosts}}",
    newHosts_other: "Downloads from new hosts: {{hosts}}",
    /** A catalog entry's card. */
    installed: "Installed",
    elsewhere: "Not for this computer",
    needsNewer: "Needs a newer AutoScene",
    install: "Install…",
    update: "Update…",
    /** The picture's name for a screen reader. */
    installNamed: "Install {{name}}…",
    settingsNamed: "{{name}} settings",
    /** The checks of the catalog, as a toast. */
    checkFailed: "Couldn't check the plugin catalog",
    needsNewerApp: "This catalog needs a newer AutoScene.",
    noCatalog: "This AutoScene has no plugin catalog.",
    upToDate: "Plugins are up to date",
    updates_one: "{{count}} plugin update",
    updates_other: "{{count}} plugin updates",
  },

  /** Asking before a plugin goes. */
  uninstall: {
    /** `name` is the plugin's. */
    toast: "Uninstalled {{name}}",
    title: "Uninstall {{name}}?",
    description: "Its commands, settings and actions will go.",
    answer: "Uninstall",
    failure: "Couldn't uninstall the plugin",
    dataLabel: "Delete what it kept too",
    dataDescription: "Remove its saved settings, credentials, caches and logs.",
  },

  /** A plugin's page in a frame. */
  frame: {
    quiet: "This page isn't responding.",
    closed: "This page was closed.",
    overflow: "This page sent AutoScene messages it couldn't read.",
    failed: "Couldn't open this page.",
    reload: "Reload",
    /** The menu of a page. */
    reloadPage: "Reload the page",
    showServiceLog: "Show its service log",
    unavailable: "This page isn't available",
    /** Why a plugin's page can't be used when the plugin is turned off. `name` is the plugin's. */
    turnedOff: "{{name}} is turned off.",
    /** The way back from a project's page, when the project's name is not known yet. It follows "Back to" in the top bar: keep it a bare noun phrase. */
    theProject: "the project",
  },

  /** A plugin's own screen. */
  screen: {
    notInstalled: "This plugin isn't installed",
    notInstalledDescription: "It was uninstalled, or this link is to another one.",
    /** The breadcrumb back to the Plugins page. It follows "Back to" in the top bar: keep it a bare noun phrase. */
    back: "Plugins",
    notReviewed: "AutoScene hasn't reviewed this plugin, so it doesn't run.",
    allowThem: "Allow them…",
    cantRun: "This plugin can't run",
    settings: {
      title: "Settings",
      withSecrets: "Each run gets them as they are when it starts, and a secret each time it asks for it.",
      plain: "Each run gets them as they are when it starts.",
      /** `why` is the reason it could not be read. */
      unread: "Couldn't read its settings: {{why}}",
      /** The note on a secret on macOS, under the setting's own help. */
      macAgain: "macOS may ask you to allow it again after AutoScene updates.",
      choose: "Choose…",
      /** `title` is the setting's own title. */
      takesNumber: "{{title}} takes a number.",
    },
    runs: {
      title: "Runs by itself",
      description: "Each runs without being asked once you turn it on, as a job in Activity.",
      afterExport: "After every export",
      afterExportFailed: "After every export, also one that failed",
      /** A switch's name. `title` is the action's own title. */
      runAfter: "Run {{title}} after every export",
      runAfterFailed: "Run {{title}} after every export, also one that failed",
    },
    pages: {
      title: "Pages",
      description: "Each opens full screen. What is on it is the plugin's.",
      inProject: "Open it from a project's More menu.",
      open: "Open",
      /** `title` is the page's own title. */
      openNamed: "Open {{title}}",
    },
    mayUse: {
      title: "What it may use",
      /** Before the line that says when AutoScene checks the permissions of a program. */
      programRights: "Its program runs with your user rights. {{when}}",
      notChecked: "AutoScene doesn't check these: they are what it says it does.",
      gpu: "Uses the graphics card",
      gpuNote: "Its runs wait while an analysis runs.",
      nothing: "Nothing beyond its own settings.",
    },
    keychain: "Saved in your system's keychain",
    files: {
      title: "Files",
      folder: "Its folder",
      log: "Its log",
      serviceLog: "Its service log",
      reveal: "Reveal",
      downloaded: "Downloaded files",
      /** `size` is written already ("312 MB"). */
      downloadedNote: "{{size}}. AutoScene keeps them in its cache and removes them with the plugin.",
    },
    warnings: {
      title: "Skipped in its plugin.json",
      description: "What this AutoScene left out of what the plugin asks for.",
    },
    secret: {
      notSet: "Not set",
      set: "Set…",
      remove: "Remove…",
      /** The buttons' names for a screen reader. `title` is the setting's. */
      setNamed: "Set {{title}}",
      removeNamed: "Remove {{title}}",
      removeTitle: "Remove {{title}}?",
      removeDescription: "It goes from your system's keychain, and the plugin can't use it until you enter it again.",
      removeAnswer: "Remove",
      removeFailed: "Couldn't remove the secret",
      dialogDescription: "Kept in your system's keychain. AutoScene never shows it again.",
      /** `max` is a number of characters, written as it is. */
      tooLong: "A secret can be at most {{max}} characters.",
      saveFailed: "Couldn't save the secret",
      save: "Save",
    },
  },

  /** Things a plugin can run by itself, and what an event is. */
  event: {
    analysisFinished: {
      /** Under an action that runs on an event. */
      when: "When a project's episodes are all analysed",
      /** A switch's name. `title` is the event action's own title. */
      run: "Run {{title}}, when a project's episodes are all analysed",
      /** In the install review's list of what a plugin adds; the label before the colon is muted. `<what/>` is the action's own title. */
      adds: "<x>Can run when an analysis finishes:</x> <what/><x> (you turn it on)</x>",
    },
  },

  /** The review of a plugin before it is installed, loaded or updated. */
  review: {
    /** The dialog's title and its button. `name` is the plugin's. */
    install: "Install {{name}}?",
    load: "Load {{name}}?",
    update: "Update {{name}}?",
    installButton: "Install",
    loadButton: "Load",
    updateButton: "Update",
    /** The toast after it went through. */
    installed: "Installed {{name}}",
    loaded: "Loaded {{name}}",
    updated: "Updated {{name}}",
    failed: "Couldn't install the plugin",
    /** A package on its way from the catalog. `name` is the plugin's. */
    getting: "Getting {{name}}…",
    updating: "Updating {{name}}…",
    gettingReady: "Getting {{name}} ready…",
    compiling: "AutoScene compiles it so it can run. This can take up to 2 minutes, and your computer may be busy meanwhile.",
    nothingNew: "It asks for nothing new, so there is nothing to review.",
    /** `done` and `total` are sizes, written already. */
    progress: "{{done}} of {{total}} from AutoScene's plugin catalog",
    fromCatalog: "From AutoScene's plugin catalog",
    /** The facts at the top: a plugin's id, version, author, its key, the folder. */
    fact: {
      id: "Id",
      version: "Version",
      author: "Author",
      saysBy: "Says it is by",
      notNamed: "Not named",
      key: "Key",
      folder: "Folder",
      /** After the version. `version` is the installed one. */
      isInstalled: "{{version}} is installed",
    },
    /** Where AutoScene's catalog built a plugin it reviewed from. `<repo/>`, `<commit/>` and `<path/>` are placed by the code and shown as they are. */
    built: "Built from <repo/> at <commit/>.",
    builtIn: "Built from <repo/> at <commit/>, in <path/>.",
    /** What the installed version was. `label` is a trust badge. */
    before: {
      unsigned:
        "The installed version wasn't signed. This one pins its key: from now on, only updates signed with it install.",
      was: "The installed version: {{label}}.",
      folderReplaced: "This package takes its place; that folder stays as it is.",
      leftSandbox: "The installed version ran in a sandbox. This version runs with your user rights.",
    },
    ownWords: "In its own words",
    warnings: "Skipped in its plugin.json",
    componentWarnings: "Noticed in its component",
    /** The field that confirms a plugin AutoScene has not reviewed. `<x/>` is the plugin's name, placed by the code: keep it as it is. */
    typeToInstall: "Type <x/> to install it",
    typeToLoad: "Type <x/> to load it",
    typeToUpdate: "Type <x/> to update it",
    cantCheck: "AutoScene can't check what it does.",
    turnedOff: "Plugins that AutoScene has not reviewed are turned off.",
    allowThem: "Allow them…",
    permissions: {
      pagesMayUse: "What its pages may use",
      saysItDoes: "What it says it does",
      notEnforced: "What it says it does (not enforced)",
      userRights: "It runs with your user rights: it can read, change and send anything you can.",
      /** `hosts` are the web sites it names; "New" marks the ones an update adds. */
      newHosts: "New: {{hosts}}",
      none: "It names no permissions.",
      gpu: "Uses the graphics card: its runs wait while an analysis runs",
    },
    sandbox: {
      intro: "It runs in a sandbox. It sees only its own folder and what is listed here.",
      mayDo: "What it may do (enforced)",
      none: "It asks for nothing more.",
      cant: "It can't start programs or see your other files.",
      unlimitedNetwork: "Not limited: what it sends to those sites, and how much it writes.",
      unlimited: "Not limited: how much it writes.",
    },
    runs: {
      title: "How it runs",
      none: "Runs no program. Its pages run in a sandbox.",
      wasm: "A WebAssembly plugin. AutoScene compiles it once you say yes.",
      program: "A program in its folder.",
      /**
       * A Python script that uv runs. `size` is how much AutoScene's own uv takes, written already.
       * With "Gets" AutoScene fetches uv first, and uv then downloads Python and the packages.
       */
      uvRun:
        "A Python script that uv runs. On its first run, uv downloads Python (if this computer has none) and the packages the script names.",
      uvRunGets:
        "A Python script that uv runs. On its first run AutoScene gets uv ({{size}}, from github.com/astral-sh/uv), and uv downloads Python (if this computer has none) and the packages the script names.",
      uvStart:
        "A Python script that uv runs. On its first start, uv downloads Python (if this computer has none) and the packages the script names.",
      uvStartGets:
        "A Python script that uv runs. On its first start AutoScene gets uv ({{size}}, from github.com/astral-sh/uv), and uv downloads Python (if this computer has none) and the packages the script names.",
      noProgramBefore: "The installed version runs no program.",
      service: "Keeps its program running while its page is open.",
      becomesService: "Now keeps its program running while its page is open.",
      /** `hosts` is a list of web sites. Marked "New". */
      newFileHosts: "The installed version names no files from {{hosts}}.",
    },
    /** What an update or a plugin adds. The label before the colon is muted: `<x>…</x>`. `<what/>` is the plugin's own title, placed by the code: keep it as it is. */
    adds: {
      title: "What it adds",
      pageRail:
        "<x>A page:</x> <what/><x>, in the rail (it holds {{max}} plugin pages; others are in the command palette)</x>",
      pageProject: "<x>A page:</x> <what/><x>, in each project's More menu</x>",
      pagePalette: "<x>A page:</x> <what/><x>, in the command palette</x>",
      exportAction: "<x>Each export's menu:</x> <what/>",
      command: "<x>Command palette:</x> <what/>",
      shotFilter: "<x>A scenepack filter:</x> <what/>",
      packAction: "<x>A scenepack's ⋯ menu:</x> <what/>",
      afterFailed: "<x>Can run after every export, also one that failed:</x> <what/><x> (you turn it on)</x>",
      settings_one: "<x>Settings:</x> {{count}} setting",
      settings_other: "<x>Settings:</x> {{count}} settings",
      secret: "Keeps {{title}} in your system's keychain",
      nothing: "Nothing to list.",
    },
  },
} as const
