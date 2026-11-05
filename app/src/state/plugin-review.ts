import { toast } from "sonner"
import { create } from "zustand"
import type { PluginReview } from "@/bindings"
import { t } from "@/lib/i18n"
import { commands, errorMessage, isAppError } from "@/lib/ipc"

/** A read that takes longer than this is given up on, so one stuck package can't hold up every later one. */
export const READ_LIMIT_MS = 60_000

/**
 * What a read for a review is, besides its load: `label` names it when it
 * fails (its file or plugin). A package got from the catalog (`get`) shows
 * in the dialog while it comes, with how far it is and a Cancel. `install`
 * sees the review before it shows, and takes care of it when it says so (an
 * update that asks for nothing new installs in one click): then it doesn't
 * show. It says "cancelled" when the person stopped that.
 */
export type ReviewAsk = {
  label: string
  get?: { pluginId: string; cancel: () => void }
  install?: (review: PluginReview) => Promise<boolean | "cancelled">
}

/**
 * A package on its way from the catalog, as the dialog shows it; `installing`
 * once it is read and installs without its review, as the ticket `ticketId`.
 */
export type Getting = {
  pluginId: string
  label: string
  done: number | null
  total: number | null
  installing: boolean
  ticketId?: string
}

/**
 * How a read for a review came out: its review `shown`, `done` without it
 * (its `install` took care of it), `failed` (said in a toast), `cancelled`
 * by the person, or `skipped` because the same plugin's get already waits
 * or comes.
 */
export type ReviewOutcome = "shown" | "done" | "failed" | "cancelled" | "skipped"

type Waiting = ReviewAsk & { load: () => Promise<PluginReview>; key: string | null; settled: (outcome: ReviewOutcome) => void }

/** Whether a get of `pluginId` from the catalog waits or comes: its buttons wait for it. */
export const getsPlugin = (s: { queue: ReadonlyArray<Pick<ReviewAsk, "get">>; getting: Getting | null }, pluginId: string) =>
  s.getting?.pluginId === pluginId || s.queue.some((w) => w.get?.pluginId === pluginId)

type PluginReviewState = {
  /** The review being asked about; the install dialog shows it. */
  review: PluginReview | null
  /** A package or folder is being read for the next review. */
  loading: boolean
  /** Reads for reviews that wait for the one shown now. */
  queue: Waiting[]
  /** The catalog package being got for the next review, while it comes. */
  getting: Getting | null
  /** What the one read or shown now was asked for with (its path), while it is read or shown. */
  current: string | null
  /** The ticket whose plugin compiles now (`plugin:compiling`), past the person's yes. */
  compiling: string | null
  /**
   * Reads a package or folder for a review (`label` names it when that
   * fails: its file or plugin) and shows it. Reviews come one at a time: one
   * asked for while another is read or shown waits until that one is closed.
   * The promise ends when this one has been read, or failed to be, not when
   * it is closed, with how it came out. A get of a plugin whose get already
   * waits or comes is skipped: a second click doesn't ask twice. `key` (its
   * path) makes a second ask for the same one, while it waits, is read or is
   * shown, skipped too: see `pending`.
   */
  openReview: (load: () => Promise<PluginReview>, ask: string | ReviewAsk, key?: string) => Promise<ReviewOutcome>
  /** Whether the one asked for with `key` waits, is read or is shown now. */
  pending: (key: string) => boolean
  /** How far the package of `pluginId` came (`catalog:progress`), in bytes. */
  progress: (pluginId: string, done: number | null, total: number | null) => void
  /** Ends the get under way (it fails as cancelled, which says nothing). */
  cancelGet: () => void
  /** The plugin of `ticketId` started to compile (`plugin:compiling`). */
  compileStarted: (ticketId: string) => void
  /** The install that compiled ended, in whatever way. */
  compileEnded: () => void
  /** Ends the compile under way: its install fails as cancelled. */
  stopCompile: () => void
  /**
   * Ends the review shown and shows the next. Its ticket goes with it, the
   * files it unpacked among them, unless the install took it.
   */
  close: (opts?: { installed?: boolean }) => void
}

