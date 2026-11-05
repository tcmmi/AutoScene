// The ui namespace: the shared pieces in components/ui (docs/DESIGN.md, Copy). English source, typed: `{{variables}}` are checked.
// Comments are notes for translators.
export default {
  /** A view whose data couldn't be loaded (`ErrorState`). */
  errorState: {
    /** Heading when the view gave none. */
    title: "This couldn't be loaded",
    /** Shown when the failure has no words of AutoScene's own (a bug in the view). */
    generic: "Something went wrong while showing this.",
    /** Button that opens the folder with AutoScene's log files. */
    openLogs: "Open logs",
  },
  /** The pill that finds things in a list. */
  search: {
    /** The × inside a search field that has text: screen-reader name. */
    clear: "Clear the search",
  },
  /** A horizontal row of cards that scrolls: the arrows on hover. */
  shelf: {
    back: "Scroll back",
    on: "Scroll on",
  },
  /** The bar at the top of a view. */
  topbar: {
    /** Button and tooltip that bring the left-hand rail back while it is hidden. */
    showRail: "Show the rail",
    /** Button, tooltip and menu entry: `place` is the name of the screen it returns to ("Library", a project's title). */
    back: "Back to {{place}}",
  },
} as const
