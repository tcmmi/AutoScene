import type * as React from "react"
import { useState } from "react"
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { MediaItem, PackFilter, PackSpec } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { layerOpen } from "@/lib/layers"
import { pluginInfo } from "@/test/plugins"
import { PackFilters } from "./PackFilters"
import { DEFAULT_SPEC } from "./spec"

const KEY = "dev.example.speech-filter/speaks"
const ICON = `data:image/svg+xml;base64,${btoa('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="24" height="24"/></svg>')}`
const plugins = [pluginInfo({ id: "dev.example.speech-filter", name: "Speech Filter", icon: ICON })]
const speaks = (more: Partial<PackFilter> = {}): PackFilter => ({
  key: KEY,
  title: "Speaks",
  description: "Keeps the shots where they talk.",
  reason: "No speech",
  plugin: "Speech Filter",
  checked: 0,
  total: 312,
  state: "ready",
  ...more,
})

const wrapper = ({ children }: { children: React.ReactNode }) => <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
const bar = (filters: PackFilter[], spec: PackSpec = DEFAULT_SPEC, onChange: (s: PackSpec) => void = () => {}) =>
  render(<PackFilters spec={spec} media={[]} faceless={false} filters={filters} plugins={plugins} onChange={onChange} />, { wrapper })
const chip = () => screen.getByRole("button", { name: /Speaks/ })

afterEach(cleanup)

