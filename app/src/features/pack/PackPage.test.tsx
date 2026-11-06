import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { Character, ExportPreset, JobInfo, PackClip, PluginInfo } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { DEFAULT_KEYMAP, setKeymap } from "@/lib/shortcuts"
import { useActivity } from "@/state/activity"
import { inLanguage } from "@/test/i18n"
import { Builder } from "./PackPage"
import { DEFAULT_SPEC, type PackCheck } from "./spec"
import { type Pack, packExportStarted } from "./use-pack"

const { list } = vi.hoisted(() => ({ list: { presets: [] as ExportPreset[] } }))

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
    usePresets: () => ({ data: list.presets }),
    usePlugins: () => ({ data: [fcpxml] }),
  }
})

const walter = { id: "c", name: "Walter", number: 1, waiting: 0, media: 1, shots: 1, cover: null, coverSmall: null } as unknown as Character
const clip = { shot: { mediaId: "m", start: 0, faceless: false }, start: 0, included: true, byRules: true, manual: null, reason: null } as unknown as PackClip
const naming = { template: "{name}", folder: true, merge: true, file: "{character}" }

/** A pack of these shots, as `usePack` hands it over. */
function pack(check: PackCheck | null = null, clips: PackClip[] = [clip], ready = true): Pack {
  const taken = clips.filter((c) => c.included).length
  return {
    state: { spec: DEFAULT_SPEC, naming },
    preview: { data: { clips, totals: { clips: taken, frames: 24, seconds: 1 }, stamp: "stamp-9" }, isPending: false, isError: false },
    plan: { data: undefined },
    filters: undefined,
    check,
    ready,
    current: ready,
    setSpec: vi.fn(),
    setIncluded: vi.fn(),
    setEach: vi.fn(() => Promise.resolve(true)),
    invertIncluded: vi.fn(() => Promise.resolve(true)),
    restore: vi.fn(() => Promise.resolve()),
    clearPicks: vi.fn(() => Promise.resolve(true)),
  } as unknown as Pack
}

async function show(check: PackCheck | null = null, it: Pack = pack(check)) {
  const root = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <AppMenu>
            <div>
              <Builder pid="p" cid="c" character={walter} media={[]} mediaById={new Map()} pack={it} toolbar={null} />
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

const dots = () => screen.getByRole("button", { name: "Pack options" }) as HTMLButtonElement
const asked = ["p", "c", "dev.example.fcpxml/fcpxml", DEFAULT_SPEC, naming, "stamp-9"] as const

beforeEach(() => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  Object.defineProperty(Element.prototype, "scrollTo", { configurable: true, value: () => {} })
  vi.mocked(commands.packActionRun).mockResolvedValue({ kind: "cancelled" })
  useActivity.setState({ jobs: {}, order: [] })
})
afterEach(() => {
  cleanup()
  list.presets = []
  Reflect.deleteProperty(Element.prototype, "scrollTo")
  queryClient.clear()
  vi.clearAllMocks()
})

describe("the pack's footer", () => {
  it("says the format an export starts with in the UI's language", async () => {
    await inLanguage("ro", { backend: { fixture: { preset: "ProRes românesc (montaj)" } } })
    list.presets = [{ id: "prores-hq", name: "ProRes 422 HQ", nameMsg: { key: "fixture.preset", text: "ProRes 422 HQ" } } as ExportPreset]
    await show(null, { ...pack(), plan: { data: { sizes: [{ presetId: "prores-hq", bytes: 3e8 }] } } } as unknown as Pack)
    expect(screen.getByText(/\(ProRes românesc\)/)).toBeTruthy()
  })
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
    for (const open of [
      () => fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false }),
      () => fireEvent.contextMenu(screen.getByText(/Click to take a shot in or out/), { clientX: 40, clientY: 40 }),
    ]) {
      open()
      const mine = await screen.findByRole("menuitem", { name: /Save as FCPXML…/ })
      expect(mine.getAttribute("aria-disabled")).toBe("true")
      fireEvent.click(mine)
      expect(commands.packActionRun).not.toHaveBeenCalled()
      fireEvent.keyDown(mine, { key: "Escape" })
      await waitFor(() => expect(screen.queryByRole("menuitem", { name: /Save as FCPXML…/ })).toBeNull())
    }
  })
})

