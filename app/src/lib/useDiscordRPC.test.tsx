import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, renderHook, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, Cast, DiscordRpcSettings, JobInfo, ProjectSummary } from "@/bindings"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { useActivity } from "@/state/activity"
import { inLanguage } from "@/test/i18n"
import { useDiscordRPC } from "./useDiscordRPC"

const { state } = vi.hoisted(() => ({
  state: { route: {} as Record<string, string>, path: "/", settings: {} as AppSettings },
}))

vi.mock("@tanstack/react-router", async (original) => ({
  ...(await original<typeof import("@tanstack/react-router")>()),
  useParams: () => state.route,
  useRouterState: ({ select }: { select: (s: { location: { pathname: string } }) => unknown }) =>
    select({ location: { pathname: state.path } }),
}))

vi.mock("@/lib/ipc", async (original) => ({
  ...(await original<typeof import("@/lib/ipc")>()),
  commands: {
    settingsGet: vi.fn(async () => state.settings),
    discordView: vi.fn(async () => null),
    projectGet: vi.fn(async (id: string) => ({ id, title: `Title of ${id}`, kind: id === "film" ? "movie" : "series" })),
    mediaList: vi.fn(async () => [{ id: "m1", label: "S01E01", fileName: "pilot.mkv" }]),
    castGet: vi.fn(async () => ({ characters: [{ id: "c1", name: "Walter", number: 1 }] })),
  },
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
)

const at = (route: Record<string, string>, discordRpc?: DiscordRpcSettings, path = "/p/p1") => {
  queryClient.clear()
  state.route = route
  state.path = path
  state.settings = { libraryRoot: "L", exportPresetId: "p", audioLanguage: "eng", discordRpc }
  return renderHook(() => useDiscordRPC(), { wrapper })
}

const lastView = () => vi.mocked(commands.discordView).mock.lastCall?.[0]
const NOTHING = { project: null, episode: null, character: null, percent: null }

const job = (kind: string, jobState: string, progress = 0.5, id = "j1") =>
  ({ id, kind, state: jobState, progress, projectId: "p9", rev: 1 }) as unknown as JobInfo
const exportJob = (progress: number) => job("export", "running", progress)
const setJobs = (...jobs: JobInfo[]) =>
  act(() => useActivity.setState({ jobs: Object.fromEntries(jobs.map((j) => [j.id, j])), order: jobs.map((j) => j.id) }))
const setJob = (one: JobInfo) => setJobs(one)

/** Flips switches as a save would, on the settings the hook is reading. */
const switched = (discordRpc: DiscordRpcSettings) =>
  act(() => queryClient.setQueryData(["settings"], { ...state.settings, discordRpc }))

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  useActivity.setState({ jobs: {}, order: [] })
})
afterEach(cleanup)

