import { useState } from "react"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it } from "vitest"
import { SearchField } from "./search-field"

afterEach(cleanup)

function Finding() {
  const [query, setQuery] = useState("")
  return <SearchField value={query} onChange={setQuery} label="Find a plugin" />
}

const field = () => screen.getByRole("textbox", { name: "Find a plugin" }) as HTMLInputElement

describe("SearchField", () => {
  it("is a search landmark, its label is its placeholder too, and × only shows with text", async () => {
    const user = userEvent.setup()
    render(<Finding />)
    expect(screen.getByRole("search").contains(field())).toBe(true)
    expect(field().placeholder).toBe("Find a plugin")
    expect(screen.queryByRole("button", { name: "Clear the search" })).toBeNull()
    await user.type(field(), "ani")
    expect(field().value).toBe("ani")
    expect(screen.getByRole("button", { name: "Clear the search" })).toBeTruthy()
  })

  it("× clears it", async () => {
    const user = userEvent.setup()
    render(<Finding />)
    await user.type(field(), "ani")
    await user.click(screen.getByRole("button", { name: "Clear the search" }))
    expect(field().value).toBe("")
    expect(screen.queryByRole("button", { name: "Clear the search" })).toBeNull()
  })

  it("the landmark has the field's name, and × gives the focus back to the field", async () => {
    const user = userEvent.setup()
    render(<Finding />)
    expect(screen.getByRole("search", { name: "Find a plugin" })).toBeTruthy()
    await user.type(field(), "ani")
    await user.click(screen.getByRole("button", { name: "Clear the search" }))
    expect(document.activeElement).toBe(field())
  })

  it("Escape clears it", async () => {
    const user = userEvent.setup()
    render(<Finding />)
    await user.type(field(), "ani{Escape}")
    expect(field().value).toBe("")
  })

  it("is 256 px wide unless the screen says otherwise", () => {
    render(<SearchField value="" onChange={() => {}} label="Find a plugin" className="w-full" />)
    expect(screen.getByRole("search").className).toContain("w-full")
    expect(screen.getByRole("search").className).not.toContain("w-64")
  })
})
