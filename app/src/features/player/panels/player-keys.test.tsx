import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { DEFAULT_KEYMAP, PLAYER_SHORTCUTS, capGroups, setKeymap } from "@/lib/shortcuts"
import { PlayerKeysDialog } from "./PlayerKeysDialog"

// The router as in `features/cast/CharacterCard.test.tsx`, and the app's
// actions as a stand-in that records where "Change keys…" goes.
vi.mock("@tanstack/react-router", () => ({ useNavigate: () => () => {} }))
const { goSettings } = vi.hoisted(() => ({ goSettings: vi.fn() }))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goSettings }) }))

afterEach(() => {
  cleanup()
  setKeymap(DEFAULT_KEYMAP)
  goSettings.mockReset()
})

/** The list row that names a shortcut. */
const row = (label: string) => {
  const li = within(screen.getByRole("dialog")).getByText(label).closest("li")
  if (!li) throw new Error(`no row for ${label}`)
  return li
}
/** A row's key caps, in order. */
const capsIn = (li: HTMLElement) => Array.from(li.querySelectorAll("kbd"), (k) => k.textContent)

describe("PlayerKeysDialog", () => {
  it("is titled Player keys and lists every player shortcut with the keymap's caps", () => {
    render(<PlayerKeysDialog open onOpenChange={() => {}} />)
    expect(screen.getByRole("dialog", { name: "Player keys" })).toBeTruthy()
    expect(within(screen.getByRole("dialog")).getAllByRole("listitem")).toHaveLength(PLAYER_SHORTCUTS.length)
    for (const s of PLAYER_SHORTCUTS) {
      const caps = capGroups(s).flat()
      expect(caps.length).toBeGreaterThan(0)
      expect(capsIn(row(s.label))).toEqual(caps)
    }
  })

  it("takes the focus itself on opening, so no button is ringed", () => {
    render(<PlayerKeysDialog open onOpenChange={() => {}} />)
    expect(document.activeElement).toBe(screen.getByRole("dialog"))
  })

  it("shows a changed key, and says No key for one the person removed", () => {
    setKeymap({ preset: "autoscene", own: { "player.mute.0": [{ key: "u" }], "player.goTo.0": [] } })
    render(<PlayerKeysDialog open onOpenChange={() => {}} />)
    expect(capsIn(row("Mute"))).toEqual(["U"])
    const goTo = row("Go to a timecode")
    expect(capsIn(goTo)).toEqual([])
    expect(within(goTo).getByText("No key")).toBeTruthy()
    // The others keep their keys.
    expect(within(row("Play / pause")).queryByText("No key")).toBeNull()
  })

  it("follows a key changed while it is open", () => {
    render(<PlayerKeysDialog open onOpenChange={() => {}} />)
    expect(capsIn(row("Mute"))).toEqual(["M"])
    act(() => setKeymap({ preset: "autoscene", own: { "player.mute.0": [{ key: "u" }] } }))
    expect(capsIn(row("Mute"))).toEqual(["U"])
  })

  it("Change keys… closes the dialog and opens Settings → Shortcuts", () => {
    const onOpenChange = vi.fn()
    render(<PlayerKeysDialog open onOpenChange={onOpenChange} />)
    fireEvent.click(screen.getByRole("button", { name: "Change keys…" }))
    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(goSettings).toHaveBeenCalledExactlyOnceWith("shortcuts")
  })

  it("renders nothing while closed", () => {
    render(<PlayerKeysDialog open={false} onOpenChange={() => {}} />)
    expect(screen.queryByRole("dialog")).toBeNull()
  })
})
