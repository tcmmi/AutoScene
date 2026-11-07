import type * as React from "react"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { Cast, Character } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { inLanguage } from "@/test/i18n"
import { CastPage } from "./CastPage"

const hoisted = vi.hoisted(() => ({ toast: vi.fn(), setHiddenAll: vi.fn(), n: 0 }))
const state: { cast: Cast; project: { id: string; title: string; kind: string; content: string } } = {
  cast: undefined as unknown as Cast,
  project: { id: "p", title: "Show", kind: "series", content: "live" },
}

vi.mock("./RescanWaiting", () => ({ RescanWaiting: () => null }))
vi.mock("./RescanBar", () => ({ RescanBar: () => null }))
vi.mock("@tanstack/react-router", () => ({
  useParams: () => ({ pid: `p${hoisted.n}` }),
  useNavigate: () => () => {},
  Link: ({ to: _to, params: _params, ...props }: React.ComponentProps<"a"> & Record<string, unknown>) => <a href="#x" {...props} />,
}))
vi.mock("sonner", () => ({ toast: Object.assign(hoisted.toast, { error: vi.fn(), success: vi.fn() }) }))
vi.mock("@/components/ui/page", () => ({
  Page: ({ children }: { children: React.ReactNode }) => <main>{children}</main>,
}))
vi.mock("@/components/ui/topbar", () => ({ TopBar: () => null }))
vi.mock("@/features/project/AnalysisStatus", () => ({ AnalysisStatus: () => null }))
vi.mock("@/features/project/ProjectNav", () => ({ ProjectNav: () => null }))
vi.mock("@/lib/shortcuts", () => ({ CAST_SHORTCUTS: [], useShortcuts: () => undefined }))
vi.mock("./AniListDialog", () => ({ AniListDialog: () => null }))
vi.mock("./AniListNames", () => ({ AniListNames: () => null }))
vi.mock("./ChoosePictureDialog", () => ({ ChoosePictureDialog: () => null }))
vi.mock("./RenameCharacterDialog", () => ({ RenameCharacterDialog: () => null }))
vi.mock("./EveryoneElseCard", () => ({ EveryoneElseCard: () => null }))
vi.mock("./cast", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./cast")>()),
  setHiddenAll: hoisted.setHiddenAll,
  useProjectAnalysis: () => [],
}))
vi.mock("@/lib/queries", () => ({
  useProject: () => ({ data: state.project }),
  useCast: () => ({ data: state.cast, isPending: false }),
  useMediaList: () => ({ data: [] }),
  useSettings: () => ({ data: { anilist: false } }),
  firstFailure: () => null,
}))

afterEach(cleanup)

const person = (id: string, name: string | null, number: number): Character =>
  ({
    id,
    name,
    label: name ?? `Character ${number}`,
    number,
    hidden: false,
    tracks: 3,
    shots: 3,
    media: 1,
    screenSec: 90,
    cover: null,
    coverSmall: null,
    unsure: 0,
    waiting: 0,
  }) as Character
const castOf = (characters: Character[], extra: Partial<Cast> = {}) =>
  ({ characters, unsure: 0, unassigned: 0, unplaced: 0, mediaDone: 2, mediaTotal: 5, ...extra }) as unknown as Cast
const render_ = () => render(<CastPage />, { wrapper: TooltipProvider })

beforeEach(() => {
  // The page remembers how each project was searched: every test is its own project.
  hoisted.n++
  state.project = { id: "p", title: "Show", kind: "series", content: "live" }
  state.cast = castOf([person("w", "Walter", 1), person("a", null, 2), person("b", null, 3), person("c", "Jesse", 4), person("d", "Skyler", 5), person("e", "Hank", 6)])
  hoisted.setHiddenAll.mockResolvedValue(1)
})

describe("the count line", () => {
  it("counts the characters and the episodes, a series' faces coming from episodes", () => {
    render_()
    expect(screen.getByText("6 characters · faces from 2 of 5 episodes")).toBeTruthy()
  })

  it("says files for a film", () => {
    state.project.kind = "movie"
    state.cast = castOf([person("w", "Walter", 1)], { mediaDone: 1, mediaTotal: 1 })
    render_()
    expect(screen.getByText("1 character · faces from 1 of 1 file")).toBeTruthy()
  })

  it("says how many of the cast a search leaves, with the language's plural forms", async () => {
    await act(() =>
      inLanguage("ro", {
        cast: {
          unnamed: "Personajul {{n}}",
          count: { characters_one: "{{count}} personaj", characters_few: "{{count}} personaje", characters_other: "{{count}} de personaje" },
          page: { narrowed_one: "{{shown}} din {{count}} personaj", narrowed_few: "{{shown}} din {{count}} personaje", narrowed_other: "{{shown}} din {{count}} de personaje" },
        },
      }),
    )
    render_()
    expect(screen.getByText(/^6 personaje · /)).toBeTruthy()
    fireEvent.change(screen.getByRole("textbox", { name: "Find a character" }), { target: { value: "personajul 3" } })
    expect(screen.getByText(/^1 din 6 personaje · /)).toBeTruthy()
  })

  it("says it in English once narrowed", () => {
    render_()
    fireEvent.change(screen.getByRole("textbox", { name: "Find a character" }), { target: { value: "walt" } })
    expect(screen.getByText(/^1 of 6 characters/)).toBeTruthy()
  })
})

describe("a selection", () => {
  const select = () => {
    const card = (name: string) => screen.getByText(name).closest("a") as HTMLElement
    fireEvent.click(card("Walter"), { ctrlKey: true })
    fireEvent.click(card("Character 2"), { ctrlKey: true })
  }

  it("shows its count on the bar with the plural form", () => {
    render_()
    select()
    expect(screen.getByText("2 characters")).toBeTruthy()
  })

  it("tells how many it hid, and takes them back", async () => {
    hoisted.setHiddenAll.mockResolvedValue(2)
    render_()
    select()
    fireEvent.click(screen.getByRole("button", { name: /Hide/ }))
    await waitFor(() => expect(hoisted.toast).toHaveBeenCalled())
    expect(hoisted.toast.mock.calls[0][0]).toBe("Hid 2 characters")
  })
})
