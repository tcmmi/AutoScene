import type * as React from "react"
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { Character } from "@/bindings"
import { t } from "@/lib/i18n"
import { inLanguage } from "@/test/i18n"
import { CharacterCard } from "./CharacterCard"
import { PickCharacter } from "./PickCharacter"
import { ALL_CAST, characterLabel, characterNames, findCharacters, unplacedNote } from "./cast"

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to: _to, params: _params, ...props }: React.ComponentProps<"a"> & Record<string, unknown>) => <a href="#x" {...props} />,
  useNavigate: () => () => {},
}))
const cast: { characters: Character[] } = { characters: [] }
vi.mock("@/lib/queries", () => ({ useCast: () => ({ data: cast }) }))

afterEach(cleanup)

const person = (id: string, name: string | null, number: number, extra: Partial<Character> = {}): Character => ({
  id,
  name,
  label: name ?? `Character ${number}`,
  number,
  hidden: false,
  tracks: 3,
  shots: 3,
  media: 1,
  screenSec: 30,
  cover: null,
  coverSmall: null,
  unsure: 0,
  waiting: 0,
  ...extra,
})

// What a Romanian catalog would hold for the words these tests touch: the plural forms have a `few`.
const ro = {
  cast: {
    unnamed: "Personajul {{n}}",
    count: { shots_one: "{{count}} cadru", shots_few: "{{count}} cadre", shots_other: "{{count}} de cadre" },
    unplaced_one: "{{count}} decizie așteaptă o față.",
    unplaced_few: "{{count}} decizii așteaptă fețe.",
    unplaced_other: "{{count}} de decizii așteaptă fețe.",
    pick: { newCharacterNamed: "Personaj nou „{{name}}”", nobody: "Nimeni nu se potrivește." },
  },
}

describe("counts in the cast", () => {
  it("say what they always said in English, with a plural key each", () => {
    expect([1, 0, 2, 1414].map((count) => t("cast:count.shots", { count }))).toEqual(["1 shot", "0 shots", "2 shots", "1,414 shots"])
    expect([1, 3].map((count) => t("cast:count.faces", { count }))).toEqual(["1 face", "3 faces"])
    expect([1, 2].map((count) => t("cast:count.characters", { count }))).toEqual(["1 character", "2 characters"])
    expect([1, 2].map((count) => t("cast:count.episodes", { count }))).toEqual(["1 episode", "2 episodes"])
  })

  it("follow the language's plural categories: one, few and other in Romanian", async () => {
    await act(() => inLanguage("ro", ro))
    expect([1, 3, 19, 20, 101].map((count) => t("cast:count.shots", { count }))).toEqual([
      "1 cadru",
      "3 cadre",
      "19 cadre",
      "20 de cadre",
      "101 cadre",
    ])
    expect(unplacedNote(3)).toBe("3 decizii așteaptă fețe.")
    expect(unplacedNote(0)).toBeNull()
  })

  it("write a whole sentence per case, not a sentence built of parts", () => {
    expect(t("cast:review.skipped", { count: 1 })).toBe("1 face skipped; they stay in the list for later.")
    expect(t("cast:maybe.lookLike", { count: 1, name: "Walter" })).toBe("1 face looks like Walter, but not clearly enough to decide alone.")
    expect(t("cast:maybe.lookLike", { count: 14, name: "Walter" })).toBe("14 faces look like Walter, but not clearly enough to decide alone.")
    expect(t("cast:page.narrowed", { shown: "3", count: 12 })).toBe("3 of 12 characters")
    expect(t("cast:page.facesFromFiles", { done: 1, count: 1 })).toBe("faces from 1 of 1 file")
    expect(t("cast:names.named", { count: 1, source: "AniList" })).toBe("Named 1 character from AniList")
  })
})

describe("Pick character's search", () => {
  it("ignores accents and case like the cast's: 'stefan' finds Ștefan, 's' with a cedilla finds s with a comma", () => {
    cast.characters = [person("s", "Ștefan", 1), person("w", "Walter", 2)]
    render(<PickCharacter pid="p" open onOpenChange={() => {}} title="Move" onPick={() => {}} />)
    const search = screen.getByRole("combobox")
    for (const query of ["stefan", "ŞTEFAN", "ștefan"]) {
      fireEvent.change(search, { target: { value: query } })
      expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([expect.stringContaining("Ștefan")])
    }
  })
})

