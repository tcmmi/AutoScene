import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, renderHook, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { JobInfo, PackChoices, PackClip, PackFilter, PackPreview, PackSpec, PackState } from "@/bindings"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { DEFAULT_SPEC, totalsOf } from "./spec"
import { useActivity } from "@/state/activity"
import { packExportStarted, usePack, usePackExport } from "./use-pack"

vi.mock("@/lib/ipc", () => ({
  commands: {
    packGet: vi.fn(),
    packSave: vi.fn(() => Promise.resolve(null)),
    packPreview: vi.fn(),
    packPlan: vi.fn(() => Promise.resolve({ names: [], sizes: [], folder: null })),
    packOverride: vi.fn(),
    packFilterCheck: vi.fn(() => Promise.resolve(true)),
  },
  errorMessage: (e: unknown) => String(e),
}))

const clip = (start: number, included: boolean, manual: boolean | null = null, byRules = true): PackClip => ({
  shot: { mediaId: "m", start, end: start + 24, sprite: null, cells: 0, trackId: 1, face: "", faceHeight: 0.3 },
  start,
  end: start + 24,
  size: "close_up",
  quality: 1,
  coverage: 1,
  others: 0,
  score: 1,
  included,
  byRules,
  reason: included ? null : manual === false ? "removed" : "short",
  manual,
  filter: null,
})
/** The picks of one click, as the backend gets them. */
const picks = (...groups: [number[], boolean | null][]) =>
  groups.map(([starts, include]) => ({ shots: starts.map((shot) => ({ mediaId: "m", shot })), include }))
const preview = (clips: PackClip[], stamp: string, shape = "two shots"): PackPreview => ({
  clips,
  totals: totalsOf(clips, () => 24),
  stamp,
  shape,
  filters: [],
  unchecked: 0,
})
/** The backend's answer to a pick: the choices of `p`. */
const choicesOf = (p: PackPreview): PackChoices => ({
  choices: p.clips.map(({ included, byRules, reason, manual, filter }) => ({ included, byRules, reason, manual, filter })),
  totals: p.totals,
  stamp: p.stamp,
  shape: p.shape,
})
const state: PackState = { spec: DEFAULT_SPEC, naming: { template: "{tc}", folder: true, merge: true, file: "{character}" } }

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
)

/** The hook over a pack of two shots, both in, once it shows them. */
async function mounted() {
  vi.mocked(commands.packGet).mockResolvedValue(state)
  vi.mocked(commands.packPreview).mockResolvedValue(preview([clip(0, true), clip(100, true)], "both"))
  const hook = renderHook(() => usePack("p", "c", new Map()), { wrapper })
  await waitFor(() => expect(hook.result.current.current).toBe(true))
  return hook
}

afterEach(() => {
  cleanup()
  queryClient.clear()
  vi.clearAllMocks()
})

