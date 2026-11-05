import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { Character } from "@/bindings"
import { AniListDialog } from "./AniListDialog"

const project: { content: string; kind: string } = { content: "animation", kind: "series" }
vi.mock("@/lib/queries", () => ({
  useProject: () => ({ data: { id: "p", ...project } }),
  qk: { cast: () => ["cast"] },
  queryClient: { invalidateQueries: () => Promise.resolve() },
}))
vi.mock("@/lib/ipc", () => ({
  commands: { anilistSearchText: () => Promise.resolve("") },
  errorMessage: (e: unknown) => String(e),
}))

afterEach(cleanup)

const open = () => render(<AniListDialog pid="p" characters={[] as Character[]} open onOpenChange={() => {}} />)

describe("the names dialog's description", () => {
  it("says what the service sees and ends with the credit its licence asks for", () => {
    open()
    expect(
      screen.getByText(
        "TVmaze sees only what you send: the search text, then the cast and images of the series you confirm. Names and pictures: TVmaze.com (CC BY-SA).",
      ),
    ).toBeTruthy()
  })

  it("has no stray space when the source asks for no credit", () => {
    project.content = "anime"
    open()
    expect(
      screen.getByText("AniList sees only what you send: the search text, then the cast and images of the series you confirm."),
    ).toBeTruthy()
    project.content = "animation"
  })

  it("speaks of a film, with Fandom's credit, for an animated film", () => {
    project.kind = "movie"
    open()
    expect(screen.getByText(/^Wikidata and Fandom see only what you send: .* of the film you confirm\. Names and pictures: the film's Fandom wiki/)).toBeTruthy()
    project.kind = "series"
  })
})
