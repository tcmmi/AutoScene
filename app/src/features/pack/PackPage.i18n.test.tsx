import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { Character, ExportPreset, MediaItem, Msg, PackClip, PluginInfo } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { castEdit } from "@/features/cast/cast"
import { queryClient } from "@/lib/queries"
import { Builder } from "./PackPage"
import { DEFAULT_SPEC } from "./spec"
import type { Pack } from "./use-pack"

// What the page passes to its parts (the header's counts, the toasts, the card's menu, the footer's format) is
// looked at here, rendered: the catalog's English alone would not show a wrong value or a wrong branch.
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))
vi.mock("@/features/cast/cast", () => ({ castEdit: vi.fn(() => Promise.resolve({})) }))
// The grid lists its items plainly, so the cards (and their menus) are the real ones.
vi.mock("@/components/ui/virtual-grid", () => ({
  VirtualGrid: ({ items, renderItem }: { items: unknown[]; renderItem: (item: unknown, i: number) => React.ReactNode }) => (
    <div>
      {items.map((c, i) => (
        <div key={i}>{renderItem(c, i)}</div>
      ))}
    </div>
  ),
}))
vi.mock("./PackFilters", () => ({ PackFilters: () => null }))
vi.mock("./PackExportDialog", () => ({ PackExportDialog: () => null }))

const preset = {
  id: "prores-hq",
  name: "ProRes 422 HQ",
  nameMsg: { key: "export.preset.proresHq.name", text: "ProRes 422 HQ" } satisfies Msg,
  description: "Edit-grade intra codec.",
  container: "mov",
  video: { codec: "prores", profile: "hq" },
  audio: { codec: "pcm24" },
  maxHeight: null,
  hdr: "auto",
  stereo: false,
  builtin: true,
} as unknown as ExportPreset

vi.mock("@/lib/queries", async (importOriginal) => {
  const q = await importOriginal<typeof import("@/lib/queries")>()
  return {
    ...q,
    useSettings: () => ({ data: { exportPresetId: "prores-hq" } }),
    usePresets: () => ({ data: [preset] }),
    usePlugins: () => ({ data: [] as PluginInfo[] }),
  }
})

const walter = { id: "c", label: "Walter", number: 1, waiting: 0, media: 3, shots: 12, cover: null, coverSmall: null } as unknown as Character
const nameless = { ...walter, label: null } as unknown as Character
const naming = { template: "{name}", folder: true, merge: true, file: "{character}" }
const clip = (more: Partial<PackClip> = {}, shot: Partial<PackClip["shot"]> = {}): PackClip =>
  ({
    shot: { mediaId: "m", start: 0, end: 48, sprite: null, cells: 0, trackId: 1, face: "", faceHeight: 0.3, faceless: false, ...shot },
    start: 0,
    end: 48,
    size: "close_up",
    quality: 1,
    coverage: 1,
    others: 0,
    score: 1,
    included: true,
    byRules: true,
    manual: null,
    reason: null,
    filter: null,
    ...more,
  }) as PackClip

function pack(clips: PackClip[], taken = clips.filter((c) => c.included).length): Pack {
  return {
    state: { spec: DEFAULT_SPEC, naming },
    preview: { data: { clips, totals: { clips: taken, frames: 24, seconds: 1 }, stamp: "s" }, isPending: false, isError: false },
    plan: { data: { names: [], sizes: [{ presetId: "prores-hq", bytes: 3e8 }], folder: null, file: "x" } },
    filters: undefined,
    check: null,
    ready: true,
    current: true,
    setSpec: vi.fn(),
    setIncluded: vi.fn(),
    setEach: vi.fn(() => Promise.resolve(true)),
    invertIncluded: vi.fn(() => Promise.resolve(true)),
    restore: vi.fn(() => Promise.resolve()),
    clearPicks: vi.fn(() => Promise.resolve(true)),
  } as unknown as Pack
}

async function show(it: Pack, character: Character = walter) {
  const root = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <AppMenu>
            <div>
              <Builder pid="p" cid="c" character={character} media={[]} mediaById={new Map<string, MediaItem>()} pack={it} toolbar={null} />
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

beforeEach(() => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  Object.defineProperty(Element.prototype, "scrollTo", { configurable: true, value: () => {} })
})
afterEach(() => {
  cleanup()
  Reflect.deleteProperty(Element.prototype, "scrollTo")
  queryClient.clear()
  vi.clearAllMocks()
})

const many = (n: number, more: Partial<PackClip> = {}) => Array.from({ length: n }, () => clip(more))

describe("the pack page passes its words the right values", () => {
  it("counts the shots and the episodes apart in the header", async () => {
    await show(pack(many(12)))
    expect(screen.getByText("12 shots in 3 episodes · the pack takes the bright ones")).toBeTruthy()
  })

  it("counts what is in the pack, and what is out, in the switch above the grid", async () => {
    // 5 shots, 2 of them taken: "All 5", "In the pack 2", "Left out 3".
    const clips = [...many(2), ...many(3, { included: false, reason: "short" })]
    await show(pack(clips))
    const group = screen.getByRole("radiogroup", { name: "Show" })
    expect(within(group).getAllByRole("radio").map((r) => r.textContent)).toEqual(["All 5", "In the pack 2", "Left out 3"])
  })

  it("names the preset in the footer from its key, not as an object", async () => {
    await show(pack(many(2)))
    expect(screen.getByText(/\(ProRes 422 HQ\)/)).toBeTruthy()
  })

  it("says how many picks went back to the filters, one or two", async () => {
    await show(pack([clip({ manual: true })]))
    fireEvent.click(screen.getByRole("button", { name: /1 pick by hand/ }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Handed 1 shot back to the filters"))
    cleanup()
    vi.clearAllMocks()
    await show(pack([clip({ manual: true }), clip({ manual: false })]))
    fireEvent.click(screen.getByRole("button", { name: /2 picks by hand/ }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Handed 2 shots back to the filters"))
  })
})

describe("a body's menu names whose pack it is", () => {
  const faceless = () => clip({ included: false, reason: "faceless" }, { faceless: true, bodies: [7] })
  const notThem = async (character: Character) => {
    await show(pack([faceless()]), character)
    fireEvent.contextMenu(screen.getAllByRole("button").find((b) => b.hasAttribute("aria-pressed"))!)
    return await screen.findByRole("menu")
  }

  it("says Not Walter when it knows the name, and the toast says so too", async () => {
    const menu = await notThem(walter)
    expect(within(menu).queryByRole("menuitem", { name: /^Not them/ })).toBeNull()
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Not Walter" }))
    await waitFor(() => expect(castEdit).toHaveBeenCalledWith("p", { op: "body_not_them", bodies: [7], characterId: "c" }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Not Walter: out of the pack"))
  })

  it("says Not them when it does not, and the toast says so too", async () => {
    const menu = await notThem(nameless)
    expect(within(menu).queryByRole("menuitem", { name: /Not Walter/ })).toBeNull()
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Not them" }))
    await waitFor(() => expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Not them: out of the pack"))
  })
})