describe("what the window tells the app for Discord", () => {
  it("off, it says nothing and reads nothing, with names on or a job running", async () => {
    setJob(exportJob(0.5))
    at({ pid: "p1", cid: "c1" }, { enabled: false, showProject: true, showCharacter: true })
    await waitFor(() => expect(commands.settingsGet).toHaveBeenCalled())
    await act(async () => {
      await queryClient.getQueryCache().find({ queryKey: ["settings"] })?.promise
    })
    expect(commands.discordView).not.toHaveBeenCalled()
    expect(commands.projectGet).not.toHaveBeenCalled()
    expect(commands.castGet).not.toHaveBeenCalled()
  })

  it("on, a name stays unread until its own switch is on too", async () => {
    at({ pid: "p1", cid: "c1" }, { enabled: true }, "/p/p1/cast/c1/pack")
    await waitFor(() => expect(lastView()).toEqual({ doing: "character", ...NOTHING }))
    expect(commands.projectGet).not.toHaveBeenCalled()
    expect(commands.castGet).not.toHaveBeenCalled()
    cleanup()

    // The player reads the project for its kind: that isn't told, nor its title.
    at({ pid: "p1", mid: "m1" }, { enabled: true }, "/p/p1/timeline/m1")
    await waitFor(() => expect(lastView()).toEqual({ doing: "episode", ...NOTHING }))
    expect(commands.mediaList).not.toHaveBeenCalled()
  })

  it("the player says file in a movie, and only 'in a project' until the kind is known", async () => {
    let kindHere!: (project: ProjectSummary) => void
    vi.mocked(commands.projectGet).mockReturnValueOnce(new Promise((here) => (kindHere = here)))
    at({ pid: "film", mid: "m1" }, { enabled: true, showEpisode: true }, "/p/film/timeline/m1")
    await waitFor(() => expect(lastView()).toEqual({ doing: "project", ...NOTHING }))
    await act(async () => kindHere({ id: "film", title: "A film", kind: "movie" } as ProjectSummary))
    await waitFor(() =>
      expect(lastView()).toEqual({ ...NOTHING, doing: "file", episode: expect.stringContaining("S01E01") }),
    )
    expect(vi.mocked(commands.discordView).mock.calls.map(([view]) => view.doing)).not.toContain("episode")
  })

  it("only the pack is a scenepack: a character's page is the project", async () => {
    const all = { enabled: true, showCharacter: true }
    at({ pid: "p1", cid: "c1" }, all, "/p/p1/cast/c1")
    await waitFor(() => expect(lastView()).toEqual({ doing: "project", ...NOTHING }))
    expect(commands.castGet).not.toHaveBeenCalled()
    cleanup()

    at({ pid: "p1", cid: "c1" }, all, "/p/p1/cast/c1/pack")
    await waitFor(() => expect(lastView()).toEqual({ ...NOTHING, doing: "character", character: "Walter" }))
  })

  it("a name switched off after it was read is not told again", async () => {
    const on = { enabled: true, showProject: true, showEpisode: true, showCharacter: true }
    at({ pid: "p1", mid: "m1" }, on, "/p/p1/timeline/m1")
    await waitFor(() => expect(lastView()?.episode).toContain("S01E01"))
    await waitFor(() => expect(lastView()?.project).toBe("Title of p1"))
    switched({ ...on, showProject: false })
    await waitFor(() => expect(lastView()?.project).toBeNull())
    expect(lastView()?.episode).toContain("S01E01")
    switched({ ...on, showProject: false, showEpisode: false })
    await waitFor(() => expect(lastView()).toEqual({ doing: "episode", ...NOTHING }))
    cleanup()

    at({ pid: "p1", cid: "c1" }, on, "/p/p1/cast/c1/pack")
    await waitFor(() => expect(lastView()?.character).toBe("Walter"))
    switched({ ...on, showCharacter: false })
    await waitFor(() => expect(lastView()?.character).toBeNull())
  })

  it("tells the view again after a save: ending the connection forgot it", async () => {
    at({ pid: "p1" }, { enabled: true })
    await waitFor(() => expect(lastView()).toEqual({ doing: "project", ...NOTHING }))
    const told = vi.mocked(commands.discordView).mock.calls.length
    switched({ enabled: true })
    await waitFor(() => expect(commands.discordView).toHaveBeenCalledTimes(told + 1))
  })

  it("a character nobody named goes out as English's Character 3, whatever AutoScene speaks", async () => {
    vi.mocked(commands.castGet).mockResolvedValueOnce({ characters: [{ id: "c3", name: null, number: 3 }] } as unknown as Cast)
    await inLanguage("ro", { cast: { unnamed: "Personajul {{n}}" } })
    at({ pid: "p1", cid: "c3" }, { enabled: true, showProject: false, showEpisode: false, showCharacter: true }, "/p/p1/cast/c3/pack")
    await waitFor(() => expect(lastView()?.character).toBe("Character 3"))
  })

  it("each place sends only the names that belong to it", async () => {
    at({}, { enabled: true, showProject: true, showEpisode: true, showCharacter: true })
    await waitFor(() => expect(lastView()).toEqual({ doing: "idle", ...NOTHING }))
    cleanup()

    at({ pid: "p1", mid: "m1" }, { enabled: true, showProject: true, showEpisode: true, showCharacter: true })
    await waitFor(() =>
      expect(lastView()).toEqual({ ...NOTHING, doing: "episode", project: "Title of p1", episode: expect.stringContaining("S01E01") }),
    )
    expect(commands.castGet).not.toHaveBeenCalled()
  })

  it("only a running export counts: not another job, not one that waits", async () => {
    setJobs(job("analyze", "running"), job("export", "queued", 0, "j2"))
    at({ pid: "p1" }, { enabled: true })
    await waitFor(() => expect(lastView()).toEqual({ doing: "project", ...NOTHING }))
    expect(vi.mocked(commands.discordView).mock.calls.map(([view]) => view.doing)).not.toContain("export")
  })

  it("an export wins over where the person is", async () => {
    setJob(exportJob(0.5))
    at({ pid: "p1", mid: "m1" }, { enabled: true, showEpisode: true }, "/p/p1/timeline/m1")
    await waitFor(() => expect(lastView()).toEqual({ ...NOTHING, doing: "export", percent: 50 }))
    expect(commands.mediaList).not.toHaveBeenCalled()
    cleanup()
    vi.clearAllMocks()

    at({ pid: "p1", cid: "c1" }, { enabled: true, showCharacter: true }, "/p/p1/cast/c1/pack")
    await waitFor(() => expect(lastView()).toEqual({ ...NOTHING, doing: "export", percent: 50 }))
    expect(commands.castGet).not.toHaveBeenCalled()
  })

  it("an export names its own project, not the one on screen, and only a new percent is told", async () => {
    setJob(exportJob(0.42))
    at({ pid: "p1" }, { enabled: true, showProject: true })
    await waitFor(() =>
      expect(lastView()).toEqual({ ...NOTHING, doing: "export", project: "Title of p9", percent: 42 }),
    )
    const told = vi.mocked(commands.discordView).mock.calls.length
    setJob(exportJob(0.4201))
    expect(commands.discordView).toHaveBeenCalledTimes(told)
    setJob(exportJob(0.43))
    await waitFor(() => expect(lastView()?.percent).toBe(43))
    expect(commands.discordView).toHaveBeenCalledTimes(told + 1)
  })
})
