// The cast namespace: the Cast page, a character's page, Review, picking and renaming a character, the names
// from AniList, TVmaze and Fandom (docs/DESIGN.md, Copy). English source, typed: `{{variables}}` are checked.
// Comments are notes for translators. A character's name, a series' title and an episode's label are the
// person's data: they come in as `{{values}}` and are never translated.
export default {
  /** The cast of a project, as a section: the page's title and the link back to it. */
  title: "Cast",
  /**
   * A character nobody has named yet. `n` is the order the characters were found in ("Character 12"),
   * written as it is, never grouped. Search finds it by this text and by the English "Character 12".
   */
  unnamed: "Character {{n}}",

  /** A number of things, said alone: a badge, a count under a heading, the line on a card. */
  count: {
    shots_one: "{{count}} shot",
    shots_other: "{{count}} shots",
    faces_one: "{{count}} face",
    faces_other: "{{count}} faces",
    characters_one: "{{count}} character",
    characters_other: "{{count}} characters",
    episodes_one: "{{count}} episode",
    episodes_other: "{{count}} episodes",
  },

  /** The title of the toast when a change to the cast fails; the reason follows it. */
  editFailed: "Couldn't change the cast",

  /** Finding someone in a big cast. */
  filters: {
    /** Names the group of controls. */
    group: "Find",
    /** Label of the search field. */
    search: "Find a character",
    /** Names the choice between everyone, the named and those to review. */
    show: "Show",
    all: "All",
    named: "Named",
    /** Only offered when someone has faces to review. */
    toReview: "To review",
    /** The orders of the cast; one is shown on the button, all in its menu. */
    order: {
      screen: "Most screen time",
      shots: "Most shots",
      name: "By name",
    },
    /** What the order button says to a screen reader; `order` is one of the three order names above. */
    orderBy: "Order: {{order}}",
  },

  /** The Cast page. */
  page: {
    /** "3 of 12 characters": how many of the cast a search or filter leaves. `shown` is already written in the language's digits. */
    narrowed_one: "{{shown}} of {{count}} character",
    narrowed_other: "{{shown}} of {{count}} characters",
    /** Follows the number of characters after a " · ", so it starts in lower case. `done` is how many files have their faces. */
    facesFromEpisodes_one: "faces from {{done}} of {{count}} episode",
    facesFromEpisodes_other: "faces from {{done}} of {{count}} episodes",
    /** The same for a film, which has files, not episodes. */
    facesFromFiles_one: "faces from {{done}} of {{count}} file",
    facesFromFiles_other: "faces from {{done}} of {{count}} files",
    /** Names the choice between the card grid and the list. */
    viewLayout: "View layout",
    mainCast: "Main cast",
    supportingCast: "Supporting and recurring",
    /** The heading that unfolds the hidden characters; their number follows it. */
    hidden: "Hidden",
    /** Title of the empty cast while an analysis runs. */
    looking: "Looking for characters",
    none: "No characters yet",
    noEpisodes: "Add episodes to this project. Characters appear here as they are analysed.",
    waitingForFaces: "Characters appear here as soon as the first episode's faces are analysed.",
    /** A search found nobody; `query` is what was typed. */
    noMatch: "Nobody matches “{{query}}”.",
    noNamed: "Nobody here has a name yet.",
    noReview: "No faces wait for review.",
    showEveryone: "Show everyone",
    /** Menu items of the page. */
    foldHidden: "Fold the hidden characters",
    showHidden: "Show the hidden characters",
    selectEvery: "Select every character",
  },

  /**
   * What the cast says of the user's decisions whose faces a later analysis didn't find again. They wait and are
   * tried again, so the sentence says both. `count` is how many decisions. The English "they" in the first form
   * is kept as it was.
   */
  unplaced_one:
    "{{count}} of your decisions waits for a face the latest analysis didn't find again. It's tried again at the next analysis.",
  unplaced_other:
    "{{count}} of your decisions wait for faces the latest analysis didn't find again. They're tried again at the next analysis.",

  /** Choosing cards or faces, then telling the choice what to do. */
  selection: {
    /** Menu item that starts or extends the selection. */
    select: "Select",
    /** Read out by a screen reader for a card that is selected. */
    selected: "Selected",
    selectAll: "Select all",
    /** In the bar of selected characters: also rescan the faces of nobody. */
    withEveryoneElse: "With Everyone else",
    /** Menu item and button name; the button itself says `clearShort`. */
    clear: "Clear selection",
    clearShort: "Clear",
    hide: "Hide",
    showInCast: "Show in the cast",
    /** Menu items on cards that are selected: `count` is how many characters. */
    hide_one: "Hide {{count}} character",
    hide_other: "Hide {{count}} characters",
    showInCast_one: "Show {{count}} character in the cast",
    showInCast_other: "Show {{count}} characters in the cast",
    /** Toast after hiding; its button takes them back. */
    hid_one: "Hid {{count}} character",
    hid_other: "Hid {{count}} characters",
  },

  /** A character as a card or a row. */
  card: {
    /** Their episode is being analysed again; the faces come back when it's done. */
    refinding: "Being found again…",
    /** A note: how many faces of this character wait for a yes or no. */
    toReview_one: "{{count}} to review",
    toReview_other: "{{count}} to review",
    /** The badge on a card in the grid: the same, with a plus. */
    moreToReview_one: "+{{count}} to review",
    moreToReview_other: "+{{count}} to review",
  },

  /** Faces and the review of them. */
  faces: {
    /** Button and menu item that open Review; `count` is how many faces wait. */
    review_one: "Review {{count}} face",
    review_other: "Review {{count}} faces",
    /** Menu item that moves the selected faces to another character; `count` is how many. */
    move_one: "Move {{count}} face to…",
    move_other: "Move {{count}} faces to…",
    /** The same on a button, where the selection's count is already shown beside it. */
    moveShort: "Move to…",
    notAFace: "Not a face",
    rescan: "Rescan",
    rescanTip: "Read these faces again at full quality",
    rescanConfirmed: "You confirmed these faces yourself",
    selectUnclear: "Select unclear faces",
    useAsPicture: "Use as picture",
    /** A face's name for a screen reader. */
    aria: "Face",
    /** `where` is the episode and the time in it. */
    ariaAt: "Face at {{where}}",
    /** In the picture-choosing dialog, the face that is the picture now. */
    ariaNow: "Face, their picture now",
    ariaAtNow: "Face at {{where}}, their picture now",
    /** A face that is marked as confirmed by a decision, for a screen reader. */
    confirmed: "Confirmed",
    watchMoment: "Watch this moment",
    /** Tooltip on a face in the grid. */
    selectHint: "Click to select · Double-click to watch",
    /** Tooltip on a face; on a card that plays the moment it is the line beside the time. */
    watchHint: "Double-click to watch",
    /** Tooltip on a face that can also play its moment where it is. */
    watchOrPlayHint: "Double-click to watch · Space plays it here",
    /** Stands for the episode's name when a face's episode has none. */
    thisFace: "This face",
    /** A face in an episode, for a screen reader; `where` is the episode's label. */
    ariaIn: "Face in {{where}}",
  },

  /** A second look at faces: suggestions, until the person answers. */
  rescan: {
    title: "Rescan",
    heading: "What Rescan found",
    nothingYet: "Nothing has changed yet. Every answer is yours.",
    startFailed: "Couldn't start Rescan",
    finished: "Rescan finished",
    show: "Show",
    left_one: "{{count}} suggestion left",
    left_other: "{{count}} suggestions left",
    leaveRest: "Leave the rest",
    leaveTitle: "Leave everything as it is?",
    leaveAbout: "The faces stay where they are. You can rescan them any time.",
    acceptClear_one: "Accept {{count}} clear match",
    acceptClear_other: "Accept {{count}} clear matches",
    acceptTitle_one: "Accept {{count}} face?",
    acceptTitle_other: "Accept {{count}} faces?",
    acceptAbout: "Each face goes to the character shown. Other unconfirmed faces may follow them.",
    acceptAnswer: "Accept",
    bar: {
      waiting: "Rescan waits for the analysis",
      starting: "Getting ready to rescan",
      reading: "Rescanning {{done}} of {{total}}",
      stop: "Stop",
    },
    waiting: {
      title_one: "Rescan has {{count}} suggestion for you",
      title_other: "Rescan has {{count}} suggestions for you",
      about: "Nothing changes until you say so.",
      open: "See them",
    },
    tally: {
      looked_one: "{{count}} face looked at",
      looked_other: "{{count}} faces looked at",
      read_one: "{{count}} read at full quality",
      read_other: "{{count}} read at full quality",
      right_one: "{{count}} already right",
      right_other: "{{count}} already right",
      unread_one: "{{count}} couldn't be read",
      unread_other: "{{count}} couldn't be read",
    },
    section: {
      pairs: "Same person?",
      faces: "Faces that look like someone",
      between: "Could be either",
      doubts: "Might not be them",
      noFace: "Maybe not faces",
    },
    lane: {
      lookLike_one: "{{count}} face looks like {{name}}",
      lookLike_other: "{{count}} faces look like {{name}}",
      doubt_one: "{{count}} face looks little like {{name}}",
      doubt_other: "{{count}} faces look little like {{name}}",
      noFace_one: "No face was found in {{count}} of these",
      noFace_other: "No face was found in these {{count}}",
      maybe: "Maybe",
      onNow: "On {{name}} now",
      leave: "Leave them",
      keep: "Keep it as it is",
      noneAreFaces: "None are faces",
    },
    choice: {
      neither: "Neither",
    },
    pair: {
      likely: "Likely one person",
      maybe: "Might be one person",
      question: "Are these two the same person?",
      facesMatch: "Their clearest faces match",
      facesClose: "Their clearest faces are close",
      oneCheck: "One check only: no second model knows this footage",
      secondAgrees: "A second model agrees",
      secondDoubts: "A second model has doubts",
      neverTogether: "Never in the same frame",
      together_one: "In the same frame {{count}} time",
      together_other: "In the same frame {{count}} times",
      same: "Same person",
      apart: "Two people",
    },
    done: {
      title: "Nothing left to decide",
      about: "Select faces or characters and choose Rescan for a closer look.",
    },
  },

  /** Everyone nobody claimed. */
  everyoneElse: {
    title: "Everyone else",
    open: "Open everyone else",
    /** `count` is how many faces; the line is under the card's title in the list. */
    unclaimed_one: "{{count}} face nobody claimed",
    unclaimed_other: "{{count}} faces nobody claimed",
    /** Under the title of the hero on their page. */
    about: "Extras, background faces and anyone seen too briefly to tell",
    /** The heading over their faces. */
    heading: "Faces nobody claimed",
  },

  /** A character's page. */
  character: {
    /** The page for a character that was merged away. */
    goneTitle: "This character isn't in the cast any more",
    goneDescription: "It may have been merged into someone else.",
    backToCast: "Back to the cast",
    /** Names the choice between a character's shots and their faces. */
    show: "Show",
    shots: "Shots",
    faces: "Faces",
    showFaces: "Show their faces",
    showShots: "Show their shots",
    selectEveryFace: "Select every face",
    noFaces: "No faces here.",
    /** Beside the choice between shots and faces, when faces are shown. */
    facesHint: "Click to select · Shift for a range · double-click to watch",
    /** Names the button: "Not Walter". `name` is the character's name or "Character 3". */
    not: "Not {{name}}",
    /** The toast after saying a shot isn't them. */
    notThemToast: "Not {{name}}: out of their shots",
    notAPersonToast: "Not a person: out of their shots",
    /** Items every character offers wherever it is shown. */
    open: "Open character",
    buildPack: "Build a scenepack",
    rename: "Rename",
    choosePicture: "Choose picture…",
    samePersonAs: "Same person as…",
    hideFromCast: "Hide from the cast",
    /** Name of the ⋯ button on a character's page; `name` is the character's. */
    options: "{{name}} options",
  },

  /** A character's shots (what a scenepack of them is made of). */
  shots: {
    loadFailed: "Their shots couldn't be loaded",
    /** Their episode is analysed again. */
    waiting: "Their episode is being analysed again; the shots come back when it's done.",
    none: "No shots yet.",
    /** `length` is how long all the shots last, already written ("1:20"). */
    summary_one: "{{count}} shot · {{length}} in all · hover to scrub",
    summary_other: "{{count}} shots · {{length}} in all · hover to scrub",
  },

  /** The hero of a character's page. */
  header: {
    /** Tooltip of the portrait button, which opens the choice of picture. */
    choosePictureTip: "Choose picture",
    /** A badge; `time` is written like "12 min". */
    onScreen: "{{time}} on screen",
    /** Beside the badges of a character nobody named. */
    clickToRename: "Click the name to rename",
  },

  /** Faces that look like a character but need a yes or no. */
  maybe: {
    /** The heading; `name` is the character's. */
    title: "Maybe {{name}}?",
    /** `count` is how many faces. */
    lookLike_one: "{{count}} face looks like {{name}}, but not clearly enough to decide alone.",
    lookLike_other: "{{count}} faces look like {{name}}, but not clearly enough to decide alone.",
    reviewOneByOne: "Review one by one",
    /** Button and menu item that give every one of these faces to the character. */
    allAre: "All are {{name}}",
    /** On one face: yes, it is them. */
    yes: "Yes, {{name}}",
    showFewer: "Show fewer",
    /** `count` is how many faces are still hidden. */
    showMore_one: "Show {{count}} more face",
    showMore_other: "Show {{count}} more faces",
  },

  /** Choosing a character, or starting a new one. */
  pick: {
    /** The title of the choice of who a character is the same person as; `name` is the character's. */
    mergeTitle: "{{name}} is the same person as…",
    mergeDescription: "Every face of both becomes one character. It keeps a name if either has one.",
    /** Asked before merging for good. */
    mergeQuestion: "Merge {{from}} into {{into}}?",
    mergeDetail: "This can't be undone. Faces you said aren't them stay apart.",
    /** The button that merges, here and on the names banner. */
    merge: "Merge",
    back: "Back",
    search: "Search",
    searchOrNew: "Search, or type a new name",
    /** The row that starts a new character; with a name typed, it names it. */
    newCharacter: "New character",
    newCharacterNamed: "New character “{{name}}”",
    nobody: "Nobody matches.",
  },

  /** Choosing the picture of a character from their faces. */
  choosePicture: {
    title: "Choose a picture for {{name}}",
    description: "Their clearest faces. Click one to make it their picture.",
    loadFailed: "Their faces couldn't be loaded.",
    none: "No faces of theirs yet.",
    /** Tooltip hint on the face that is the picture now. */
    now: "Their picture now",
    showMore: "Show more faces",
  },

  /** Renaming a character. */
  rename: {
    title: "Rename {{name}}",
    description: "Give this character a name, or clear it to keep their numbered label.",
    nameLabel: "Character name",
    save: "Save",
  },

  /** Review: faces the analysis wasn't sure about, one at a time. */
  review: {
    title: "Review faces",
    /** Shown when nothing is left. */
    allDone: "All reviewed",
    nothing: "Nothing to review",
    /** `count` is how many faces were skipped. The English "they" is kept in both forms. */
    skipped_one: "{{count}} face skipped; they stay in the list for later.",
    skipped_other: "{{count}} faces skipped; they stay in the list for later.",
    /** `count` is how many decisions were saved. The English "They" is kept in both forms. */
    saved_one: "{{count}} decision saved. They stick, whatever later analyses find.",
    saved_other: "{{count}} decisions saved. They stick, whatever later analyses find.",
    waiting: "Faces the analysis isn't sure about show up here.",
    showSkipped: "Show skipped faces",
    /** How far the review is: "3 of 12". */
    progress: "{{done}} of {{total}}",
    /** After the progress, beside a " · ". */
    skippedCount_one: "{{count}} skipped",
    skippedCount_other: "{{count}} skipped",
    /** The question over the face; the name is set in italic when it isn't a given name. */
    isThis: "Is this <x>{{name}}</x>?",
    whoIsThis: "Who is this?",
    /** Under the question; the analysis suggested someone. */
    lookALot: "Looks a lot like them — the analysis wasn't sure enough to decide alone.",
    mightBe: "Might be them — the analysis wasn't sure enough to decide alone.",
    /** Describes the picture of the face to decide on. */
    faceAlt: "The face to decide on",
    watch: "Watch",
    yes: "Yes, {{name}}",
    no: "No, someone else",
    skip: "Skip",
    skipForNow: "Skip for now",
    orSomeoneElse: "Or someone else",
    /** Menu item: this face is that character. */
    itsThem: "It's {{name}}",
  },

  /** Names for the cast from AniList (anime), TVmaze (animation) or a film's Fandom wiki. */
  names: {
    /** The button, the menu item and the dialog's first title; `source` is the service's name. */
    fromSource: "Names from {{source}}",
    /** The dialog's title once the series or film is chosen. */
    thisFilm: "This film?",
    thisSeries: "This series?",
    suggestions: "Name suggestions",
    /** Description of the dialog, for a film; the credit the licence asks for follows. */
    descriptionFilm:
      "Wikidata and Fandom see only what you send: the search text, then the cast and images of the film you confirm. {{credit}}",
    /** The same for a series; `source` is the service's name. */
    descriptionSeries:
      "{{source}} sees only what you send: the search text, then the cast and images of the series you confirm. {{credit}}",
    /** Credits, as the licence asks them to be said. */
    creditTvmaze: "Names and pictures: TVmaze.com (CC BY-SA).",
    creditFandom: "Names and pictures: the film's Fandom wiki (CC BY-SA), found through Wikidata.",
    searchFilm: "Film to search for",
    searchSeries: "Series to search for",
    /** Names the search field. */
    searchLabel: "Search text",
    search: "Search",
    /** `n` is how many episodes the series has. */
    episodesShort: "{{n}} ep",
    /** Nothing was found for what was typed; `query` is that text. */
    nothingFilm: "Nothing found for “{{query}}”: only a film whose franchise has a Fandom wiki can be named.",
    nothingSeries: "{{source}} has nothing for “{{query}}”.",
    /** Only shown for two or more, so the English has one wording for both forms. */
    chain_one: "The whole chain is kept ({{count}} series — sequels share their cast):",
    chain_other: "The whole chain is kept ({{count}} series — sequels share their cast):",
    chainOne: "Its cast is matched to your characters:",
    matchCast: "Match the cast",
    noMatch: "No portrait matched a character closely enough.",
    clickWrong: "Click one that's wrong to leave it out.",
    /** Suggested merge of two characters: `from` and `into` are their names. */
    merge: "Merge {{from}} into {{into}}",
    notNow: "Not now",
    apply: "Apply",
    /** `n` is how many suggestions are kept. */
    applyN: "Apply {{n}}",
    /** Toast title when the service didn't answer. */
    couldntAnswer: "{{source}} couldn't answer",
    /** Toast after names were given; `count` is how many characters. */
    named_one: "Named {{count}} character from {{source}}",
    named_other: "Named {{count}} characters from {{source}}",
    applied: "Names applied",
    merged: "Characters merged",
    /** Stands for a character that is no longer in the cast, inside a sentence ("Merge a character into ..."). */
    aCharacter: "a character",
    /** The same at the start of a sentence. */
    aCharacterStart: "A character",
    /** Stands for the second character of a suggested merge that is no longer in the cast. */
    another: "another",
    /** The banner's headline; `source` is the service's name. */
    hasNames_one: "{{source}} has names for {{count}} character",
    hasNames_other: "{{source}} has names for {{count}} characters",
    seesOne: "{{source}} sees one person in two characters",
    /** Under the headline; `series` is the series' title. */
    fromCast: "From the cast of {{series}}. Click one that's wrong to leave it out.",
    fromCastUnknown: "From the cast of the series. Click one that's wrong to leave it out.",
    wrongFilm: "Wrong film…",
    wrongSeries: "Wrong series…",
    giveNames: "Give names",
    /** `n` is how many names are kept. */
    giveN: "Give {{n}}",
    /** A suggested merge: "Skye is Marshall"; both are character names. */
    same: "{{from}} is {{into}}",
    /** Shown while the app looks the names up by itself. */
    looking: "Looking up the characters' names on {{source}}…",
    /** Toast titles when an answer failed; the reason follows. */
    giveFailed: "The names weren't given",
    dismissFailed: "The names are still waiting",
    rejectFailed: "The series wasn't turned down",
  },
} as const
