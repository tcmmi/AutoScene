// The export namespace: the words of the export dialog (a clip, or several), and the ones the scenepack's
// export dialog shares with it (`pack` reads them as `export:…`). English source, typed: `{{variables}}` are checked.
// Comments are notes for the translator.
export default {
  /** The title of the dialog for one clip. */
  title: "Export clip",
  /** The title of the dialog for several clips; `count` is 2 or more and only picks the plural, `total` is the number written (never grouped, as this always read). */
  titleMany_one: "Export {{total}} clip",
  titleMany_other: "Export {{total}} clips",
  /** Under the title, when several clips go out at once. For one clip it shows where the clip is from. */
  description: "Each clip is cut frame-exact and checked against the source.",
  field: {
    /** The label of the field that names the one clip's file. */
    fileName: "File name",
    /** The label of the format choice (ProRes, H.264…). */
    format: "Format",
    /** A screen reader's name for the format choice. */
    formatAria: "Export format",
    /** Shown in the format choice until one is picked. */
    formatPlaceholder: "Choose a format",
    /** The label of the folder the export goes to. */
    destination: "Destination",
  },
  option: {
    /** A switch: crop the black bars a film was mastered with off its clips. */
    removeBars: "Remove black bars",
    /** Under that switch. */
    removeBarsHint: "Cuts off the bars built into the picture.",
    /** A switch: bring every clip to the same loudness. */
    matchLoudness: "Match loudness",
    /** Under that switch. */
    matchLoudnessHint: "Brings every clip to the same volume.",
  },
  /** The button that picks another folder for the export. */
  change: "Change…",
  /** The title of the OS folder dialog for the export's destination. */
  pickDestination: "Export to",
  /**
   * Under the format choice: how big the export gets in total. `size` is written ("12.3 GB"); `format` is the
   * format's short name ("ProRes 422 HQ").
   */
  about: "About {{size}} in all as {{format}}.",
  /** The submit button. */
  submit: "Export",
  /** The toast that says an export is on its way, for one clip; `name` is its file name, in quotation marks. */
  started: "Exporting “{{name}}”",
  /** What stands for the clip's name in the toast when it has none yet (it sits inside the quotation marks). */
  theClip: "the clip",
  /** The toast for several clips; `count` is 2 or more and only picks the plural, `total` is the number written (never grouped). */
  startedMany_one: "Exporting {{total}} clip",
  startedMany_other: "Exporting {{total}} clips",
  /** The toast's button that opens Activity, where the export runs. */
  show: "Show",
  /** The toast when AutoScene refuses to start the export; the reason follows under it. */
  startFailed: "Couldn't start the export",
} as const