describe("usePack", () => {
  it("lets nothing be exported while a click is on its way, and never flips a card back", async () => {
    const { result } = await mounted()
    let answer: (a: PackChoices) => void = () => {}
    vi.mocked(commands.packOverride).mockImplementation(() => new Promise((ok) => (answer = ok)))

    // Taken out: at once on screen; no export until the backend has it.
    let click!: Promise<void>
    act(() => {
      click = result.current.setIncluded([result.current.preview.data!.clips[0]], false)
    })
    await waitFor(() => expect(result.current.preview.data?.clips[0].included).toBe(false))
    expect(result.current.current).toBe(false)

    // A refetch meanwhile (an event) brings the pack without the click: the card stays out.
    await act(() => queryClient.refetchQueries())
    expect(result.current.preview.data?.clips[0].included).toBe(false)
    expect(result.current.current).toBe(false)

    // The backend has it now: its answer is what an export takes.
    await act(async () => {
      answer(choicesOf(preview([clip(0, false, false), clip(100, true)], "one")))
      await click
    })
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(result.current.preview.data?.stamp).toBe("one")
    expect(result.current.preview.data?.clips[0]).toMatchObject({ included: false, manual: false })
    expect(commands.packOverride).toHaveBeenCalledWith("p", "c", picks([[0], false]), DEFAULT_SPEC)
  })

  it("fetches only the choices a click changed, and the plan once the clicks are in", async () => {
    const { result } = await mounted()
    const shots = vi.mocked(commands.packPreview).mock.calls.length
    const plans = vi.mocked(commands.packPlan).mock.calls.length
    vi.mocked(commands.packOverride).mockResolvedValue(choicesOf(preview([clip(0, false, false), clip(100, true)], "one")))
    await act(() => result.current.setIncluded([result.current.preview.data!.clips[0]], false))
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(result.current.preview.data?.stamp).toBe("one")
    // The shots aren't fetched again; the plan (names, size) is.
    expect(vi.mocked(commands.packPreview).mock.calls.length).toBe(shots)
    await waitFor(() => expect(vi.mocked(commands.packPlan).mock.calls.length).toBe(plans + 1))
  })

  it("fetches the whole pack when it changed otherwise meanwhile", async () => {
    const { result } = await mounted()
    // An analysis found a third shot meanwhile: the answer is of another pack.
    const now = preview([clip(0, false, false), clip(50, true), clip(100, true)], "three", "three shots")
    vi.mocked(commands.packOverride).mockResolvedValue(choicesOf(now))
    vi.mocked(commands.packPreview).mockResolvedValue(now)
    await act(() => result.current.setIncluded([result.current.preview.data!.clips[0]], false))
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(result.current.preview.data?.clips.map((c) => c.start)).toEqual([0, 50, 100])
    expect(result.current.preview.data?.stamp).toBe("three")
  })

  it("fetches the whole pack again when a fetch of it is on its way as the answer comes", async () => {
    const { result } = await mounted()
    let answer: (a: PackChoices) => void = () => {}
    vi.mocked(commands.packOverride).mockImplementation(() => new Promise((ok) => (answer = ok)))
    let click!: Promise<void>
    act(() => {
      click = result.current.setIncluded([result.current.preview.data!.clips[0]], false)
    })
    await waitFor(() => expect(commands.packOverride).toHaveBeenCalled())
    // An event asks for the whole pack meanwhile; it comes late, made before the pick.
    let late: (p: PackPreview) => void = () => {}
    vi.mocked(commands.packPreview).mockImplementationOnce(() => new Promise((ok) => (late = ok)))
    act(() => void queryClient.invalidateQueries())
    const after = preview([clip(0, false, false), clip(100, true)], "one")
    vi.mocked(commands.packPreview).mockResolvedValue(after)
    await act(async () => {
      answer(choicesOf(after))
      await click
      late(preview([clip(0, true), clip(100, true)], "both"))
    })
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(result.current.preview.data?.stamp).toBe("one")
    expect(result.current.preview.data?.clips[0].included).toBe(false)
  })

  it("refetches the pack of filters changed during a click from the start once the click is in", async () => {
    const { result } = await mounted()
    let answer: (a: PackChoices) => void = () => {}
    vi.mocked(commands.packOverride).mockImplementation(() => new Promise((ok) => (answer = ok)))
    let click!: Promise<void>
    act(() => {
      click = result.current.setIncluded([result.current.preview.data!.clips[0]], false)
    })
    await waitFor(() => expect(commands.packOverride).toHaveBeenCalled())
    // Other filters meanwhile: their pack's first fetch reads it before the click.
    const solo = { ...DEFAULT_SPEC, solo: true }
    let late: (p: PackPreview) => void = () => {}
    vi.mocked(commands.packPreview).mockImplementationOnce(() => new Promise((ok) => (late = ok)))
    act(() => result.current.setSpec(solo))
    await waitFor(() => expect(commands.packPreview).toHaveBeenCalledWith("p", "c", solo))
    vi.mocked(commands.packPreview).mockResolvedValue(preview([clip(0, false, false), clip(100, true)], "solo"))
    await act(async () => {
      answer(choicesOf(preview([clip(0, false, false), clip(100, true)], "one")))
      await click
      late(preview([clip(0, true), clip(100, true)], "before the click"))
    })
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(result.current.preview.data?.stamp).toBe("solo")
    expect(result.current.preview.data?.clips[0].included).toBe(false)
  })

  it("sends a click's picks and hand-backs at once, and keeps later clicks on screen until they are in", async () => {
    vi.mocked(commands.packGet).mockResolvedValue(state)
    vi.mocked(commands.packPreview).mockResolvedValue(preview([clip(0, false, false), clip(100, false, null, false)], "none"))
    const { result } = renderHook(() => usePack("p", "c", new Map()), { wrapper })
    await waitFor(() => expect(result.current.current).toBe(true))
    const answers: ((a: PackChoices) => void)[] = []
    vi.mocked(commands.packOverride).mockImplementation(() => new Promise((ok) => answers.push(ok)))

    // Both in: one taken out by hand goes back to the filters, the other is picked.
    let first!: Promise<void>
    act(() => {
      first = result.current.setIncluded(result.current.preview.data!.clips, true)
    })
    await waitFor(() => expect(answers.length).toBe(1))
    expect(commands.packOverride).toHaveBeenCalledWith("p", "c", picks([[100], true], [[0], null]), DEFAULT_SPEC)
    // Out again, before the first click is in: it waits for it.
    let second!: Promise<void>
    act(() => {
      second = result.current.setIncluded([result.current.preview.data!.clips[0]], false)
    })
    await act(async () => {
      answers[0](choicesOf(preview([clip(0, true), clip(100, true, true, false)], "both")))
      await first
    })
    await waitFor(() => expect(answers.length).toBe(2))
    // The first is in; the second still shows on screen, and nothing can be exported.
    expect(result.current.preview.data?.clips.map((c) => c.included)).toEqual([false, true])
    expect(result.current.current).toBe(false)
    await act(async () => {
      answers[1](choicesOf(preview([clip(0, false, false), clip(100, true, true, false)], "one")))
      await second
    })
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(result.current.preview.data?.stamp).toBe("one")
    expect(result.current.preview.data?.clips.map((c) => c.manual)).toEqual([false, true])
  })

  it("shows the pack as the backend has it when a click fails", async () => {
    const { result } = await mounted()
    const shots = vi.mocked(commands.packPreview).mock.calls.length
    vi.mocked(commands.packOverride).mockRejectedValue("This character isn't in the cast any more.")
    await act(() => result.current.setIncluded([result.current.preview.data!.clips[0]], false))
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(vi.mocked(commands.packPreview).mock.calls.length).toBe(shots + 1)
    expect(result.current.preview.data?.clips[0].included).toBe(true)
  })

  it("hands picks back to the filters the same way", async () => {
    vi.mocked(commands.packGet).mockResolvedValue(state)
    vi.mocked(commands.packPreview).mockResolvedValue(preview([clip(0, false, false), clip(100, true)], "one"))
    const { result } = renderHook(() => usePack("p", "c", new Map()), { wrapper })
    await waitFor(() => expect(result.current.current).toBe(true))
    vi.mocked(commands.packOverride).mockResolvedValue(choicesOf(preview([clip(0, true), clip(100, true)], "both")))
    await act(() => result.current.clearPicks(result.current.preview.data!.clips))
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(commands.packOverride).toHaveBeenCalledWith("p", "c", picks([[0], null]), DEFAULT_SPEC)
    expect(result.current.preview.data?.clips[0]).toMatchObject({ included: true, manual: null })
    expect(result.current.preview.data?.stamp).toBe("both")
  })

  it("hands picks back while the pack of new filters is on its way, then shows it as the backend has it", async () => {
    vi.mocked(commands.packGet).mockResolvedValue(state)
    vi.mocked(commands.packPreview).mockResolvedValue(preview([clip(0, false, false), clip(100, true)], "one"))
    const { result } = renderHook(() => usePack("p", "c", new Map()), { wrapper })
    await waitFor(() => expect(result.current.current).toBe(true))
    // New filters: their pack's first fetch reads it before the hand-back, and comes late.
    const solo = { ...DEFAULT_SPEC, solo: true }
    let late: (p: PackPreview) => void = () => {}
    vi.mocked(commands.packPreview).mockImplementationOnce(() => new Promise((ok) => (late = ok)))
    act(() => result.current.setSpec(solo))
    await waitFor(() => expect(commands.packPreview).toHaveBeenCalledWith("p", "c", solo))
    expect(result.current.ready).toBe(false)
    vi.mocked(commands.packOverride).mockResolvedValue(choicesOf(preview([clip(0, true), clip(100, true)], "both")))
    vi.mocked(commands.packPreview).mockResolvedValue(preview([clip(0, true), clip(100, true)], "both, solo"))
    await act(async () => {
      await result.current.clearPicks(result.current.preview.data!.clips)
      late(preview([clip(0, false, false), clip(100, true)], "before the hand-back"))
    })
    await waitFor(() => expect(result.current.current).toBe(true))
    expect(commands.packOverride).toHaveBeenCalledWith("p", "c", picks([[0], null]), solo)
    expect(result.current.preview.data?.stamp).toBe("both, solo")
    expect(result.current.preview.data?.clips[0]).toMatchObject({ included: true, manual: null })
  })
})

