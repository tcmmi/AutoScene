import type * as React from "react"
import { cleanup, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { JobInfo, MediaItem } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { queryClient } from "@/lib/queries"
import { useActivity } from "@/state/activity"
import { inLanguage } from "@/test/i18n"
import { EpisodeCard } from "./EpisodeCard"
import { EpisodesPage } from "./EpisodesPage"
import { OverviewPage } from "./OverviewPage"
import { ProjectLoadError } from "./ProjectLoadError"
import { ProjectNav } from "./ProjectNav"

// The English of a project's screens, pinned where a series and a movie differ: a swapped key (the movie's
// "Add a folder" on a series, the series' "Episodes" on a movie's nav) would otherwise pass every test.

const data = vi.hoisted(() => ({ project: { kind: "series" } as Record<string, unknown>, media: [] as unknown[] }))
vi.mock("@tanstack/react-router", () => ({
  useParams: () => ({ pid: "p" }),
  useNavigate: () => vi.fn(),
  useRouterState: () => "/p/p",
  useCanGoBack: () => false,
  Link: ({ children, to, className }: { children: React.ReactNode; to: string; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}))
vi.mock("@/lib/queries", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/queries")>()),
  useProject: () => ({ data: data.project }),
  useMediaList: () => ({ data: data.media, isPending: false }),
  useCast: () => ({ data: { characters: [], unsure: 0 } }),
  usePlugins: () => ({ data: [] }),
  firstFailure: () => null,
}))
vi.mock("@/lib/ambient", () => ({ useAmbient: () => null }))
vi.mock("@/components/ui/page", () => ({ Page: ({ children, menu }: { children: React.ReactNode; menu?: React.ReactNode }) => <main data-menu={menu ? "y" : ""}>{children}</main> }))
vi.mock("@/components/ui/topbar", () => ({ TopBar: ({ nav }: { nav?: React.ReactNode }) => <header>{nav}</header> }))
vi.mock("@/features/cast/AniListNames", () => ({ AniListNames: () => null }))

beforeEach(() => {
  data.project = { id: "p", title: "Show", kind: "series", episodeCount: 3 }
  data.media = []
  useActivity.setState({ jobs: {}, order: [] })
})
afterEach(() => {
  cleanup()
  queryClient.clear()
})

const media = (o: Partial<MediaItem> = {}) => ({ id: "m", fileName: "Show.S01E01.mkv", label: "S01E01", status: "ready", shots: "none", ...o }) as MediaItem
const wrap = (ui: React.ReactElement) =>
  render(
    <AppMenu>
      <TooltipProvider>{ui}</TooltipProvider>
    </AppMenu>,
  )
const buttons = () => screen.getAllByRole("button").map((b) => b.textContent?.trim() || b.getAttribute("aria-label"))

describe("a project's navigation", () => {
  it("names the second section Episodes for a series and Files for a movie", () => {
    wrap(<ProjectNav pid="p" />)
    const nav = screen.getByRole("navigation", { name: "Project" })
    expect(within(nav).getAllByRole("link").map((l) => l.textContent)).toEqual(["Overview", "Episodes", "Cast", "Exports"])
    cleanup()
    data.project = { id: "p", title: "Film", kind: "movie" }
    wrap(<ProjectNav pid="p" />)
    expect(within(screen.getByRole("navigation", { name: "Project" })).getAllByRole("link").map((l) => l.textContent)).toEqual(["Overview", "Files", "Cast", "Exports"])
  })
})

describe("the overview", () => {
  it("says a series' words: add episodes, the Episodes shelf, the season folder", () => {
    wrap(<OverviewPage />)
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Show")
    expect(screen.getByRole("button", { name: /Add episodes/ })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /Add a folder/ })).toBeNull()
    expect(screen.getByText("Episodes")).toBeTruthy()
    expect(screen.getByRole("button", { name: /Add a season folder/ })).toBeTruthy()
    expect(screen.getByText("Add episodes, and the characters appear here as they are analysed.")).toBeTruthy()
  })

  it("says a movie's words: add a folder, the Files shelf", () => {
    data.project = { id: "p", title: "Film", kind: "movie" }
    wrap(<OverviewPage />)
    expect(screen.getAllByRole("button", { name: /Add a folder/ }).length).toBe(2)
    expect(screen.queryByRole("button", { name: /Add episodes/ })).toBeNull()
    expect(screen.getByText("Files")).toBeTruthy()
    expect(screen.queryByText("Episodes")).toBeNull()
    expect(buttons()).not.toContain("Add a season folder")
  })

  it("shows the Details link and no empty-shelf button once there are episodes", () => {
    data.media = [media()]
    wrap(<OverviewPage />)
    expect(screen.getByRole("link", { name: /Details/ })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /Add a season folder/ })).toBeNull()
    expect(screen.getByText("Not analysed yet")).toBeTruthy()
  })
})

describe("an episode's card", () => {
  it("badges a missing and a failed file", () => {
    const { unmount } = wrap(<EpisodeCard pid="p" media={media({ status: "missing" })} onOpen={() => {}} onRemove={() => {}} />)
    expect(screen.getByText("Missing")).toBeTruthy()
    unmount()
    wrap(<EpisodeCard pid="p" media={media({ status: "failed" })} onOpen={() => {}} onRemove={() => {}} />)
    expect(screen.getByText("Failed")).toBeTruthy()
  })

  it("says the stage of its job in the language, under its title and on its ring", async () => {
    await inLanguage("ro", { backend: { job: { stage: { findingShots: "Se caută cadrele" } } } })
    const stage = { key: "job.stage.findingShots", text: "Finding shots" }
    const job = { id: "j", kind: "analyze", mediaId: "m", state: "running", progress: 0.2, stage, rev: 1 } as JobInfo
    useActivity.setState({ jobs: { j: job }, order: ["j"] })
    const { container } = wrap(<EpisodeCard pid="p" media={media({ id: "m" })} onOpen={() => {}} onRemove={() => {}} />)
    expect(screen.getByText("Se caută cadrele")).toBeTruthy()
    expect(screen.getByRole("progressbar", { name: "Se caută cadrele" })).toBeTruthy()
    expect(container.innerHTML).not.toContain("[object Object]")
    useActivity.setState({ jobs: {}, order: [] })
  })

  it("counts shots and faces under the title", () => {
    wrap(<EpisodeCard pid="p" media={media({ shots: "ready", shotCount: 1200, faceCount: 31 })} onOpen={() => {}} onRemove={() => {}} />)
    expect(screen.getByText(/1,200 shots · 31 faces/)).toBeTruthy()
  })
})

describe("the episodes page", () => {
  it("is titled Episodes for a series and Files for a movie, with its empty state", () => {
    wrap(<EpisodesPage />)
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Episodes")
    expect(screen.getByText("No videos yet")).toBeTruthy()
    cleanup()
    data.project = { id: "p", title: "Film", kind: "movie" }
    wrap(<EpisodesPage />)
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Files")
  })
})

describe("a project that can't be loaded", () => {
  it("says it is gone, and leads back to the library, when it isn't found", () => {
    wrap(<ProjectLoadError error={{ code: "not_found", message: "No such project" }} onRetry={() => {}} />)
    expect(screen.getByText("This project isn't here any more")).toBeTruthy()
    expect(screen.getByRole("link", { name: "Back to Library" })).toBeTruthy()
  })
})
