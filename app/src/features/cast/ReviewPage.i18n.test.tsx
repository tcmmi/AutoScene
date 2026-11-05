import type * as React from "react"
import { act, cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { Character, FaceTrackItem } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { inLanguage } from "@/test/i18n"
import { ReviewPage } from "./ReviewPage"

const unnamed = { id: "c2", label: "Character 2", number: 2, name: null, hidden: false } as Character
const face = { id: 7, mediaId: "m", start: 24, end: 48, thumb: "t.jpg", characterId: "c2", assign: "unsure", score: 0.5 } as FaceTrackItem
const cast = { characters: [unnamed] }

vi.mock("@tanstack/react-router", () => ({
  useParams: () => ({ pid: "p" }),
  useSearch: () => ({}),
  useNavigate: () => vi.fn(),
  Link: ({ children }: { children: React.ReactNode }) => <a>{children}</a>,
}))
vi.mock("@/components/ui/page", () => ({ Page: ({ children }: { children: React.ReactNode }) => <main>{children}</main> }))
vi.mock("@/lib/queries", () => ({
  useCast: () => ({ data: cast }),
  useMediaList: () => ({ data: [] }),
  useCastTracks: () => ({ data: [face], isSuccess: true }),
  usePortrait: () => ({ data: undefined }),
  portraitQuery: () => ({}),
  queryClient: { prefetchQuery: () => Promise.resolve() },
  firstFailure: () => null,
}))

afterEach(cleanup)

describe("the review's question", () => {
  const heading = () => screen.getByRole("heading", { level: 2 })

  it("sets an unnamed suggestion in italic, inside one sentence", () => {
    render(<ReviewPage />, { wrapper: TooltipProvider })
    expect(heading().textContent).toBe("Is this Character 2?")
    expect(heading().querySelector("span")?.className).toContain("italic")
    expect(screen.getByText(/Looks a lot like them — the analysis wasn't sure enough to decide alone\./)).toBeTruthy()
  })

  it("reads in the language AutoScene speaks, the name still in its span", async () => {
    await act(() =>
      inLanguage("ro", { cast: { unnamed: "Personajul {{n}}", review: { isThis: "Este <x>{{name}}</x>?", yes: "Da, {{name}}" } } }),
    )
    render(<ReviewPage />, { wrapper: TooltipProvider })
    expect(heading().textContent).toBe("Este Personajul 2?")
    expect(heading().querySelector("span")?.textContent).toBe("Personajul 2")
    expect(screen.getByRole("button", { name: /Da, Personajul 2/ })).toBeTruthy()
  })

  it("keeps a name that looks like markup as the text it is", async () => {
    cast.characters = [{ ...unnamed, name: "<b>Heisenberg</b>" }]
    await act(() => inLanguage("ro", { cast: { review: { isThis: "Este <x>{{name}}</x>?" } } }))
    render(<ReviewPage />, { wrapper: TooltipProvider })
    expect(heading().textContent).toBe("Este <b>Heisenberg</b>?")
    expect(heading().querySelector("b")).toBeNull()
    cast.characters = [unnamed]
  })
})