/**
 * The plugin install review can be opened from the Plugins page, a plugin's
 * settings, and by dropping a package on the window: one dialog, mounted in
 * the shell, shows whichever comes.
 */
export const usePluginReview = create<PluginReviewState>((set, get) => {
  /** The read under way: its get's Cancel. */
  let reading: Waiting | null = null
  /** Reads the next waiting one, when nothing is read or shown. */
  const next = () => {
    const { review, loading, queue } = get()
    if (review || loading || queue.length === 0) return
    const [head, ...rest] = queue
    reading = head
    const getting = head.get ? { pluginId: head.get.pluginId, label: head.label, done: null, total: null, installing: false } : null
    set({ loading: true, queue: rest, getting, current: head.key })
    // `async` so that a load which throws at once is a failed one.
    const loaded = (async () => head.load())()
    let gaveUp = false
    let timer: ReturnType<typeof setTimeout> | undefined
    // A download shows how far it came and can be cancelled: it may take
    // longer than a read, and the person decides when it is too long.
    const limit = head.get
      ? new Promise<never>(() => {})
      : new Promise<never>((_, reject) => {
          timer = setTimeout(() => {
            gaveUp = true
            reject(new Error(t("shell:pluginReview.tookTooLong")))
          }, READ_LIMIT_MS)
        })
    // A read that ends after it was given up on has staged a ticket nobody will see.
    loaded.then(
      (late) => {
        if (gaveUp) void commands.pluginDiscard(late.ticketId).catch(() => undefined)
      },
      () => undefined,
    )
    void Promise.race([loaded, limit]).then(
      async (read) => {
        clearTimeout(timer)
        let installed: boolean | "cancelled" = false
        if (head.install) {
          set((s) => ({ getting: s.getting && { ...s.getting, installing: true, ticketId: read.ticketId } }))
          installed = await head.install(read).catch(() => false)
        }
        const done = installed !== false
        reading = null
        set({ review: done ? null : read, loading: false, getting: null })
        head.settled(installed === "cancelled" ? "cancelled" : done ? "done" : "shown")
        if (done) next()
      },
      (e: unknown) => {
        clearTimeout(timer)
        reading = null
        set({ loading: false, getting: null })
        // Cancelled by the person: nothing went wrong.
        const cancelled = isAppError(e) && e.code === "cancelled"
        if (!cancelled) toast.error(t("shell:pluginReview.openFailed", { label: head.label }), { description: errorMessage(e) })
        head.settled(cancelled ? "cancelled" : "failed")
        next()
      },
    )
  }
  const pending = (key: string) => {
    const { review, loading, queue, current } = get()
    return ((review != null || loading) && current === key) || queue.some((w) => w.key === key)
  }
  return {
    review: null,
    loading: false,
    queue: [],
    getting: null,
    current: null,
    compiling: null,
    pending,
    openReview: (load, ask, key) =>
      new Promise<ReviewOutcome>((settled) => {
        const what = typeof ask === "string" ? { label: ask } : ask
        if (what.get && getsPlugin(get(), what.get.pluginId)) return settled("skipped")
        // The same package asked for again (opened twice) is not reviewed twice.
        if (key != null && pending(key)) return settled("skipped")
        set((s) => ({ queue: [...s.queue, { ...what, key: key ?? null, load, settled }] }))
        next()
      }),
    progress: (pluginId, done, total) =>
      set((s) => (s.getting?.pluginId === pluginId && !s.getting.installing ? { getting: { ...s.getting, done, total } } : {})),
    cancelGet: () => {
      if (get().getting?.installing) return
      reading?.get?.cancel()
    },
    compileStarted: (ticketId) => set({ compiling: ticketId }),
    compileEnded: () => set({ compiling: null }),
    stopCompile: () => {
      const { compiling } = get()
      if (compiling != null) void commands.pluginCompileCancel(compiling).catch(() => undefined)
    },
    close: (opts) => {
      const { review } = get()
      if (!review) return
      // One gone already is no error; one left goes when it expires.
      if (!opts?.installed) void commands.pluginDiscard(review.ticketId).catch(() => undefined)
      set({ review: null })
      next()
    },
  }
})
