import { renderHook, waitFor } from "@testing-library/react"
import { toast } from "sonner"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { PluginReview } from "@/bindings"
import { commands, events } from "@/lib/ipc"
import { usePluginReview } from "@/state/plugin-review"
import { pluginInfo } from "@/test/plugins"
import { useOpenedPackages } from "./use-opened-packages"

/** What `plugins:opened` calls, once the hook listens. */
let opened: (() => void) | null = null

vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return {
    ...ipc,
    commands: {
      pluginsTakeOpened: vi.fn(),
      pluginInspect: vi.fn(),
      pluginDiscard: vi.fn(() => Promise.resolve(null)),
    },
    events: {
      ...ipc.events,
      pluginsOpened: {
        listen: vi.fn(async (f: () => void) => {
          opened = f
          return () => (opened = null)
        }),
      },
    },
  }
})
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn(), loading: vi.fn(() => "tid"), dismiss: vi.fn() }) }))

const review = (path: string): PluginReview => ({
  ticketId: path,
  plugin: pluginInfo({ id: `dev.example.${path.length}`, name: path }),
  update: null,
  needsUnreviewed: false,
  allowed: true,
  uvMissing: false,
  uvBytes: null,
  resourceBytes: null,
  resourceHosts: [],
  catalog: null,
  componentWarnings: [],
})

const store = () => usePluginReview.getState()

beforeEach(() => {
  vi.clearAllMocks()
  opened = null
  usePluginReview.setState({ review: null, loading: false, queue: [], current: null })
  vi.mocked(commands.pluginInspect).mockImplementation((path: string) => Promise.resolve(review(path)))
})

describe("useOpenedPackages", () => {
  it("packages opened before the page listened are reviewed one at a time", async () => {
    const first = String.raw`C:\Downloads\a.asp`
    const second = String.raw`C:\Downloads\b.asp`
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValueOnce([first, second]).mockResolvedValue([])

    const { unmount } = renderHook(() => useOpenedPackages())
    // Taken once the hook listens, without an event.
    await waitFor(() => expect(store().review?.ticketId).toBe(first))
    expect(commands.pluginsTakeOpened).toHaveBeenCalledOnce()
    // As for a drop, the window says what it reads and what waits.
    expect(toast.loading).toHaveBeenCalledExactlyOnceWith("Reading a.asp… 1 more wait their turn")
    expect(toast.dismiss).toHaveBeenCalledWith("tid")
    // The second waits for the first: not even read yet.
    expect(commands.pluginInspect).toHaveBeenCalledTimes(1)
    expect(store().queue).toHaveLength(1)

    store().close()
    await waitFor(() => expect(store().review?.ticketId).toBe(second))
    expect(commands.pluginInspect).toHaveBeenLastCalledWith(second)
    // Unmounted, it stops listening.
    unmount()
    await waitFor(() => expect(opened).toBeNull())
  })

  it("takes what is opened later, when AutoScene says so", async () => {
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValue([])
    renderHook(() => useOpenedPackages())
    await waitFor(() => expect(commands.pluginsTakeOpened).toHaveBeenCalledOnce())
    expect(store().review).toBeNull()

    const later = "/home/you/c.asp"
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValueOnce([later])
    opened?.()
    await waitFor(() => expect(store().review?.ticketId).toBe(later))

    // One opened while that review is shown waits for it, and the window says so.
    const third = "/home/you/d.asp"
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValueOnce([third])
    opened?.()
    await waitFor(() => expect(toast).toHaveBeenCalledWith("d.asp waits for the review open now"))
    expect(store().queue).toHaveLength(1)
  })

  it("a package opened again while its review is shown is not reviewed twice", async () => {
    const pkg = String.raw`C:\Downloads\a.asp`
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValueOnce([pkg]).mockResolvedValue([])
    renderHook(() => useOpenedPackages())
    await waitFor(() => expect(store().review?.ticketId).toBe(pkg))

    // A second double-click on the same file only brings the window up.
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValueOnce([pkg])
    opened?.()
    await waitFor(() => expect(toast).toHaveBeenCalledWith("a.asp is already being reviewed"))
    expect(store().queue).toHaveLength(0)
    store().close()
    expect(store().review).toBeNull()
    expect(store().loading).toBe(false)
    expect(commands.pluginInspect).toHaveBeenCalledOnce()
  })

  it("takes only once it listens, so a package opened in between is taken by one or the other", async () => {
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValue([])
    let listening: (un: () => void) => void = () => {}
    vi.mocked(events.pluginsOpened.listen).mockReturnValueOnce(new Promise((resolve) => (listening = resolve)))
    renderHook(() => useOpenedPackages())
    await waitFor(() => expect(events.pluginsOpened.listen).toHaveBeenCalledOnce())
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(commands.pluginsTakeOpened).not.toHaveBeenCalled()

    listening(() => undefined)
    await waitFor(() => expect(commands.pluginsTakeOpened).toHaveBeenCalledOnce())
  })

  it("still takes what was opened when it can't listen", async () => {
    const pkg = "/home/you/a.asp"
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValueOnce([pkg]).mockResolvedValue([])
    vi.mocked(events.pluginsOpened.listen).mockRejectedValueOnce(new Error("no events"))
    renderHook(() => useOpenedPackages())
    await waitFor(() => expect(store().review?.ticketId).toBe(pkg))
    expect(commands.pluginsTakeOpened).toHaveBeenCalledOnce()
  })

  it("names a package that can't be read by its file", async () => {
    vi.mocked(commands.pluginsTakeOpened).mockResolvedValueOnce([String.raw`D:\Plugins\broken.asp`]).mockResolvedValue([])
    vi.mocked(commands.pluginInspect).mockRejectedValueOnce({ code: "invalid", message: "This isn't an AutoScene plugin package." })
    renderHook(() => useOpenedPackages())
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Couldn't open broken.asp", {
        description: "This isn't an AutoScene plugin package.",
      }),
    )
    expect(store().review).toBeNull()
  })
})
