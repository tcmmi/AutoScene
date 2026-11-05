import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { AniListNames as Found, Character, Cast } from "@/bindings"
import { inLanguage } from "@/test/i18n"
import { AniListNames } from "./AniListNames"

const hoisted = vi.hoisted(() => ({ success: vi.fn() }))
const state: { names: Found } = { names: { looking: false, seriesId: 1, series: "Paw Patrol", names: [], merges: [] } }
vi.mock("@/lib/queries", () => ({
  useSettings: () => ({ data: { anilist: true } }),
  useProject: () => ({ data: { id: "p", content: "animation", kind: "series" } }),
  useAniListNames: () => ({ data: state.names }),
  qk: { cast: () => ["cast"] },
  queryClient: { invalidateQueries: () => Promise.resolve() },
}))
vi.mock("sonner", () => ({ toast: { success: hoisted.success, error: vi.fn() } }))
vi.mock("@/lib/ipc", () => ({ commands: { anilistAccept: () => Promise.resolve() }, errorMessage: (e: unknown) => String(e) }))

afterEach(cleanup)

const person = (id: string, name: string | null, number: number) => ({ id, name, number, label: `Character ${number}` }) as unknown as Character
const cast = { characters: [person("a", null, 2), person("b", null, 3), person("c", "Chase", 4)] } as unknown as Cast
const suggest = (characterId: string, name: string) => ({ characterId, anilistId: 1, name, score: 0.9, image: null })

const ro = {
  cast: {
    unnamed: "Personajul {{n}}",
    names: {
      hasNames_one: "{{source}} are un nume pentru {{count}} personaj",
      hasNames_few: "{{source}} are nume pentru {{count}} personaje",
      hasNames_other: "{{source}} are nume pentru {{count}} de personaje",
      same: "{{from}} este {{into}}",
    },
  },
}

describe("the names banner", () => {
  it("counts the names it found with one plural key, and names the source", () => {
    state.names = { ...state.names, names: [suggest("a", "Skye")] }
    const { unmount } = render(<AniListNames pid="p" cast={cast} />)
    expect(screen.getByText("TVmaze has names for 1 character")).toBeTruthy()
    expect(screen.getByRole("button", { name: /^Give names$/ })).toBeTruthy()
    unmount()
    state.names = { ...state.names, names: [suggest("a", "Skye"), suggest("b", "Marshall")] }
    render(<AniListNames pid="p" cast={cast} />)
    expect(screen.getByText("TVmaze has names for 2 characters")).toBeTruthy()
  })

  it("tells in a toast how many it named, with the plural form", async () => {
    state.names = { ...state.names, names: [suggest("a", "Skye")] }
    render(<AniListNames pid="p" cast={cast} />)
    fireEvent.click(screen.getByRole("button", { name: "Give names" }))
    await waitFor(() => expect(hoisted.success).toHaveBeenCalledWith("Named 1 character from TVmaze"))
    // Taking one of two out: the button counts what is kept.
    cleanup()
    state.names = { ...state.names, names: [suggest("a", "Skye"), suggest("b", "Marshall")] }
    render(<AniListNames pid="p" cast={cast} />)
    fireEvent.click(screen.getByRole("button", { name: /Marshall/ }))
    expect(screen.getByRole("button", { name: "Give 1" })).toBeTruthy()
  })

  it("says a merge as one sentence, the characters by their label in the language", async () => {
    state.names = { ...state.names, names: [suggest("b", "Marshall")], merges: [{ from: "a", into: "b", anilistId: 1 }] }
    await act(() => inLanguage("ro", ro))
    render(<AniListNames pid="p" cast={cast} />)
    expect(screen.getByText("Personajul 2 este Personajul 3")).toBeTruthy()
    expect(screen.getByText("TVmaze are un nume pentru 1 personaj")).toBeTruthy()
  })

  it("says a character gone from the cast in words, not as a gap", () => {
    state.names = { ...state.names, names: [suggest("b", "Marshall")], merges: [{ from: "x", into: "y", anilistId: 1 }] }
    render(<AniListNames pid="p" cast={cast} />)
    expect(screen.getByText("A character is another")).toBeTruthy()
  })
})