describe("the pack's export", () => {
  const job = (id: string, exportId: string, state: JobInfo["state"], rev = 1) =>
    ({ id, kind: "export", projectId: "p", exportId, state, progress: null, rev, createdAt: rev }) as JobInfo

  it("is the job of the export the pack started, while it waits or runs", () => {
    useActivity.setState({ jobs: {}, order: [] })
    const { result } = renderHook(() => usePackExport("p", "walter"))
    expect(result.current).toBeUndefined()
    act(() => {
      packExportStarted("p", "walter", "e1")
      // Another character's pack, and Walter's in another project.
      packExportStarted("p", "jesse", "e2")
      packExportStarted("q", "walter", "e3")
      for (const j of [job("j2", "e2", "running"), job("j3", "e3", "running"), job("j1", "e1", "queued")]) {
        useActivity.getState().upsert(j)
      }
    })
    expect(result.current?.id).toBe("j1")
    act(() => useActivity.getState().upsert(job("j1", "e1", "cancelled", 2)))
    expect(result.current).toBeUndefined()
  })

  it("is never a plugin's run on that export", () => {
    useActivity.setState({ jobs: {}, order: [] })
    const { result } = renderHook(() => usePackExport("p", "walter"))
    act(() => {
      packExportStarted("p", "walter", "e1")
      useActivity.getState().upsert({ ...job("j4", "e1", "running", 3), kind: "plugin" })
    })
    expect(result.current).toBeUndefined()
  })
})