describe("every shot at once", () => {
  const out = { ...clip, shot: { ...clip.shot, start: 100 }, start: 100, included: false, byRules: false, reason: "short" } as unknown as PackClip
  const gone = { ...clip, shot: { ...clip.shot, start: 200 }, start: 200, included: false, reason: "missing" } as unknown as PackClip
  const press = (key: string, init: KeyboardEventInit = {}, at: EventTarget = window) => {
    const e = new KeyboardEvent("keydown", { key, ctrlKey: true, bubbles: true, cancelable: true, ...init })
    act(() => void at.dispatchEvent(e))
    return e
  }
  /** What a call of `setEach` makes of each shot. */
  const asked = (p: Pack) => {
    const [clips, included] = vi.mocked(p.setEach).mock.lastCall!
    return clips.map(included)
  }
  const entry = (name: RegExp) => screen.findByRole("menuitem", { name })
  afterEach(() => setKeymap(DEFAULT_KEYMAP))

  it("Ctrl+A takes every shot, and once all are in leaves them out", async () => {
    const some = pack(null, [clip, out])
    await show(null, some)
    expect(press("a").defaultPrevented).toBe(true)
    expect(asked(some)).toEqual([true, true])
    cleanup()
    const all = pack()
    await show(null, all)
    press("a")
    expect(asked(all)).toEqual([false])
  })

  it("a shot whose file is gone doesn't keep Ctrl+A from leaving the others out", async () => {
    const it = pack(null, [clip, gone])
    await show(null, it)
    press("a")
    expect(asked(it)).toEqual([false, false])
  })

  it("the other two keys leave every shot out and invert the pack", async () => {
    const it = pack(null, [clip, out])
    await show(null, it)
    press("A", { shiftKey: true })
    expect(asked(it)).toEqual([false, false])
    press("I", { shiftKey: true })
    expect(it.invertIncluded).toHaveBeenCalledExactlyOnceWith([clip, out])
  })

  it("answers the keys the person gave them, and shows those", async () => {
    setKeymap({ preset: "autoscene", own: { "pack.invert.0": [{ key: "v" }] } })
    const it = pack(null, [clip, out])
    await show(null, it)
    press("I", { shiftKey: true })
    expect(it.invertIncluded).not.toHaveBeenCalled()
    press("v", { ctrlKey: false })
    expect(it.invertIncluded).toHaveBeenCalledOnce()
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    expect((await entry(/Invert the pack/)).textContent).toContain("V")
  })

  it("Ctrl+A in a text field stays the field's", async () => {
    const it = pack(null, [clip, out])
    await show(null, it)
    const field = document.body.appendChild(document.createElement("input"))
    expect(press("a", {}, field).defaultPrevented).toBe(false)
    field.remove()
    expect(it.setEach).not.toHaveBeenCalled()
  })

  it("a key held down counts once, and an open dialog keeps the keys", async () => {
    const it = pack(null, [clip, out])
    await show(null, it)
    press("a", { repeat: true })
    expect(it.setEach).not.toHaveBeenCalled()
    const dialog = document.body.appendChild(document.createElement("div"))
    dialog.setAttribute("role", "dialog")
    dialog.dataset.state = "open"
    press("a")
    dialog.remove()
    expect(it.setEach).not.toHaveBeenCalled()
  })

  it("waits for the pack: no key, and entries that say so", async () => {
    const it = pack(null, [clip, out], false)
    await show(null, it)
    press("a")
    expect(it.setEach).not.toHaveBeenCalled()
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    expect((await entry(/Take every shot/)).getAttribute("aria-disabled")).toBe("true")
    expect((await entry(/Invert the pack/)).getAttribute("aria-disabled")).toBe("true")
  })

  it("the ⋯ and the right-click list the same entries with their keys", async () => {
    const it = pack(null, [{ ...clip, manual: true } as PackClip, out])
    await show(null, it)
    const names = [/Take every shot/, /Leave every shot out/, /Invert the pack/, /Hand every shot back to the filters/]
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    expect((await entry(names[0])).textContent).toContain("Ctrl")
    for (const name of names) await entry(name)
    fireEvent.click(await entry(names[2]))
    await waitFor(() => expect(it.invertIncluded).toHaveBeenCalledOnce())
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    fireEvent.contextMenu(screen.getByText(/Click to take a shot in or out/), { clientX: 40, clientY: 40 })
    for (const name of names) await entry(name)
    fireEvent.click(await entry(names[1]))
    await waitFor(() => expect(asked(it)).toEqual([false, false]))
  })

  it("what would change nothing is off: all in, none in", async () => {
    await show(null, pack())
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    expect((await entry(/Take every shot/)).getAttribute("aria-disabled")).toBe("true")
    expect((await entry(/Leave every shot out/)).getAttribute("aria-disabled")).toBeNull()
    cleanup()
    await show(null, pack(null, [out]))
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    expect((await entry(/Take every shot/)).getAttribute("aria-disabled")).toBeNull()
    expect((await entry(/Leave every shot out/)).getAttribute("aria-disabled")).toBe("true")
  })

  it("says what it did and takes it back: the shots as they were", async () => {
    const before = [clip, out]
    const it = pack(null, before)
    await show(null, it)
    press("a")
    await waitFor(() => expect(toast).toHaveBeenCalledOnce())
    const [said, options] = vi.mocked(toast).mock.lastCall as unknown as [string, { id: string; action: { label: string; onClick: () => void } }]
    expect(said).toBe("Took every shot in")
    expect(options.action.label).toBe("Undo")
    options.action.onClick()
    expect(it.restore).toHaveBeenCalledExactlyOnceWith(before)
    // The next one replaces this toast: only the last click can be taken back.
    press("I", { shiftKey: true })
    await waitFor(() => expect(toast).toHaveBeenCalledTimes(2))
    expect((vi.mocked(toast).mock.lastCall as unknown as [string, { id: string }])[1].id).toBe(options.id)
  })

  it("says nothing when the click changed nothing", async () => {
    const it = pack(null, [clip, out])
    vi.mocked(it.setEach).mockResolvedValue(false)
    await show(null, it)
    press("a")
    await waitFor(() => expect(it.setEach).toHaveBeenCalledOnce())
    expect(toast).not.toHaveBeenCalled()
  })

  it("handing the picks back can be taken back too", async () => {
    const picked = { ...clip, manual: true } as PackClip
    const it = pack(null, [picked, out])
    await show(null, it)
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    fireEvent.click(await entry(/Hand every shot back to the filters/))
    await waitFor(() => expect(toast).toHaveBeenCalledOnce())
    const [said, options] = vi.mocked(toast).mock.lastCall as unknown as [string, { action: { onClick: () => void } }]
    expect(said).toBe("Handed 1 shot back to the filters")
    options.action.onClick()
    expect(it.restore).toHaveBeenCalledExactlyOnceWith([picked, out])
  })

  it("under a limit it says the shots are picked by hand", async () => {
    const it = pack(null, [clip, out])
    it.state = { ...it.state!, spec: { ...DEFAULT_SPEC, limit: 3 } }
    await show(null, it)
    press("a")
    await waitFor(() => expect(toast).toHaveBeenCalledOnce())
    expect((vi.mocked(toast).mock.lastCall as unknown as [string, { description?: string }])[1].description).toMatch(/limit/)
  })

  it("offers the hand-back only with picks, and every entry shows its key", async () => {
    await show(null, pack(null, [clip, out]))
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    await entry(/Invert the pack/)
    expect(screen.queryByRole("menuitem", { name: /Hand every shot back to the filters/ })).toBeNull()
    expect((await entry(/Take every shot/)).textContent).toContain("A")
    expect((await entry(/Leave every shot out/)).textContent).toMatch(/Ctrl.*Shift.*A|Shift.*A/)
    expect((await entry(/Invert the pack/)).textContent).toMatch(/I/)
  })

  it("with every shot in, the entry that takes doesn't promise the key that leaves them out", async () => {
    await show(null, pack())
    fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false })
    const take = await entry(/Take every shot/)
    expect(take.getAttribute("aria-disabled")).toBe("true")
    expect(take.textContent).not.toContain("Ctrl")
  })

  it("heads the plugins' entries with what they wait for, whichever menu", async () => {
    await show(null, pack(null, [out]))
    for (const open of [
      () => fireEvent.pointerDown(dots(), { button: 0, ctrlKey: false }),
      () => fireEvent.contextMenu(screen.getByText(/Click to take a shot in or out/), { clientX: 40, clientY: 40 }),
    ]) {
      open()
      expect(await screen.findByText("Save with a plugin needs a shot in the pack")).toBeTruthy()
      fireEvent.keyDown(await screen.findByRole("menuitem", { name: /Save as FCPXML…/ }), { key: "Escape" })
      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    }
  })
})
