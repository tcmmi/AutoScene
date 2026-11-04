import type * as React from "react"
import { useState } from "react"
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { Character, CharacterShot, MediaItem, ShotGroup } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { PageEntry, PageScrollElement } from "@/components/ui/page-scroll"
import { TooltipProvider } from "@/components/ui/tooltip"
import { CharacterPage } from "./CharacterPage"

const hoisted = vi.hoisted(() => ({ navigate: vi.fn() }))

const walter = { id: "walter", label: "Walter", name: "Walter", hidden: false, shots: 6, tracks: 6, media: 1, waiting: 0 } as unknown as Character
const media = [{ id: "e1", label: "E01", fps: { num: 24, den: 1 } } as MediaItem]
const shot = (start: number): CharacterShot => ({
  mediaId: "e1",
  start,
  end: start + 48,
  sprite: null,
  cells: 0,
  trackId: start + 1,
  face: `f${start}.jpg`,
  faceHeight: 0.2,
})
let groups: ShotGroup[] | undefined
/** The history entry the page is on: Back comes to the same one, a new visit to a new one. */
let entry = 0

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useParams: () => ({ pid: "p", cid: "walter" }),
  useNavigate: () => hoisted.navigate,
  useCanGoBack: () => false,
  Link: ({ children }: { children: React.ReactNode }) => <a>{children}</a>,
}))
// The page around it scrolls: the grid only needs to know in what.
vi.mock("@/components/ui/page", () => ({
  Page: function Page({ children }: { children: React.ReactNode }) {
    const [el, setEl] = useState<HTMLElement | null>(null)
    return (
      <main ref={setEl}>
        <PageEntry.Provider value={`entry-${entry}`}>
          <PageScrollElement.Provider value={el}>{children}</PageScrollElement.Provider>
        </PageEntry.Provider>
      </main>
    )
  },
}))
vi.mock("@/components/ui/topbar", () => ({ TopBar: () => null }))
vi.mock("@/lib/ambient", () => ({ useAmbient: () => undefined }))
vi.mock("@/lib/queries", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/queries")>()),
  useProject: () => ({ data: { id: "p" } }),
  useCast: () => ({ data: { characters: [walter] } }),
  useMediaList: () => ({ data: media }),
  useCastTracks: () => ({ data: [], isPending: false }),
  useCharacterShots: () => ({ data: groups, isPending: groups === undefined, isError: false }),
  firstFailure: () => null,
}))

const page = () => (
  <TooltipProvider>
    <AppMenu>
      <div>
        <CharacterPage />
      </div>
    </AppMenu>
  </TooltipProvider>
)

const stack = (name: RegExp) => screen.getByRole("button", { name })
const band = () => document.getElementById("shot-band")
const bandShots = () => within(band()!).getAllByRole("button", { name: /^Shot at/ })

const landsOn = (card: () => HTMLElement) => vi.waitFor(() => expect(document.activeElement).toBe(card()))

beforeEach(() => {
  entry += 1
  groups = [{ shots: [shot(0), shot(60), shot(120)] }, { shots: [shot(5000)] }, { shots: [shot(9000), shot(9060)] }]
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1000)
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(800)
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(1000)
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(800)
  Element.prototype.scrollBy ??= () => {}
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  hoisted.navigate.mockClear()
  document.body.style.pointerEvents = ""
})

