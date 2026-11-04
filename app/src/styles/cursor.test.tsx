// The app's own controls, as Radix and cmdk render them, against the cursor rule in base.css:
// the hand where a click acts, the arrow on disabled controls, menu and list entries, fields,
// drag regions and the caption buttons.
import baseCss from "./base.css?raw"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { Command } from "cmdk"
import { afterEach, describe, expect, it, vi } from "vitest"
import { WindowControls } from "@/components/shell/WindowControls"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Field } from "@/components/ui/input"
import { Segmented } from "@/components/ui/segmented"
import { Select } from "@/components/ui/select"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: () => Promise.resolve(false),
    onResized: () => Promise.resolve(() => {}),
    minimize: () => Promise.resolve(),
    toggleMaximize: () => Promise.resolve(),
    close: () => Promise.resolve(),
  }),
}))

const css = baseCss.replace(/\/\*[\s\S]*?\*\//g, "")
const rule = css.match(/(:where\([^{]*\))\s*\{\s*cursor:\s*pointer;\s*&(:is\([^{]*\))\s*\{\s*cursor:\s*default;/)
if (!rule) throw new Error("the hand rule in base.css isn't where this test reads it")
const HAND = rule[1].replace(/\s+/g, " ")
const ARROW = `${HAND}${rule[2].replace(/\s+/g, " ")}`

// A Tailwind utility beats the base layer, so an element that pins the arrow shows it whatever
// the rule says. `data-[disabled]:cursor-default` is the disabled state and doesn't count.
const pinsArrow = (el: Element) => /(^|\s)cursor-(default|auto)(\s|$)/.test(el.getAttribute("class") ?? "")
const hand = (el: Element) => el.matches(HAND) && !el.matches(ARROW) && !pinsArrow(el)
const arrow = (el: Element) => !el.matches(HAND) || el.matches(ARROW) || pinsArrow(el)
const sliderRoot = (name: string) => screen.getByRole("slider", { name }).closest("[data-orientation]:not([role])")

afterEach(cleanup)

describe("the cursor", () => {
  it("buttons, links, switches and the labels that work them show the hand", () => {
    render(
      <>
        <Button>Go</Button>
        <a href="#/x">A link</a>
        <Switch aria-label="On" />
        <label>
          <span>All shots in one file</span>
          <Switch aria-label="Merge" />
        </label>
        <Field label="Fast Scan" htmlFor="fs">
          <Switch id="fs" />
        </Field>
        <Field label="Wrapped scan" htmlFor="ws">
          <div className="flex h-9 items-center">
            <Switch id="ws" />
          </div>
        </Field>
        <Checkbox label="Don't show again" />
        <ul>
          <li role="link">Open</li>
        </ul>
        <div role="tab">Tab</div>
        <a href="#/y" role="menuitem">Menu link</a>
        {/* oxlint-disable-next-line react/forbid-dom-props -- a plugin page's native checkbox */}
        <input type="checkbox" aria-label="Raw box" />
        <Field label="Locked scan" htmlFor="ls">
          <div>
            <Switch id="ls" disabled />
          </div>
        </Field>
        <button data-disabled="false">Working</button>
        <Select aria-label="Format" value="a" onChange={() => {}} options={[{ value: "a", label: "Alpha" }]} />
      </>,
    )
    expect(hand(screen.getByText("Wrapped scan"))).toBe(true)
    expect(hand(screen.getByText("Don't show again").closest("label")!)).toBe(true)
    expect(hand(screen.getByRole("checkbox", { name: "Don't show again" }))).toBe(true)
    expect(hand(screen.getByRole("link", { name: "Open" }))).toBe(true)
    expect(hand(screen.getByText("Tab"))).toBe(true)
    expect(arrow(screen.getByRole("menuitem", { name: "Menu link" }))).toBe(true)
    expect(hand(screen.getByRole("checkbox", { name: "Raw box" }))).toBe(true)
    expect(arrow(screen.getByText("Locked scan"))).toBe(true)
    // data-disabled="false" (as cmdk writes it) is not disabled.
    expect(hand(screen.getByRole("button", { name: "Working" }))).toBe(true)
    expect(hand(screen.getByRole("button", { name: "Go" }))).toBe(true)
    expect(hand(screen.getByText("A link"))).toBe(true)
    expect(hand(screen.getByRole("switch", { name: "On" }))).toBe(true)
    expect(hand(screen.getByText("All shots in one file").closest("label")!)).toBe(true)
    expect(hand(screen.getByText("Fast Scan"))).toBe(true)
    expect(hand(screen.getByRole("combobox", { name: "Format" }))).toBe(true)
  })

  it("segmented options and sliders show the hand, the slider's track too", () => {
    render(
      <>
        <Segmented
          aria-label="View"
          value="grid"
          onChange={() => {}}
          options={[
            { value: "grid", label: "Grid" },
            { value: "list", label: "List" },
          ]}
        />
        <Slider label="Volume" value={0.5} onChange={() => {}} />
      </>,
    )
    expect(hand(screen.getByRole("radio", { name: "Grid" }))).toBe(true)
    expect(hand(screen.getByRole("radio", { name: "List" }))).toBe(true)
    expect(hand(screen.getByRole("slider", { name: "Volume" }))).toBe(true)
    // A click on the track jumps there: the slider's root takes the hand as well.
    expect(sliderRoot("Volume")?.getAttribute("class")).toMatch(/(^|\s)cursor-pointer(\s|$)/)
  })

  it("menu entries, select options, palette entries and a list's options keep the arrow (cmdk's working entries too)", async () => {
    const user = userEvent.setup()
    render(
      <>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button>Options</Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            <DropdownMenuItem>Rename…</DropdownMenuItem>
            <DropdownMenuCheckboxItem checked>Solo</DropdownMenuCheckboxItem>
            <DropdownMenuRadioGroup value="a">
              <DropdownMenuRadioItem value="a">By name</DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
        <Command>
          <Command.List>
            <Command.Item>Settings</Command.Item>
          </Command.List>
        </Command>
        <div role="listbox" aria-label="People">
          <button role="option" aria-selected={false}>
            Pick me
          </button>
        </div>
        <Select
          aria-label="Format"
          value="a"
          onChange={() => {}}
          options={[
            { value: "a", label: "Alpha" },
            { value: "b", label: "Beta" },
          ]}
        />
      </>,
    )
    // cmdk writes "false" on the entries that work, which must not read as disabled.
    const entry = screen.getByRole("option", { name: "Settings" })
    expect(entry.getAttribute("data-disabled")).toBe("false")
    expect(arrow(entry)).toBe(true)
    expect(arrow(screen.getByRole("option", { name: "Pick me" }))).toBe(true)
    await user.click(screen.getByRole("button", { name: "Options" }))
    expect(arrow(await screen.findByRole("menuitem", { name: "Rename…" }))).toBe(true)
    expect(arrow(screen.getByRole("menuitemcheckbox", { name: "Solo" }))).toBe(true)
    expect(arrow(screen.getByRole("menuitemradio", { name: "By name" }))).toBe(true)
    await user.keyboard("{Escape}")
    screen.getByRole("combobox", { name: "Format" }).focus()
    await user.keyboard("{Enter}")
    expect(arrow(await screen.findByRole("option", { name: "Beta" }))).toBe(true)
  })

  it("disabled controls keep the arrow", async () => {
    const user = userEvent.setup()
    render(
      <>
        <Button disabled>Not now</Button>
        <button disabled>Raw</button>
        <Switch aria-label="Off" disabled />
<div>        <label>
          <span>Locked option</span>
          <Switch aria-label="Locked" disabled />
        </label>
</div>        <Segmented
          aria-label="View"
          value="grid"
          onChange={() => {}}
          options={[
            { value: "grid", label: "Grid" },
            { value: "off", label: "Off", disabled: true },
          ]}
        />
        <Slider label="Volume" value={0.5} onChange={() => {}} disabled />
        <Command>
          <Command.List>
            <Command.Item disabled>Nothing</Command.Item>
          </Command.List>
        </Command>
        <button role="tab" aria-disabled="true">
          Gone
        </button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button>Options</Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            <DropdownMenuItem disabled>Remove</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </>,
    )
    expect(arrow(screen.getByRole("button", { name: "Not now" }))).toBe(true)
    expect(arrow(screen.getByRole("button", { name: "Raw" }))).toBe(true)
    expect(arrow(screen.getByRole("switch", { name: "Off" }))).toBe(true)
    expect(arrow(screen.getByText("Locked option").closest("label")!)).toBe(true)
    expect(arrow(screen.getByRole("radio", { name: "Off" }))).toBe(true)
    expect(arrow(screen.getByRole("slider", { name: "Volume" }))).toBe(true)
    expect(arrow(screen.getByRole("option", { name: "Nothing" }))).toBe(true)
    expect(arrow(screen.getByRole("tab", { name: "Gone" }))).toBe(true)
    // The slider's root holds the arrow of its own for the disabled state.
    expect(sliderRoot("Volume")?.getAttribute("class")).toMatch(/data-\[disabled\]:cursor-default/)
    await user.click(screen.getByRole("button", { name: "Options" }))
    expect(arrow(await screen.findByRole("menuitem", { name: "Remove" }))).toBe(true)
  })

  it("text fields, drag regions, the caption buttons and a link without a target never show the hand", () => {
    render(
      <>
        <Field label="Title" htmlFor="t">
          <input id="t" />
        </Field>
        <textarea aria-label="Notes" />
        <header data-tauri-drag-region>
          <h1 data-tauri-drag-region>Window title</h1>
        </header>
        <a>No target</a>
        <WindowControls />
      </>,
    )
    expect(arrow(screen.getByRole("textbox", { name: "Title" }))).toBe(true)
    expect(arrow(screen.getByText("Title", { selector: "label" }))).toBe(true)
    expect(arrow(screen.getByRole("textbox", { name: "Notes" }))).toBe(true)
    expect(arrow(screen.getByRole("banner"))).toBe(true)
    expect(arrow(screen.getByRole("heading"))).toBe(true)
    expect(arrow(screen.getByText("No target"))).toBe(true)
    for (const name of ["Minimize", "Maximize", "Close"]) {
      const caption = screen.getByRole("button", { name })
      // A button the rule would give the hand: its own class holds the arrow, like Windows' own.
      expect(caption.matches(HAND)).toBe(true)
      expect(pinsArrow(caption)).toBe(true)
    }
  })
})