describe("an unnamed character", () => {
  const twelve = person("c12", null, 12)

  it("is named in the UI from its number, as it always read in English", () => {
    expect(characterLabel(twelve)).toBe("Character 12")
    expect(characterLabel(person("w", "Walter", 3))).toBe("Walter")
    render(<CharacterCard pid="p" character={twelve} />)
    expect(screen.getByText("Character 12")).toBeTruthy()
  })

  it("is named in the language AutoScene speaks", async () => {
    await act(() => inLanguage("ro", ro))
    render(<CharacterCard pid="p" character={twelve} />)
    expect(screen.getByText("Personajul 12")).toBeTruthy()
    // A card says its shots with the language's plural form.
    expect(screen.getByText(/3 cadre/)).toBeTruthy()
  })

  it("is found by its label and by the English 'Character 12', whichever language AutoScene speaks", async () => {
    const characters = [person("w", "Walter", 1), person("c2", null, 2), twelve]
    const find = (query: string) => findCharacters(characters, { ...ALL_CAST, query }).map((c) => c.id)
    expect(find("character 12")).toEqual(["c12"])
    await act(() => inLanguage("ro", ro))
    expect(characterNames(twelve)).toEqual(["Personajul 12", "Character 12"])
    expect(characterNames(person("w", "Walter", 1))).toEqual(["Walter"])
    expect(find("personajul 12")).toEqual(["c12"])
    expect(find("character 12")).toEqual(["c12"])
    expect(find("walter")).toEqual(["w"])
  })

  it("is found in Pick character by either, and an English search stays an English one", async () => {
    cast.characters = [person("w", "Walter", 1), twelve]
    await act(() => inLanguage("ro", ro))
    render(<PickCharacter pid="p" open onOpenChange={() => {}} title="Move" onPick={() => {}} />)
    const search = screen.getByRole("combobox")
    expect(screen.getAllByRole("option")).toHaveLength(2)
    fireEvent.change(search, { target: { value: "Character 12" } })
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([expect.stringContaining("Personajul 12")])
    fireEvent.change(search, { target: { value: "personajul 1" } })
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([expect.stringContaining("Personajul 12")])
    fireEvent.change(search, { target: { value: "nimeni" } })
    expect(screen.getByText("Nimeni nu se potrivește.")).toBeTruthy()
  })
})

describe("the order by name", () => {
  const names = ["Ș", "Sa", "T"].map((n, i) => person(n, n, i + 1))
  const byName = () => findCharacters(names, { ...ALL_CAST, order: "name" }).map((c) => c.id)

  it("uses the language's own order: Ș after S in Romanian", async () => {
    // English reads the diacritic as the same letter.
    expect(byName()).toEqual(["Ș", "Sa", "T"])
    await act(() => inLanguage("ro", ro))
    expect(byName()).toEqual(["Sa", "Ș", "T"])
  })

  it("puts the named first, the unnamed after them in their numbers' order, by their label", async () => {
    const mixed = [person("u10", null, 10), person("u2", null, 2), person("w", "Walter", 7), person("u3", null, 3)]
    const order = () => findCharacters(mixed, { ...ALL_CAST, order: "name" }).map((c) => c.id)
    expect(order()).toEqual(["w", "u2", "u3", "u10"])
    // The backend's label is not what is sorted: with labels that sort the other way round, the order is the same.
    const reversed = mixed.map((c) => ({ ...c, label: `Z${100 - c.number}` }))
    expect(findCharacters(reversed, { ...ALL_CAST, order: "name" }).map((c) => c.id)).toEqual(["w", "u2", "u3", "u10"])
  })
})

describe("the plural keys of what was plain numbers", () => {
  it("say what they said in English", () => {
    expect([1, 3].map((count) => t("cast:card.toReview", { count }))).toEqual(["1 to review", "3 to review"])
    expect(t("cast:card.moreToReview", { count: 2 })).toBe("+2 to review")
    expect([1, 3].map((count) => t("cast:review.skippedCount", { count }))).toEqual(["1 skipped", "3 skipped"])
  })

  it("let a language inflect the participle and move the plus", async () => {
    await act(() =>
      inLanguage("ro", {
        cast: {
          review: { skippedCount_one: "{{count}} omisă", skippedCount_few: "{{count}} omise", skippedCount_other: "{{count}} de omise" },
          card: { moreToReview_one: "{{count}} de revizuit +", moreToReview_few: "{{count}} de revizuit +", moreToReview_other: "{{count}} de revizuit +" },
        },
      }),
    )
    expect([1, 3, 20].map((count) => t("cast:review.skippedCount", { count }))).toEqual(["1 omisă", "3 omise", "20 de omise"])
    render(<CharacterCard pid="p" character={person("w", "Walter", 1, { unsure: 2 })} />)
    expect(screen.getByText("2 de revizuit +")).toBeTruthy()
  })
})