describe("a character's shots", () => {
  it("shows a card per group, and still counts every shot", () => {
    render(page())
    expect(screen.getByText(/6 shots · 12 s in all · hover to scrub/)).toBeTruthy()
    expect(screen.getAllByRole("button", { name: /^(\d shots|Shot) at/ })).toHaveLength(3)
  })

  it("opens a stack, and Esc closes it with the focus back on the stack", () => {
    render(page())
    const card = stack(/^3 shots/)
    fireEvent.click(card)
    expect(card.getAttribute("aria-expanded")).toBe("true")
    expect(bandShots()).toHaveLength(3)
    bandShots()[1].focus()
    expect(card.getAttribute("aria-expanded")).toBe("true")
    fireEvent.keyDown(document.activeElement!, { key: "Escape" })
    expect(band()).toBeNull()
    expect(card.getAttribute("aria-expanded")).toBe("false")
    expect(document.activeElement).toBe(card)
  })

  it("leaves Esc alone with nothing open", () => {
    render(page())
    expect(fireEvent.keyDown(window, { key: "Escape" })).toBe(true)
    fireEvent.click(stack(/^3 shots/))
    expect(fireEvent.keyDown(window, { key: "Escape" })).toBe(false)
  })

  it("closes with one Esc though a tooltip is showing in the band", async () => {
    render(page())
    fireEvent.click(stack(/^3 shots/))
    act(() => within(band()!).getByRole("button", { name: "Close" }).focus())
    await screen.findByRole("tooltip")
    fireEvent.keyDown(document.activeElement!, { key: "Escape" })
    expect(band()).toBeNull()
  })

  it("points the band's notch at the middle of its stack", () => {
    render(page())
    fireEvent.click(stack(/^2 shots/))
    // Third of three columns on a page 1000 wide, 16 between them.
    const width = (1000 - 2 * 16) / 3
    const notch = band()!.querySelector<HTMLElement>("[aria-hidden]")!
    expect(parseFloat(notch.style.left)).toBeCloseTo(2 * (width + 16) + width / 2)
  })

  it("keeps Esc for the menu while one is open in the band", async () => {
    render(page())
    fireEvent.click(stack(/^3 shots/))
    fireEvent.contextMenu(bandShots()[0], { clientX: 40, clientY: 40 })
    const menu = await screen.findByRole("menu")
    fireEvent.keyDown(menu, { key: "Escape" })
    expect(band()).not.toBeNull()
    await vi.waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    expect(band()).not.toBeNull()
    fireEvent.keyDown(window, { key: "Escape" })
    expect(band()).toBeNull()
  })

  it("has one stack open at a time", () => {
    render(page())
    fireEvent.click(stack(/^3 shots/))
    fireEvent.click(stack(/^2 shots/))
    expect(bandShots()).toHaveLength(2)
    expect(stack(/^3 shots/).getAttribute("aria-expanded")).toBe("false")
  })

  it("watches from the stack's first frame with the band's button, and a shot from its own", () => {
    render(page())
    fireEvent.click(stack(/^3 shots/))
    fireEvent.click(within(band()!).getByRole("button", { name: "Watch from 00:00" }))
    expect(hoisted.navigate).toHaveBeenLastCalledWith(expect.objectContaining({ search: { f: 0 } }))
    fireEvent.click(bandShots()[1])
    expect(hoisted.navigate).toHaveBeenLastCalledWith(expect.objectContaining({ search: { f: 60 } }))
  })

  it("comes back from a shot of the band with the stack open and the focus on that shot", async () => {
    const view = render(page())
    fireEvent.click(stack(/^3 shots/))
    fireEvent.click(bandShots()[1])
    view.unmount()
    render(page())
    expect(bandShots()).toHaveLength(3)
    await landsOn(() => bandShots()[1])
  })

  it("comes back to the band's first shot, not to the stack that goes by the same key", async () => {
    const view = render(page())
    fireEvent.click(stack(/^3 shots/))
    fireEvent.click(bandShots()[0])
    view.unmount()
    render(page())
    await landsOn(() => bandShots()[0])
  })

  it("comes back from watching a stack with the focus on that stack, closed if it was", async () => {
    const view = render(page())
    fireEvent.keyDown(stack(/^2 shots/), { key: "Enter" })
    expect(hoisted.navigate).toHaveBeenLastCalledWith(expect.objectContaining({ search: { f: 9000 } }))
    view.unmount()
    render(page())
    expect(band()).toBeNull()
    await landsOn(() => stack(/^2 shots/))
  })

  it("comes back open from anywhere, without moving the focus again on the next return", async () => {
    const first = render(page())
    fireEvent.click(stack(/^3 shots/))
    fireEvent.click(bandShots()[1])
    first.unmount()
    const second = render(page())
    await landsOn(() => bandShots()[1])
    second.unmount()
    render(page())
    expect(bandShots()).toHaveLength(3)
    await act(async () => {})
    expect(document.activeElement).toBe(document.body)
  })

  it("starts closed on a new visit", () => {
    const view = render(page())
    fireEvent.click(stack(/^3 shots/))
    view.unmount()
    entry += 1
    render(page())
    expect(band()).toBeNull()
  })

  it("stays open while the shots load again", () => {
    const loaded = groups
    const first = render(page())
    fireEvent.click(stack(/^3 shots/))
    first.unmount()
    groups = undefined
    const view = render(page())
    groups = loaded
    view.rerender(page())
    expect(bandShots()).toHaveLength(3)
  })

  it("moves Tab from an open stack straight into its band", () => {
    render(page())
    const card = stack(/^3 shots/)
    fireEvent.click(card)
    card.focus()
    fireEvent.keyDown(card, { key: "Tab" })
    expect(document.activeElement).toBe(within(band()!).getByRole("button", { name: "Watch from 00:00" }))
  })

  it("stays open on the rest of the group when an edit takes its first shot", () => {
    const view = render(page())
    fireEvent.click(stack(/^3 shots/))
    const open = band()
    groups = [{ shots: [shot(60), shot(120)] }, ...groups!.slice(1)]
    view.rerender(page())
    expect(band()).toBe(open)
    expect(bandShots()).toHaveLength(2)
  })

  it("closes when an edit leaves one shot, on that shot's card", () => {
    const view = render(page())
    fireEvent.click(stack(/^3 shots/))
    groups = [{ shots: [shot(120)] }, ...groups!.slice(1)]
    act(() => view.rerender(page()))
    expect(band()).toBeNull()
    expect(document.activeElement).toBe(stack(/^Shot at E01 · 00:00:05:00/))
  })

  it("does that every time an edit leaves that one shot", () => {
    const rest = groups!.slice(1)
    const view = render(page())
    groups = [{ shots: [shot(120), shot(180)] }, ...rest]
    act(() => view.rerender(page()))
    for (let i = 0; i < 2; i++) {
      groups = [{ shots: [shot(120), shot(180)] }, ...rest]
      act(() => view.rerender(page()))
      fireEvent.click(stack(/^2 shots at E01 · 00:05/))
      groups = [{ shots: [shot(120)] }, ...rest]
      act(() => view.rerender(page()))
      expect(band()).toBeNull()
      expect(document.activeElement).toBe(stack(/^Shot at E01 · 00:00:05:00/))
      act(() => (document.activeElement as HTMLElement).blur())
    }
  })

  it("leaves the focus where it is when the band closes by itself elsewhere", () => {
    const view = render(page())
    fireEvent.click(stack(/^3 shots/))
    const elsewhere = stack(/^Choose a picture/)
    elsewhere.focus()
    groups = [{ shots: [shot(120)] }, ...groups!.slice(1)]
    act(() => view.rerender(page()))
    expect(document.activeElement).toBe(elsewhere)
  })
})
