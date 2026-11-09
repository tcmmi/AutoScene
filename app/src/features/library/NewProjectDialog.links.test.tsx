import type * as React from "react"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { LinkItem, PluginInfo } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { useNewProject } from "@/state/new-project"
import { pluginInfo } from "@/test/plugins"
import { NewProjectDialog } from "./NewProjectDialog"

const reel = (o: Partial<PluginInfo> = {}): PluginInfo =>
  pluginInfo({
    id: "dev.autoscene.reel-in",
    name: "Reel In",
    settings: [
      {
        key: "quality",
        title: "Quality",
        description: null,
        kind: {
          type: "select",
          default: "best",
          options: [
            { value: "best", label: "Best" },
            { value: "1080", label: "1080p" },
          ],
        },
      },
    ],
    sources: [
      {
        id: "links",
        title: "YouTube and TikTok",
        description: null,
        linkHosts: ["youtube.com", "youtu.be", "tiktok.com"],
        settings: ["quality"],
      },
    ],
    ...o,
  })

/** What the backend reads of each token: a YouTube link is taken, anything else isn't. */
const read = (text: string): LinkItem[] =>
  text
    .split(/\s+/)
    .filter(Boolean)
    .map((token) => {
      const taken = token.includes("youtu")
      return {
        text: token,
        link: taken ? `https://${token.replace(/^https?:\/\//, "")}` : "",
        short: token.replace(/^https?:\/\/(www\.)?/, ""),
        pluginId: taken ? "dev.autoscene.reel-in" : null,
        sourceId: taken ? "links" : null,
        sourceTitle: taken ? "YouTube and TikTok" : null,
        pluginName: taken ? "Reel In" : null,
        refused: taken ? null : { key: "", text: "No plugin takes links from vimeo.com." },
      }
    })

const { state } = vi.hoisted(() => ({ state: { plugins: [] as PluginInfo[] } }))

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => vi.fn() }))
vi.mock("@/lib/ipc", () => ({
  commands: {
    importScan: vi.fn(() => Promise.resolve([])),
    projectCreate: vi.fn(() => Promise.resolve({ id: "p" })),
    settingsGet: vi.fn(() => Promise.resolve({ findBodies: false, fastScan: false })),
    scanDefaultsGet: vi.fn(() => new Promise(() => {})),
    pluginsList: vi.fn(() => Promise.resolve(state.plugins)),
    pluginSettingsGet: vi.fn(() => Promise.resolve({ quality: "best" })),
    pluginSettingsSet: vi.fn((_: string, v: Record<string, unknown>) => Promise.resolve(v)),
    sourceLinksRead: vi.fn((text: string) => Promise.resolve(read(text))),
    sourceLinksQueue: vi.fn((_: string, links: string[]) =>
      Promise.resolve({ queued: links.length, waiting: 0, refused: [], outDir: "D:\\Downloads\\Show" }),
    ),
  },
  errorMessage: (e: unknown) => String(e),
}))

let queryClient: QueryClient
const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)
const show = () => {
  useNewProject.getState().show([])
  render(<NewProjectDialog />, { wrapper })
}

describe("links in New project", () => {
  beforeEach(() => {
    queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    state.plugins = [reel()]
    vi.clearAllMocks()
  })
  afterEach(() => {
    cleanup()
    useNewProject.getState().setOpen(false)
  })

  it("offers Add links… only while a plugin that may run takes links", async () => {
    state.plugins = [reel({ active: false })]
    show()
    await screen.findByText("Drop a season folder or video files here")
    await waitFor(() => expect(commands.pluginsList).toHaveBeenCalled())
    expect(screen.queryByRole("button", { name: "Add links…" })).toBeNull()
    cleanup()
    state.plugins = [reel()]
    queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    show()
    expect(await screen.findByRole("button", { name: "Add links…" })).toBeTruthy()
  })

  it("adds the links a plugin takes, says why the others can't be, and creates the project with links alone", async () => {
    const user = userEvent.setup()
    show()
    await user.click(await screen.findByRole("button", { name: "Add links…" }))
    expect(screen.getByText("Download only videos you have the right to use.")).toBeTruthy()
    // The source's setting shows beside the links.
    expect(await screen.findByText("Quality")).toBeTruthy()
    await user.type(screen.getByLabelText("Links"), "youtu.be/abc vimeo.com/1 www.youtube.com/watch?v=x")
    expect(await screen.findByText("No plugin takes links from vimeo.com.")).toBeTruthy()
    await user.click(await screen.findByRole("button", { name: "Add 2 links" }))
    // Each a row of the list, until the project exists.
    expect(screen.getAllByText("Downloads after you create the project")).toHaveLength(2)
    expect(screen.getByText("Links download after you create the project.")).toBeTruthy()
    expect(screen.getByText(/2 links/)).toBeTruthy()
    // One taken out again.
    await user.click(screen.getByRole("button", { name: "Remove youtu.be/abc" }))
    expect(screen.getAllByText("Downloads after you create the project")).toHaveLength(1)

    await user.type(screen.getByLabelText("Title"), "Sprite Fright")
    await user.click(screen.getByRole("button", { name: "Create project" }))
    await waitFor(() => expect(commands.sourceLinksQueue).toHaveBeenCalled())
    expect(vi.mocked(commands.projectCreate).mock.calls[0][0]).toMatchObject({ title: "Sprite Fright", files: [], kind: "series" })
    expect(commands.sourceLinksQueue).toHaveBeenCalledWith("p", ["https://www.youtube.com/watch?v=x"])
  })

  it("keeps Create off without a title, or without files and links", async () => {
    const user = userEvent.setup()
    show()
    await user.type(await screen.findByLabelText("Title"), "Show")
    expect(screen.getByRole("button", { name: "Create project" })).toHaveProperty("disabled", true)
  })
})
