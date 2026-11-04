import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { Character, JobInfo, PackClip, PluginInfo } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { useActivity } from "@/state/activity"
import { Builder } from "./PackPage"
import { DEFAULT_SPEC, type PackCheck } from "./spec"
import { type Pack, packExportStarted } from "./use-pack"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return { ...ipc, commands: { ...ipc.commands, packActionRun: vi.fn() } }
})
// The page's wiring is what is looked at here: the grid, the cards, the filters and the export dialog stand aside.
vi.mock("@/components/ui/virtual-grid", () => ({ VirtualGrid: () => null }))
vi.mock("./PackFilters", () => ({ PackFilters: () => null }))
vi.mock("./PackExportDialog", () => ({ PackExportDialog: () => null }))

const fcpxml = {
  id: "dev.example.fcpxml",
  name: "Cutroom Bridge",
  icon: null,
  active: true,
  packActions: [{ id: "fcpxml", title: "Save as FCPXML…" }],
} as unknown as PluginInfo

vi.mock("@/lib/queries", async (importOriginal) => {
  const q = await importOriginal<typeof import("@/lib/queries")>()
  return {
    ...q,
    useSettings: () => ({ data: { exportPresetId: "prores-hq" } }),
    usePresets: () => ({ data: [] }),
    usePlugins: () => ({ data: [fcpxml] }),
  }
})

const walter = { id: "c", label: "Walter", waiting: 0, media: 1, shots: 1, cover: null, coverSmall: null } as unknown as Character
const clip = { shot: { mediaId: "m", start: 0, faceless: false }, start: 0, included: true, byRules: true, manual: null, reason: null } as unknown as PackClip
const naming = { template: "{name}", folder: true, merge: true, file: "{character}" }

/** A pack of one shot, as `usePack` hands it over. */
function pack(check: PackCheck | null = null): Pack {
  return {
    state: { spec: DEFAULT_SPEC, naming },
    preview: { data: { clips: [clip], totals: { clips: 1, frames: 24, seconds: 1 }, stamp: "stamp-9" }, isPending: false, isError: false },
    plan: { data: undefined },
    filters: undefined,
    check,
    current: true,
    setSpec: vi.fn(),
    setIncluded: vi.fn(),
    clearPicks: vi.fn(),
  } as unknown as Pack
}

async function show(check: PackCheck | null = null) {
  const root = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <AppMenu>
            <div>
              <Builder pid="p" cid="c" character={walter} media={[]} mediaById={new Map()} pack={pack(check)} toolbar={null} />
            </div>
          </AppMenu>
        </TooltipProvider>
      </QueryClientProvider>
    ),
  })
  const here = createRoute({ getParentRoute: () => root, path: "/", component: () => null })
  const router = createRouter({ routeTree: root.addChildren([here]), history: createMemoryHistory({ initialEntries: ["/"] }) })
  render(<RouterProvider router={router} />)
  await screen.findByText(/Click to take a shot in or out/)
}

const dots = () => screen.getByRole("button", { name: "Save the pack with a plugin" }) as HTMLButtonElement
const asked = ["p", "c", "dev.example.fcpxml/fcpxml", DEFAULT_SPEC, naming, "stamp-9"] as const

beforeEach(() => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  Object.defineProperty(Element.prototype, "scrollTo", { configurable: true, value: () => {} })
  vi.mocked(commands.packActionRun).mockResolvedValue({ kind: "cancelled" })
  useActivity.setState({ jobs: {}, order: [] })
})
afterEach(() => {
  cleanup()
  Reflect.deleteProperty(Element.prototype, "scrollTo")
  queryClient.clear()
  vi.clearAllMocks()
})

describe("saving the pack with a plugin", () => {
  it("the ⋯ asks for the pack as the grid shows it", async () => {
    await show()
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    fireEvent.click(await screen.findByRole("menuitem", { name: /Save as FCPXML…/ }))
    await waitFor(() => expect(commands.packActionRun).toHaveBeenCalledExactlyOnceWith(...asked))
  })

  it("the grid's right-click menu asks for the same, in a group of its own after the app's", async () => {
    await show()
    fireEvent.contextMenu(screen.getByText(/Click to take a shot in or out/), { clientX: 40, clientY: 40 })
    const menu = await screen.findByRole("menu")
    const entries = within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent ?? "")
    // Export first, the plugin's entry after a heading that is not its own.
    expect(entries[0]).toContain("Export the pack…")
    expect(within(menu).getByText("Save with a plugin")).toBeTruthy()
    const mine = within(menu).getByRole("menuitem", { name: /Save as FCPXML…/ })
    expect(entries.indexOf(mine.textContent ?? "")).toBeGreaterThan(0)
    fireEvent.click(mine)
    await waitFor(() => expect(commands.packActionRun).toHaveBeenCalledExactlyOnceWith(...asked))
  })

  it("stays possible while the pack's export runs", async () => {
    packExportStarted("p", "c", "e1" as never)
    act(() => {
      useActivity.getState().upsert({ id: "j1", kind: "export", state: "running", progress: 0.4, exportId: "e1", projectId: "p", rev: 1 } as unknown as JobInfo)
    })
    await show()
    // Export shows how far it is, and saving goes on.
    expect(await screen.findByText(/Exporting…/)).toBeTruthy()
    expect(dots().disabled).toBe(false)
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    fireEvent.click(await screen.findByRole("menuitem", { name: /Save as FCPXML…/ }))
    await waitFor(() => expect(commands.packActionRun).toHaveBeenCalledExactlyOnceWith(...asked))
  })

  it("waits while a plugin's filter checks shots, in the ⋯ and in the right-click menu", async () => {
    await show({ kind: "checking", checked: 3, total: 10 })
    expect(dots().disabled).toBe(true)
    fireEvent.contextMenu(screen.getByText(/Click to take a shot in or out/), { clientX: 40, clientY: 40 })
    const mine = await screen.findByRole("menuitem", { name: /Save as FCPXML…/ })
    expect(mine.getAttribute("aria-disabled")).toBe("true")
    fireEvent.click(mine)
    expect(commands.packActionRun).not.toHaveBeenCalled()
  })
})