describe("PackFilters, plugin filters", () => {
  it("shows no chip without a plugin that filters shots", () => {
    bar([])
    expect(screen.queryByRole("button", { name: /Speaks/ })).toBeNull()
    expect(screen.getByRole("button", { name: "Filters" })).toBeTruthy()
  })

  it("a plugin filter chip names its plugin", () => {
    const onChange = vi.fn()
    bar([speaks()], DEFAULT_SPEC, onChange)
    // After AutoScene's own chips, with the plugin's mark.
    const group = screen.getByRole("group", { name: "Filters" })
    const chips = within(group).getAllByRole("button", { pressed: false })
    expect(chips.at(-1)).toBe(chip())
    expect(chip().querySelector("img")?.getAttribute("src")).toBe(ICON)
    // Its tooltip says whose it is, and what it keeps.
    fireEvent.focus(chip())
    expect(screen.getAllByText("Plugin “Speech Filter”: Keeps the shots where they talk.").length).toBeGreaterThan(0)
    // A click turns it on.
    fireEvent.click(chip())
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, pluginFilters: [KEY] })
  })

  it("turns off with a second click, and resets with the others", () => {
    const onChange = vi.fn()
    const on = { ...DEFAULT_SPEC, pluginFilters: [KEY, "other/filter"] }
    bar([speaks()], on, onChange)
    expect(chip().getAttribute("aria-pressed")).toBe("true")
    fireEvent.click(chip())
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, pluginFilters: ["other/filter"] })
    fireEvent.click(screen.getByRole("button", { name: /Reset filters/ }))
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_SPEC)
  })

  it("a filter that checks shows a ring, and how far in its tooltip", () => {
    bar([speaks({ state: "checking", checked: 120 })], { ...DEFAULT_SPEC, pluginFilters: [KEY] })
    const ring = within(chip()).getByRole("progressbar", { name: "Checking Speaks" })
    expect(ring.getAttribute("aria-valuenow")).toBe("38")
    fireEvent.focus(chip())
    expect(screen.getAllByText("Checking 120 of 312 shots").length).toBeGreaterThan(0)
  })

  it("a filter whose check failed shows a warning mark, and what it couldn't check", () => {
    bar([speaks({ state: "failed", checked: 192 })], { ...DEFAULT_SPEC, pluginFilters: [KEY] })
    expect(within(chip()).getByRole("img", { name: "Couldn't check Speaks" })).toBeTruthy()
    fireEvent.focus(chip())
    expect(screen.getAllByText("Couldn't check 120 shots").length).toBeGreaterThan(0)
    cleanup()
    // Not turned on, or every shot judged after all: no mark.
    bar([speaks({ state: "failed", checked: 192 })])
    expect(within(chip()).queryByRole("img", { name: /Couldn't check/ })).toBeNull()
    cleanup()
    bar([speaks({ state: "failed", checked: 312 })], { ...DEFAULT_SPEC, pluginFilters: [KEY] })
    expect(within(chip()).queryByRole("img", { name: /Couldn't check/ })).toBeNull()
  })

  it("a filter that is not turned on has no ring", () => {
    // Turned off while its check goes on: the pack no longer waits for it.
    bar([speaks({ state: "checking" })])
    expect(within(chip()).queryByRole("progressbar")).toBeNull()
  })

  it("a filter of a plugin that is off shows dimmed", () => {
    const onChange = vi.fn()
    bar([speaks({ state: "off" })], { ...DEFAULT_SPEC, pluginFilters: [KEY] }, onChange)
    expect(within(chip()).getByText("Plugin off")).toBeTruthy()
    expect(chip().className).toContain("opacity-60")
    fireEvent.focus(chip())
    expect(screen.getAllByText(/Plugin “Speech Filter” is off/).length).toBeGreaterThan(0)
    // The pack ignores it; the chip can still be turned off, and then it goes.
    fireEvent.click(chip())
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_SPEC)
  })

  it("a filter no plugin has now asks only to be turned off", () => {
    const onChange = vi.fn()
    bar([speaks({ state: "gone", title: "Removed filter", plugin: "" })], { ...DEFAULT_SPEC, pluginFilters: [KEY] }, onChange)
    const removed = screen.getByRole("button", { name: /Removed filter/ })
    expect(within(removed).queryByText("Plugin off")).toBeNull()
    expect(removed.className).toContain("opacity-60")
    fireEvent.focus(removed)
    expect(screen.getAllByText("This filter's plugin is gone, or no longer has it. Turn this filter off.").length).toBeGreaterThan(0)
    expect(screen.queryByText(/Turn it on in Plugins/)).toBeNull()
    fireEvent.click(removed)
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_SPEC)
  })
})

const media = (...ids: string[]) => ids.map((id) => ({ id, fileName: `${id}.mkv`, path: `${id}.mkv` }) as MediaItem)

/** The whole bar, as a person changes it: each change comes back as the next spec. */
function Bar({
  start = DEFAULT_SPEC,
  episodes = media("a", "b", "c"),
  faceless = false,
  filters = [] as PackFilter[],
  seen = undefined as ((spec: PackSpec) => void) | undefined,
}) {
  const [spec, setSpec] = useState(start)
  return (
    <PackFilters
      spec={spec}
      media={episodes}
      faceless={faceless}
      filters={filters}
      plugins={plugins}
      onChange={(next) => {
        seen?.(next)
        setSpec(next)
      }}
    />
  )
}
const open = (name: string | RegExp = /^Filters/) => {
  const trigger = screen.getByRole("button", { name })
  trigger.focus()
  fireEvent.keyDown(trigger, { key: "Enter" })
  return screen.getByRole("menu")
}
const press = (key: string) => fireEvent.keyDown(document.activeElement ?? document.body, { key })
/** A list of its own, opened from its entry in the Filters menu: its row and the list. */
const openList = async (name: RegExp) => {
  const row = within(open()).getByRole("menuitem", { name })
  row.focus()
  press("ArrowRight")
  return { row, list: (await screen.findAllByRole("menu")).at(-1) as HTMLElement }
}
const focused = () => document.activeElement?.textContent ?? ""
/** Down, as often as it takes to reach `entry` (Radix moves the focus a tick later). */
const downTo = async (entry: string) => {
  for (let i = 0; i < 12 && focused() !== entry; i++) {
    const was = focused()
    press("ArrowDown")
    await waitFor(() => expect(focused()).not.toBe(was))
  }
  expect(focused()).toBe(entry)
}

describe("PackFilters, the Filters menu", () => {
  it("holds every filter once: nothing beside it, and the handles stay in the bar", () => {
    render(<Bar faceless />, { wrapper })
    const group = screen.getByRole("group", { name: "Filters" })
    expect(within(group).getAllByRole("button").map((b) => b.textContent?.trim())).toEqual(["Filters", "No handles"])
    const menu = open()
    expect(within(menu).getAllByRole("menuitemcheckbox").map((i) => i.textContent)).toEqual([
      "Extreme close-up",
      "Close-up",
      "Medium close-up",
      "Medium shot",
      "Wide shot",
      "Only them in the shot",
      "Skip flash cuts",
      "Only with their faceLeaves out the shots found by their body",
    ])
    expect(within(menu).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["LengthAny length", "EpisodesAll", "LimitNo limit"])
  })

  it("shows the face filter only when it applies, and the episodes only with more than one", () => {
    render(<Bar episodes={media("a")} />, { wrapper })
    const menu = open()
    expect(within(menu).queryByRole("menuitemcheckbox", { name: /Only with their face/ })).toBeNull()
    expect(within(menu).queryByRole("menuitem", { name: /Episodes/ })).toBeNull()
  })

  it("counts what is on, plugin filters too, and leaves the order and the handles out", () => {
    render(<Bar filters={[speaks()]} start={{ ...DEFAULT_SPEC, order: "best", handles: 12 }} />, { wrapper })
    expect(screen.getByRole("button", { name: "Filters" })).toBeTruthy()
    fireEvent.click(chip())
    const count = screen.getByRole("button", { name: "Filters, 1 on" })
    expect(count.textContent?.trim()).toBe("Filters1")
    const menu = open(/^Filters/)
    fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: "Wide shot" }))
    fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: "Close-up" }))
    fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: "Only them in the shot" }))
    // Two sizes are one filter; the menu is still open for the next.
    expect(screen.getByRole("menu")).toBe(menu)
    expect(within(menu).getByRole("menuitemcheckbox", { name: "Wide shot" }).getAttribute("aria-checked")).toBe("true")
    press("Escape")
    expect(screen.getByRole("button", { name: "Filters, 3 on" }).textContent?.trim()).toBe("Filters3")
  })

  it("works by keyboard, a step further and back, and Esc closes it where it was opened", async () => {
    render(<Bar />, { wrapper })
    open()
    // The first entry has the focus, Space ticks and stays, Down moves on past the entries with a list.
    await waitFor(() => expect(focused()).toBe("Extreme close-up"))
    press(" ")
    expect(focused()).toBe("Extreme close-up")
    expect(document.activeElement?.getAttribute("aria-checked")).toBe("true")
    await downTo("LengthAny length")
    // Right opens the list, a choice shows at once and the list stays.
    press("ArrowRight")
    const list = (await screen.findAllByRole("menu")).at(-1) as HTMLElement
    await waitFor(() => expect(list.contains(document.activeElement)).toBe(true))
    await waitFor(() => expect(focused()).toBe("Any length"))
    await downTo("1 s or longer")
    press("Enter")
    expect(within(list).getByRole("menuitemradio", { name: "1 s or longer" }).getAttribute("aria-checked")).toBe("true")
    expect(list.isConnected).toBe(true)
    // Left comes back, with the choice beside its name.
    press("ArrowLeft")
    await waitFor(() => expect(focused()).toBe("Length1 s or longer"))
    // Esc from a step further closes the lot, and the focus is back on the button.
    press("ArrowRight")
    await waitFor(() => expect(screen.getAllByRole("menu").length).toBe(2))
    press("Escape")
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Filters, 2 on" }))
  })

  it("a menu of filters says it is open, so the pack's keys leave it alone", () => {
    render(<Bar />, { wrapper })
    expect(layerOpen()).toBe(false)
    open()
    expect(layerOpen()).toBe(true)
  })

  it("picks episodes, one at least, and All takes them all again", async () => {
    render(<Bar />, { wrapper })
    const row = within(open()).getByRole("menuitem", { name: /Episodes/ })
    row.focus()
    press("ArrowRight")
    const list = (await screen.findAllByRole("menu")).at(-1) as HTMLElement
    const entry = (name: string) => within(list).getByRole("menuitemcheckbox", { name })
    expect(entry("All episodes").getAttribute("aria-disabled")).toBe("true")
    fireEvent.click(entry("a"))
    fireEvent.click(entry("b"))
    expect(row.textContent).toBe("Episodes1 of 3")
    expect(entry("c").getAttribute("aria-disabled")).toBe("true")
    fireEvent.click(entry("All episodes"))
    expect(row.textContent).toBe("EpisodesAll")
  })

  it("a range set elsewhere still shows, beside Length", () => {
    render(<Bar start={{ ...DEFAULT_SPEC, minSec: 3, maxSec: 9 }} />, { wrapper })
    expect(within(open()).getByRole("menuitem", { name: /Length/ }).textContent).toBe("Length3 to 9 s")
  })

  it("the handles keep their own menu, and Reset shows once anything differs", () => {
    render(<Bar />, { wrapper })
    expect(screen.queryByRole("button", { name: /Reset filters/ })).toBeNull()
    const menu = open("No handles")
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: "12 frames before and after" }))
    expect(screen.getByRole("button", { name: "Handles 12 fr" })).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: /Reset filters/ }))
    expect(screen.getByRole("button", { name: "No handles" })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /Reset filters/ })).toBeNull()
  })

  it("picks a limit, the list stays, and No limit takes it back", async () => {
    const seen = vi.fn()
    render(<Bar seen={seen} />, { wrapper })
    const { row, list } = await openList(/Limit/)
    fireEvent.click(within(list).getByRole("menuitemradio", { name: "The best 50" }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, limit: 50 })
    expect(row.textContent).toBe("LimitBest 50")
    expect(list.isConnected).toBe(true)
    fireEvent.click(within(list).getByRole("menuitemradio", { name: "No limit" }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, limit: null })
    expect(row.textContent).toBe("LimitNo limit")
    expect(list.isConnected).toBe(true)
  })

  it("picks a length: both ends, the list stays", async () => {
    const seen = vi.fn()
    render(<Bar seen={seen} />, { wrapper })
    const { row, list } = await openList(/Length/)
    fireEvent.click(within(list).getByRole("menuitemradio", { name: "2 to 10 s" }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, minSec: 2, maxSec: 10 })
    expect(row.textContent).toBe("Length2 to 10 s")
    expect(list.isConnected).toBe(true)
    fireEvent.click(within(list).getByRole("menuitemradio", { name: "Up to 6 s" }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, minSec: null, maxSec: 6 })
    fireEvent.click(within(list).getByRole("menuitemradio", { name: "Any length" }))
    expect(seen).toHaveBeenLastCalledWith(DEFAULT_SPEC)
  })

  it("flash cuts and the solo filter toggle, and show in the count", () => {
    const seen = vi.fn()
    render(<Bar seen={seen} />, { wrapper })
    const menu = open()
    const flash = within(menu).getByRole("menuitemcheckbox", { name: "Skip flash cuts" })
    expect(flash.getAttribute("aria-checked")).toBe("true")
    fireEvent.click(flash)
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, skipTiny: false })
    expect(flash.getAttribute("aria-checked")).toBe("false")
    // Letting flash cuts in differs from the default: it counts.
    expect(screen.getByRole("button", { name: "Filters, 1 on", hidden: true })).toBeTruthy()
    fireEvent.click(flash)
    expect(seen).toHaveBeenLastCalledWith(DEFAULT_SPEC)
    fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: "Only them in the shot" }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, solo: true })
  })

  it("the shot sizes keep their order, whatever the order they are ticked in", () => {
    const seen = vi.fn()
    render(<Bar seen={seen} />, { wrapper })
    const menu = open()
    for (const name of ["Wide shot", "Extreme close-up", "Medium shot"]) fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, sizes: ["extreme_close_up", "medium", "wide"] })
    fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: "Medium shot" }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, sizes: ["extreme_close_up", "wide"] })
  })

  it("the face filter toggles, and shows for a saved choice though no shot is without a face", () => {
    const seen = vi.fn()
    render(<Bar faceless seen={seen} />, { wrapper })
    const face = within(open()).getByRole("menuitemcheckbox", { name: /Only with their face/ })
    expect(face.getAttribute("aria-checked")).toBe("false")
    fireEvent.click(face)
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, skipFaceless: true })
    expect(face.getAttribute("aria-checked")).toBe("true")
    expect(screen.getByRole("button", { name: "Filters, 1 on", hidden: true })).toBeTruthy()
    cleanup()
    // Saved with the pack: it shows, so it can be turned off.
    render(<Bar start={{ ...DEFAULT_SPEC, skipFaceless: true }} />, { wrapper })
    expect(within(open()).getByRole("menuitemcheckbox", { name: /Only with their face/ }).getAttribute("aria-checked")).toBe("true")
  })

  it("ticking every episode back by hand is all of them again", async () => {
    const seen = vi.fn()
    render(<Bar start={{ ...DEFAULT_SPEC, media: ["a", "b"] }} seen={seen} />, { wrapper })
    const { row, list } = await openList(/Episodes/)
    expect(row.textContent).toBe("Episodes2 of 3")
    fireEvent.click(within(list).getByRole("menuitemcheckbox", { name: "c" }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, media: [] })
    expect(row.textContent).toBe("EpisodesAll")
    expect(screen.queryByRole("button", { name: /Reset filters/ })).toBeNull()
  })

  it("keeps the episodes in the project's order", async () => {
    const seen = vi.fn()
    render(<Bar start={{ ...DEFAULT_SPEC, media: ["c"] }} seen={seen} />, { wrapper })
    const { list } = await openList(/Episodes/)
    fireEvent.click(within(list).getByRole("menuitemcheckbox", { name: "a" }))
    expect(seen).toHaveBeenLastCalledWith({ ...DEFAULT_SPEC, media: ["a", "c"] })
  })

  it("counts only the episodes still in the project; a pack for all that are left is no filter", async () => {
    // Narrowed to a and an episode since removed: 1 of 3.
    render(<Bar start={{ ...DEFAULT_SPEC, media: ["a", "gone"] }} />, { wrapper })
    const { row } = await openList(/Episodes/)
    expect(row.textContent).toBe("Episodes1 of 3")
    cleanup()
    // Narrowed to the two there are, the third since removed: all of them, no filter.
    render(<Bar start={{ ...DEFAULT_SPEC, media: ["a", "b"] }} episodes={media("a", "b")} />, { wrapper })
    const again = await openList(/Episodes/)
    expect(again.row.textContent).toBe("EpisodesAll")
    expect(within(again.list).getByRole("menuitemcheckbox", { name: "All episodes" }).getAttribute("aria-checked")).toBe("true")
    expect(screen.getByRole("button", { name: "Filters", hidden: true })).toBeTruthy()
  })

  it("a long series scrolls inside its list, and All episodes stays above it", async () => {
    const many = media(...Array.from({ length: 60 }, (_, i) => `e${i}`))
    render(<Bar episodes={many} />, { wrapper })
    const { list } = await openList(/Episodes/)
    const all = within(list).getByRole("menuitemcheckbox", { name: "All episodes" })
    const scroller = within(list).getByRole("menuitemcheckbox", { name: "e0" }).parentElement as HTMLElement
    expect(scroller.className).toContain("overflow-y-auto")
    expect(scroller.contains(all)).toBe(false)
    expect(scroller.children.length).toBe(60)
  })

  it("a cut episode name shows the rest in a tooltip, which takes the first Esc (the menu stays)", async () => {
    const widths = (w: { scroll: number; client: number }) => {
      Object.defineProperty(HTMLElement.prototype, "scrollWidth", {
        configurable: true,
        get(this: HTMLElement) {
          return this.classList.contains("truncate") ? w.scroll : 0
        },
      })
      Object.defineProperty(HTMLElement.prototype, "clientWidth", {
        configurable: true,
        get(this: HTMLElement) {
          return this.classList.contains("truncate") ? w.client : 0
        },
      })
    }
    widths({ scroll: 500, client: 300 })
    try {
      const long = ["a", "b"].map((id) => ({ id, fileName: `The very long name of episode ${id}.mkv`, path: `${id}.mkv` }) as MediaItem)
      render(<Bar episodes={long} />, { wrapper })
      const { list } = await openList(/Episodes/)
      const name = list.querySelector(".truncate") as HTMLElement
      expect(name.textContent).toContain("The very long name of episode a")
      fireEvent.pointerMove(name, { pointerType: "mouse" })
      await waitFor(() => expect(screen.getByRole("tooltip").textContent).toContain("The very long name of episode a"))
      // The tooltip is the topmost layer: the first Esc closes it, the second the menu.
      press("Escape")
      await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull())
      expect(screen.getAllByRole("menu").length).toBe(2)
      press("Escape")
      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    } finally {
      Reflect.deleteProperty(HTMLElement.prototype, "scrollWidth")
      Reflect.deleteProperty(HTMLElement.prototype, "clientWidth")
    }
  })

  it("a name that fits has no tooltip", async () => {
    render(<Bar />, { wrapper })
    const { list } = await openList(/Episodes/)
    fireEvent.pointerMove(list.querySelector(".truncate") as HTMLElement, { pointerType: "mouse" })
    await new Promise((r) => setTimeout(r, 50))
    expect(screen.queryByRole("tooltip")).toBeNull()
  })
})
