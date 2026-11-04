import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { PluginReview } from "@/bindings"
import { commands } from "@/lib/ipc"
import { pluginInfo } from "@/test/plugins"
import { READ_LIMIT_MS, usePluginReview } from "./plugin-review"

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: { pluginDiscard: vi.fn(() => Promise.resolve(null)) },
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }))

const review = (ticketId: string): PluginReview => ({
  ticketId,
  plugin: pluginInfo({ id: `dev.example.${ticketId}`, name: ticketId }),
  update: null,
  needsUnreviewed: false,
  allowed: true,
  uvMissing: false,
  catalog: null,
})

const store = () => usePluginReview.getState()

beforeEach(() => {
  vi.clearAllMocks()
  usePluginReview.setState({ review: null, loading: false, queue: [] })
})
afterEach(() => vi.useRealTimers())

describe("the install review store", () => {
  it("shows a review once it is read, and is loading until then", async () => {
    let read: (r: PluginReview) => void = () => {}
    const settled = store().openReview(() => new Promise((resolve) => (read = resolve)), "a.asp")
    expect(store().loading).toBe(true)
    expect(store().review).toBeNull()
    read(review("a"))
    await settled
    expect(store().loading).toBe(false)
    expect(store().review?.ticketId).toBe("a")
  })

  it("a second review waits for the first", async () => {
    const second = vi.fn(() => Promise.resolve(review("b")))
    await store().openReview(() => Promise.resolve(review("a")), "a.asp")
    // Asked for while the first is shown: it is not even read yet.
    const waiting = store().openReview(second, "b.asp")
    await Promise.resolve()
    expect(second).not.toHaveBeenCalled()
    expect(store().review?.ticketId).toBe("a")

    store().close({ installed: true })
    await waiting
    expect(second).toHaveBeenCalledOnce()
    expect(store().review?.ticketId).toBe("b")

    // One asked for while another is still being read waits for it, and for it to be closed.
    usePluginReview.setState({ review: null })
    let read: (r: PluginReview) => void = () => {}
    const slow = store().openReview(() => new Promise((resolve) => (read = resolve)), "c.asp")
    const third = vi.fn(() => Promise.resolve(review("d")))
    const queued = store().openReview(third, "d.asp")
    read(review("c"))
    await slow
    expect(third).not.toHaveBeenCalled()
    expect(store().review?.ticketId).toBe("c")
    store().close({ installed: true })
    await queued
    expect(store().review?.ticketId).toBe("d")
  })

  it("closing discards the ticket", async () => {
    await store().openReview(() => Promise.resolve(review("a")), "a.asp")
    store().close()
    expect(commands.pluginDiscard).toHaveBeenCalledExactlyOnceWith("a")
    expect(store().review).toBeNull()

    // An installed one is taken already: nothing to discard.
    await store().openReview(() => Promise.resolve(review("b")), "b.asp")
    store().close({ installed: true })
    expect(commands.pluginDiscard).toHaveBeenCalledOnce()
    expect(store().review).toBeNull()

    // A ticket that is gone already is no error.
    vi.mocked(commands.pluginDiscard).mockRejectedValueOnce({ code: "not_found", message: "Closed." })
    await store().openReview(() => Promise.resolve(review("c")), "c.asp")
    store().close()
    await Promise.resolve()
    expect(store().review).toBeNull()

    // Nothing shown, nothing to close.
    store().close()
    expect(commands.pluginDiscard).toHaveBeenCalledTimes(2)
  })

  it("gives up on a read that never ends, so the next is still read", async () => {
    vi.useFakeTimers()
    let late: (r: PluginReview) => void = () => {}
    const stuck = store().openReview(() => new Promise((resolve) => (late = resolve)), "usb.asp")
    const next = store().openReview(() => Promise.resolve(review("b")), "b.asp")
    await vi.advanceTimersByTimeAsync(READ_LIMIT_MS)
    await Promise.all([stuck, next])
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Couldn't open usb.asp", { description: expect.stringContaining("too long") })
    expect(store().review?.ticketId).toBe("b")
    expect(store().loading).toBe(false)

    // The stuck read ends after all: its ticket is nobody's, and goes.
    late(review("late"))
    await vi.advanceTimersByTimeAsync(0)
    expect(commands.pluginDiscard).toHaveBeenCalledExactlyOnceWith("late")
    expect(store().review?.ticketId).toBe("b")
  })

  it("names what it couldn't read, and goes on to the next", async () => {
    const next = vi.fn(() => Promise.resolve(review("b")))
    const bad = store().openReview(() => Promise.reject({ code: "invalid", message: "Not a plugin package." }), "bad.asp")
    const good = store().openReview(next, "b.asp")
    await Promise.all([bad, good])
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Couldn't open bad.asp", { description: "Not a plugin package." })
    expect(store().review?.ticketId).toBe("b")
    expect(store().loading).toBe(false)

    // A load that throws before it returns a promise fails the same way.
    store().close({ installed: true })
    await store().openReview(() => {
      throw new Error("Boom")
    }, "boom.asp")
    expect(toast.error).toHaveBeenLastCalledWith("Couldn't open boom.asp", { description: expect.stringContaining("Boom") })
    expect(store().review).toBeNull()
  })
})