describe("usePack, plugin filters", () => {
  // Each test says how a check starts.
  beforeEach(() => void vi.mocked(commands.packFilterCheck).mockReset().mockResolvedValue(true))
  const KEY = "dev.example.speech-filter/speaks"
  const on: PackSpec = { ...DEFAULT_SPEC, pluginFilters: [KEY] }
  const speaks = (more: Partial<PackFilter> = {}): PackFilter => ({
    key: KEY,
    title: "Speaks",
    description: null,
    reason: "No speech",
    plugin: "Speech Filter",
    checked: 0,
    total: 2,
    state: "ready",
    ...more,
  })
  /** The two shots, with the filter as it stands. */
  const judged = (filter: PackFilter, tag = ""): PackPreview => ({
    ...preview([clip(0, true), clip(100, true)], `${filter.state} ${filter.checked} ${tag}`),
    filters: [filter],
    unchecked: filter.total - filter.checked,
  })
  /** The hook with the filter turned on (`spec`), the pack as the backend shows it first. */
  async function open(first: PackFilter, spec: PackSpec = on) {
    vi.mocked(commands.packGet).mockResolvedValue({ ...state, spec })
    vi.mocked(commands.packPreview).mockResolvedValue(judged(first))
    const hook = renderHook(() => usePack("p", "c", new Map()), { wrapper })
    await waitFor(() => expect(hook.result.current.preview.data).toBeDefined())
    return hook
  }

  /** The check starts when asked: the pack read afterwards says it checks. */
  const startsWhenAsked = (now: PackFilter) =>
    vi.mocked(commands.packFilterCheck).mockImplementation(async () => {
      vi.mocked(commands.packPreview).mockResolvedValue(judged(now))
      return true
    })

  it("unchecked shots ask for one check", async () => {
    startsWhenAsked(speaks({ state: "checking", checked: 1 }))
    const { result } = await open(speaks())
    await waitFor(() => expect(commands.packFilterCheck).toHaveBeenCalledOnce())
    expect(commands.packFilterCheck).toHaveBeenCalledWith("p", "c", KEY, on, false)
    // The pack is read again to learn it checks now, and says so: no second ask.
    await waitFor(() => expect(result.current.check).toEqual({ kind: "checking", checked: 1, total: 2 }))
    expect(commands.packFilterCheck).toHaveBeenCalledOnce()
  })

  it("asks again only once something changed, never for the same answer twice", async () => {
    // One shot of two judged, and the check has nothing more to start.
    const { result } = await open(speaks({ checked: 1 }))
    await waitFor(() => expect(commands.packFilterCheck).toHaveBeenCalledOnce())
    // The pack changes otherwise meanwhile (an analysis): the filter's numbers are what they were.
    for (const tag of ["a", "b"]) {
      vi.mocked(commands.packPreview).mockResolvedValue(judged(speaks({ checked: 1 }), tag))
      await act(() => queryClient.invalidateQueries())
      await waitFor(() => expect(result.current.preview.data?.stamp).toBe(`ready 1 ${tag}`))
    }
    expect(commands.packFilterCheck).toHaveBeenCalledOnce()
    // It checks; then every shot is judged; then a settings change makes one new again: asked again.
    vi.mocked(commands.packPreview).mockResolvedValue(judged(speaks({ state: "checking", checked: 1 })))
    await act(() => queryClient.invalidateQueries())
    await waitFor(() => expect(result.current.check?.kind).toBe("checking"))
    vi.mocked(commands.packPreview).mockResolvedValue(judged(speaks({ checked: 2 })))
    await act(() => queryClient.invalidateQueries())
    await waitFor(() => expect(result.current.check).toBeNull())
    vi.mocked(commands.packPreview).mockResolvedValue(judged(speaks({ checked: 1 })))
    await act(() => queryClient.invalidateQueries())
    await waitFor(() => expect(commands.packFilterCheck).toHaveBeenCalledTimes(2))
  })

  it("asks for nothing from a filter that is not turned on, is off, or has judged every shot", async () => {
    await open(speaks(), DEFAULT_SPEC)
    cleanup()
    await open(speaks({ state: "off", total: 0 }))
    cleanup()
    await open(speaks({ checked: 2 }))
    await act(() => queryClient.invalidateQueries())
    expect(commands.packFilterCheck).not.toHaveBeenCalled()
  })

  it("holds Export back while a check is asked for", async () => {
    let answer: (started: boolean) => void = () => {}
    vi.mocked(commands.packFilterCheck).mockImplementation(() => new Promise((ok) => (answer = ok)))
    const { result } = await open(speaks())
    await waitFor(() => expect(commands.packFilterCheck).toHaveBeenCalled())
    expect(result.current.current).toBe(false)
    vi.mocked(commands.packPreview).mockResolvedValue(judged(speaks({ state: "checking" })))
    await act(async () => answer(true))
    await waitFor(() => expect(result.current.check?.kind).toBe("checking"))
  })

  it("a failed check isn't asked again by itself, only by Try again", async () => {
    const { result } = await open(speaks({ state: "failed", checked: 1 }))
    await act(() => queryClient.invalidateQueries())
    await act(() => queryClient.invalidateQueries())
    expect(commands.packFilterCheck).not.toHaveBeenCalled()
    // The shots it did not judge keep its word from before, or are in, as shown: Export goes on.
    expect(result.current.check).toEqual({ kind: "failed", unchecked: 1 })
    await waitFor(() => expect(result.current.current).toBe(true))
    startsWhenAsked(speaks({ state: "checking", checked: 1 }))
    act(() => result.current.tryAgain())
    await waitFor(() => expect(commands.packFilterCheck).toHaveBeenCalledExactlyOnceWith("p", "c", KEY, on, true))
    await waitFor(() => expect(result.current.check?.kind).toBe("checking"))
  })

  it("a check that finds nothing to start says it failed, and starts again on Try again", async () => {
    vi.mocked(commands.packFilterCheck).mockResolvedValue(false)
    const { result } = await open(speaks({ checked: 1 }))
    await waitFor(() => expect(commands.packFilterCheck).toHaveBeenCalledOnce())
    // Not silent: the footer offers Try again, and Export goes on.
    await waitFor(() => expect(result.current.check).toEqual({ kind: "failed", unchecked: 1 }))
    expect(result.current.filters[0].state).toBe("failed")
    await waitFor(() => expect(result.current.current).toBe(true))
    await act(() => queryClient.invalidateQueries())
    expect(commands.packFilterCheck).toHaveBeenCalledOnce()
    startsWhenAsked(speaks({ state: "checking", checked: 1 }))
    act(() => result.current.tryAgain())
    await waitFor(() => expect(commands.packFilterCheck).toHaveBeenLastCalledWith("p", "c", KEY, on, true))
    await waitFor(() => expect(result.current.check?.kind).toBe("checking"))
    expect(result.current.filters[0].state).toBe("checking")
  })

  it("a check that can't start is not asked for again, and lets Export go on", async () => {
    vi.mocked(commands.packFilterCheck).mockRejectedValue("Speech Filter is turned off.")
    const { result } = await open(speaks())
    await waitFor(() => expect(commands.packFilterCheck).toHaveBeenCalledOnce())
    await waitFor(() => expect(result.current.current).toBe(true))
    await act(() => queryClient.invalidateQueries())
    expect(commands.packFilterCheck).toHaveBeenCalledOnce()
  })
})
