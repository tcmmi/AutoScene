import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { useUi } from "@/state/ui"
import { inLanguage } from "@/test/i18n"
import { CommandPalette } from "./CommandPalette"

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({ select }: { select: (s: { location: { pathname: string } }) => unknown }) => select({ location: { pathname: "/p/p1" } }),
  useNavigate: () => vi.fn(),
}))
vi.mock("@/lib/queries", () => ({
  useProjects: () => ({ data: [{ id: "p1", title: "Serial", kind: "series", mediaCount: 2, poster: null, posterSmall: null }] }),
  usePlugins: () => ({ data: [] }),
  useCast: () => ({
    data: {
      characters: [
        { id: "c1", name: "Ștefan", number: 1, hidden: false, screenSec: 60, shots: 3, coverSmall: null },
        { id: "c2", name: "Mihai", number: 2, hidden: false, screenSec: 30, shots: 2, coverSmall: null },
      ],
    },
  }),
  useMediaList: () => ({ data: undefined }),
}))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goSettings: vi.fn() }) }))
vi.mock("@/lib/shortcuts", () => ({ useShortcutKeys: () => () => [] }))

beforeEach(() => useUi.setState({ paletteOpen: true }))
afterEach(() => {
  cleanup()
  useUi.setState({ paletteOpen: false })
})

const search = (text: string) => fireEvent.change(screen.getByRole("combobox"), { target: { value: text } })
const characters = () => screen.queryAllByRole("option").map((o) => o.textContent ?? "")

describe("the open project's characters, found without their diacritics", () => {
  it("finds Ștefan by stefan, with or without the letter's mark, and not Mihai", async () => {
    await inLanguage("ro")
    render(<CommandPalette />)
    for (const typed of ["stefan", "ștefan", "ŞTEFAN", "tefan"]) {
      await act(async () => search(typed))
      expect(characters().some((o) => o.includes("Ștefan")), typed).toBe(true)
      expect(characters().some((o) => o.includes("Mihai")), typed).toBe(false)
    }
  })
})
