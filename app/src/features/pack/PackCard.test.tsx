import type * as React from "react"
import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it } from "vitest"
import type { MediaItem, PackClip, PackFilter } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { PackCard } from "./PackCard"

const speaks: PackFilter = {
  key: "dev.example.speech-filter/speaks",
  title: "Speaks",
  description: null,
  reason: "No speech",
  plugin: "Speech Filter",
  checked: 3,
  total: 3,
  state: "ready",
}
const clip = (more: Partial<PackClip>): PackClip => ({
  shot: { mediaId: "m", start: 0, end: 48, sprite: null, cells: 0, trackId: 1, face: "", faceHeight: 0.3 },
  start: 0,
  end: 48,
  size: "close_up",
  quality: 1,
  coverage: 1,
  others: 0,
  score: 1,
  included: false,
  byRules: false,
  reason: "plugin",
  manual: null,
  filter: 0,
  ...more,
})
const wrapper = ({ children }: { children: React.ReactNode }) => <TooltipProvider>{children}</TooltipProvider>
const card = (c: PackClip, filter?: PackFilter, media?: MediaItem) =>
  render(<PackCard clip={c} index={0} media={media} filter={filter} onToggle={() => {}} onOpen={() => {}} />, { wrapper })
/** An episode with a frame rate: the card's tooltip says where the shot is, and the rest. */
const episode = { id: "m", label: "S01E01", fps: { num: 24, den: 1 } } as MediaItem
/** The card's tooltip, once it has the keyboard's focus. */
const tooltip = () => {
  fireEvent.focus(screen.getByRole("button", { name: /^Shot/ }))
  return screen.getAllByText(/Click to/)[0].textContent ?? ""
}

afterEach(cleanup)

describe("PackCard, plugin filters", () => {
  it("a shot a plugin's filter leaves out wears the filter's own words", () => {
    card(clip({}), speaks)
    expect(screen.getByText("No speech")).toBeTruthy()
    expect(screen.getByRole("button", { name: /left out: No speech/ })).toBeTruthy()
  })

  it("wears a general reason when it can't tell which filter, or the filter has no words", () => {
    card(clip({ filter: null }))
    expect(screen.getByText("Plugin filter")).toBeTruthy()
    cleanup()
    card(clip({}), { ...speaks, reason: "" })
    expect(screen.getByText("Plugin filter")).toBeTruthy()
  })

  it("a shot taken back in by hand shows in, not left out", () => {
    card(clip({ included: true, manual: true, reason: null }), speaks)
    expect(screen.queryByText("No speech")).toBeNull()
    expect(screen.getByRole("button", { name: /in the pack/ })).toBeTruthy()
  })

  it("a shot taken back in by hand says in its tooltip which filter would leave it out", () => {
    card(clip({ included: true, manual: true, reason: null }), speaks, episode)
    expect(tooltip()).toContain("“Speaks” would leave it out")
    cleanup()
    // One the filters take says nothing of it.
    card(clip({ included: true, byRules: true, reason: null, filter: null }), undefined, episode)
    expect(tooltip()).not.toContain("would leave it out")
  })

  it("a word the filter may have given with its settings before says so", () => {
    // Every shot checked with its settings as they are: the word is today's.
    card(clip({}), speaks, episode)
    expect(tooltip()).not.toContain("settings before")
    cleanup()
    // None checked yet: the word is from before for sure.
    card(clip({}), { ...speaks, checked: 0, state: "failed" }, episode)
    expect(tooltip()).toContain("“Speaks” said so with its settings before")
    cleanup()
    card(clip({}), { ...speaks, checked: 1, state: "checking" }, episode)
    expect(tooltip()).toContain("“Speaks” may have said so with its settings before")
  })
})
