import type * as React from "react"
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { refusal } from "@/lib/keymap"
import { DEFAULT_KEYMAP, PRESETS, setKeymap, shortcutOf } from "@/lib/shortcuts"
import { ShortcutsSection } from "./sections/ShortcutsSection"

// The words only a Mac says: the platform is a Mac for this whole file.

vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isMac: true,
  modKey: "⌘",
  shiftKey: "⇧",
  altKey: "⌥",
}))
vi.mock("@/lib/ipc", () => ({
  commands: {
    settingsGet: vi.fn(() => Promise.resolve({ libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng" })),
    settingsUpdate: vi.fn((s: unknown) => Promise.resolve(s)),
    menuSetKeys: vi.fn(() => Promise.resolve(null)),
  },
  errorMessage: (e: unknown) => String(e),
}))

const wrapper = ({ children }: { children: React.ReactNode }) => <TooltipProvider>{children}</TooltipProvider>

afterEach(() => {
  cleanup()
  setKeymap(DEFAULT_KEYMAP)
})

describe("on a Mac", () => {
  it("Premiere Pro's keys are described by what differs from AutoScene's", () => {
    const premiere = PRESETS.find((p) => p.id === "premiere")
    expect(premiere?.description).toBe("Zoom with = − \\, the rest like AutoScene.")
  })

  it("a key the menu bar owns is refused as the menu bar's, not the window's", () => {
    expect(refusal({ key: "q", mod: true }, shortcutOf("player.mute"), 0, DEFAULT_KEYMAP)).toBe("⌘ Q belongs to the menu bar")
  })

  it("recording with Control tells the person to use ⌘", () => {
    render(<ShortcutsSection />, { wrapper })
    fireEvent.click(screen.getByRole("button", { name: "Mute: M. Change" }))
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { key: "u", ctrlKey: true, bubbles: true, cancelable: true })))
    expect(screen.getAllByText("Use ⌘ instead of Control").length).toBeGreaterThan(0)
  })
})
