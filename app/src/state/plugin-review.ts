import { toast } from "sonner"
import { create } from "zustand"
import type { PluginReview } from "@/bindings"
import { commands, errorMessage } from "@/lib/ipc"

/** A read that takes longer than this is given up on, so one stuck package can't hold up every later one. */
export const READ_LIMIT_MS = 60_000

type Waiting = { load: () => Promise<PluginReview>; label: string; settled: () => void }

type PluginReviewState = {
  /** The review being asked about; the install dialog shows it. */
  review: PluginReview | null
  /** A package or folder is being read for the next review. */
  loading: boolean
  /** Reads for reviews that wait for the one shown now. */
  queue: Waiting[]
  /**
   * Reads a package or folder for a review (`label` names it when that
   * fails: its file or plugin) and shows it. Reviews come one at a time: one
   * asked for while another is read or shown waits until that one is closed.
   * The promise ends when this one has been read, or failed to be, not when
   * it is closed.
   */
  openReview: (load: () => Promise<PluginReview>, label: string) => Promise<void>
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
  /** Reads the next waiting one, when nothing is read or shown. */
  const next = () => {
    const { review, loading, queue } = get()
    if (review || loading || queue.length === 0) return
    const [head, ...rest] = queue
    set({ loading: true, queue: rest })
    // `async` so that a load which throws at once is a failed one.
    const loaded = (async () => head.load())()
    let gaveUp = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const limit = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        gaveUp = true
        reject(new Error("Reading it took too long. Is the file on a drive that is not connected?"))
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
      (read) => {
        clearTimeout(timer)
        set({ review: read, loading: false })
        head.settled()
      },
      (e: unknown) => {
        clearTimeout(timer)
        set({ loading: false })
        toast.error(`Couldn't open ${head.label}`, { description: errorMessage(e) })
        head.settled()
        next()
      },
    )
  }
  return {
    review: null,
    loading: false,
    queue: [],
    openReview: (load, label) =>
      new Promise<void>((settled) => {
        set((s) => ({ queue: [...s.queue, { load, label, settled }] }))
        next()
      }),
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
