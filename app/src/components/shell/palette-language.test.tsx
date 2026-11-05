import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { useUi } from "@/state/ui"
import { inLanguage } from "@/test/i18n"
import { CommandPalette } from "./CommandPalette"
import { characterEntries, episodeKeywords } from "./palette-items"

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({ select }: { select: (s: { location: { pathname: string } }) => unknown }) => select({ location: { pathname: "/" } }),
  useNavigate: () => vi.fn(),
}))
vi.mock("@/lib/queries", () => ({
  useProjects: () => ({ data: [] }),
  usePlugins: () => ({ data: [] }),
  useCast: () => ({ data: undefined }),
  useMediaList: () => ({ data: undefined }),
}))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goSettings: vi.fn() }) }))
vi.mock("@/lib/shortcuts", () => ({ useShortcutKeys: () => () => [] }))

// Fixture Romanian: none of its words contains an English one.
const romanian = {
  shell: {
    palette: {
      title: "Paleta de comenzi",
      search: "Caută proiecte, locuri și comenzi",
      empty: "Nimic nu se potrivește.",
      group: { actions: "Acțiuni", goTo: "Mergi la" },
      go: { library: "Biblioteca", settings: "Setări", about: "Despre AutoScene" },
      action: { rail: "Arată sau ascunde bara", updates: "Caută actualizări", logs: "Deschide dosarul jurnalelor" },
    },
  },
}

beforeEach(() => useUi.setState({ paletteOpen: true }))
afterEach(() => {
  cleanup()
  useUi.setState({ paletteOpen: false })
})

const search = (text: string) => fireEvent.change(screen.getByRole("combobox"), { target: { value: text } })

describe("the command palette while AutoScene speaks another language", () => {
  it("lists commands in that language", async () => {
    await inLanguage("ro", romanian)
    render(<CommandPalette />)
    expect(screen.getByRole("option", { name: /Arată sau ascunde bara/ })).toBeTruthy()
    expect(screen.queryByRole("option", { name: /Show or hide the rail/ })).toBeNull()
    expect(screen.getByPlaceholderText("Caută proiecte, locuri și comenzi")).toBeTruthy()
  })

  it("finds a command by its English name as well", async () => {
    await inLanguage("ro", romanian)
    render(<CommandPalette />)
    // "hide the rail" is in no Romanian word of the entry and not in its id ("sidebar"): the English name finds it.
    search("hide the rail")
    expect(screen.getByRole("option", { name: /Arată sau ascunde bara/ })).toBeTruthy()
    expect(screen.queryByRole("option", { name: /Caută actualizări/ })).toBeNull()
    search("check for updates")
    expect(screen.getByRole("option", { name: /Caută actualizări/ })).toBeTruthy()
    // A word no name has finds nothing.
    search("zebra crossing")
    expect(screen.getByText("Nimic nu se potrivește.")).toBeTruthy()
  })

  it("finds a command typed without its diacritics", async () => {
    await inLanguage("ro", romanian)
    render(<CommandPalette />)
    // Many type Romanian with plain letters: "setari" is "Setări". The palette's matching is loose (it keeps
    // faint matches too), so the test asks whether the command is among those shown.
    const shown = () => screen.getAllByRole("option").map((o) => o.textContent)
    search("setari")
    expect(shown()).toContain("Setări")
    search("actualizari")
    expect(shown()).toContain("Caută actualizări")
    // And with them, as before.
    search("Caută actualizări")
    expect(shown()).toContain("Caută actualizări")
  })

  it("finds it by its own language's name, and speaks English again after a switch back", async () => {
    await inLanguage("ro", romanian)
    render(<CommandPalette />)
    search("actualizări")
    expect(screen.getByRole("option", { name: /Caută actualizări/ })).toBeTruthy()
    await act(() => inLanguage("en"))
    search("")
    expect(screen.getByRole("option", { name: /Check for updates/ })).toBeTruthy()
  })
})

describe("what the project's own entries are found by while AutoScene speaks another language", () => {
  const words = { shell: { palette: { unnamed: "Personajul {{number}}", unnamedWord: "fără nume", episodeWord: "Episodul {{number}}" } } }
  const character = { id: "c7", name: null, number: 7, hidden: false, screenSec: 60, shots: 3, coverSmall: null }
  const cast = { characters: [character] } as unknown as Parameters<typeof characterEntries>[0]
  const episode = { season: 1, episode: 3, fileName: "show.s01e03.mkv" }

  it("finds an unnamed character by its English name and the English 'unnamed' too", async () => {
    await inLanguage("ro", words)
    const [entry] = characterEntries(cast)
    expect(entry.label).toBe("Personajul 7")
    expect(entry.keywords).toEqual(["fără nume", "unnamed", "Character 7"])
  })

  it("finds an episode by its English name too", async () => {
    await inLanguage("ro", words)
    expect(episodeKeywords(episode)).toEqual(expect.arrayContaining(["Episodul 3", "Episode 3", "S01E03", "E03"]))
  })

  it("adds nothing twice in English", () => {
    expect(characterEntries(cast)[0].keywords).toEqual(["unnamed"])
    const eps = episodeKeywords(episode)
    expect(eps.filter((w) => w === "Episode 3")).toHaveLength(1)
  })

  it("finds a group's items by the group's English name as well", async () => {
    await inLanguage("ro", romanian)
    render(<CommandPalette />)
    search("go to")
    expect(screen.getByRole("option", { name: /Setări/ })).toBeTruthy()
    expect(screen.queryByRole("option", { name: /Arată sau ascunde bara/ })).toBeNull()
  })
})
