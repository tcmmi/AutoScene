import type * as React from "react"
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { PackFilter, PackSpec } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
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
    expect(screen.getByRole("button", { name: "Skip flash cuts" })).toBeTruthy()
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
