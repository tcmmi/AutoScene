import { useState } from "react"
import { act, cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { type TextState, canDo, captureText, pastable, revealCaret, runTextAction, textFieldAt } from "./text-edit"

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
  Reflect.deleteProperty(document, "execCommand")
})

const base: TextState = {
  start: 0,
  end: 6,
  direction: "forward",
  selected: "Walter",
  readOnly: false,
  secret: false,
  empty: false,
  all: false,
  canUndo: true,
  canRedo: false,
}

describe("what the text menu offers", () => {
  it("follows the selection, the clipboard and the field", () => {
    expect(canDo(base, "cut", null)).toBe(true)
    expect(canDo(base, "copy", null)).toBe(true)
    expect(canDo(base, "delete", null)).toBe(true)
    expect(canDo({ ...base, start: 3, end: 3, selected: "" }, "copy", null)).toBe(false)
    expect(canDo(base, "undo", null)).toBe(true)
    expect(canDo(base, "redo", null)).toBe(false)
    // Paste once the clipboard is known to hold text.
    expect(canDo(base, "paste", undefined)).toBe(false)
    expect(canDo(base, "paste", null)).toBe(false)
    expect(canDo(base, "paste", "")).toBe(false)
    expect(canDo(base, "paste", "Jesse")).toBe(true)
    expect(canDo(base, "selectAll", null)).toBe(true)
    expect(canDo({ ...base, all: true }, "selectAll", null)).toBe(false)
    expect(canDo({ ...base, empty: true }, "selectAll", null)).toBe(false)
  })

  it("only copies out of a read-only field, and nothing out of a password", () => {
    const readOnly = { ...base, readOnly: true }
    expect(["undo", "cut", "paste", "delete"].map((a) => canDo(readOnly, a as never, "x"))).toEqual([false, false, false, false])
    expect(canDo(readOnly, "copy", null)).toBe(true)
    const secret = { ...base, secret: true }
    expect(canDo(secret, "copy", null)).toBe(false)
    expect(canDo(secret, "cut", null)).toBe(false)
    expect(canDo(secret, "paste", "x")).toBe(true)
  })

  it("is for fields that hold text", () => {
    render(
      <>
        <input aria-label="Name" />
        <input aria-label="Search" type="search" />
        <textarea aria-label="Notes" />
      </>,
    )
    expect(textFieldAt(screen.getByLabelText("Name"))).toBe(screen.getByLabelText("Name"))
    expect(textFieldAt(screen.getByLabelText("Search"))).not.toBeNull()
    expect(textFieldAt(screen.getByLabelText("Notes"))).not.toBeNull()
    // A number field has no text selection to edit (the app has none; the browser's would be).
    const count = document.createElement("input")
    count.type = "number"
    expect(textFieldAt(count)).toBeNull()
    expect(textFieldAt(document.body)).toBeNull()
  })

  it("reads the field as it is", () => {
    render(<input aria-label="Name" type="password" defaultValue="Walter White" readOnly />)
    const input = screen.getByLabelText("Name") as HTMLInputElement
    input.focus()
    input.setSelectionRange(7, 12)
    expect(captureText(input)).toMatchObject({ start: 7, end: 12, selected: "White", secret: true, readOnly: true, all: false })
  })
})

describe("what it does", () => {
  it("pastes into a controlled field as typing would: React hears it", async () => {
    const changed = vi.fn()
    render(<Controlled onChange={changed} />)
    const input = screen.getByLabelText("Name") as HTMLInputElement
    input.focus()
    input.setSelectionRange(7, 12)
    const state = captureText(input)
    await act(() => runTextAction(input, state, "paste", { copy: vi.fn(), clipboard: "Jesse\nPinkman" }))
    // One line in a one-line field.
    expect(changed).toHaveBeenLastCalledWith("Walter Jesse Pinkman")
    expect(input.value).toBe("Walter Jesse Pinkman")
    expect([input.selectionStart, input.selectionEnd]).toEqual([20, 20])
  })

  it("types with the engine's own command where there is one (for its undo)", async () => {
    const exec = vi.fn(() => true)
    Object.defineProperty(document, "execCommand", { value: exec, configurable: true })
    render(<Controlled onChange={vi.fn()} />)
    const input = screen.getByLabelText("Name") as HTMLInputElement
    input.focus()
    await act(() => runTextAction(input, { ...captureText(input), start: 0, end: 6 }, "paste", { copy: vi.fn(), clipboard: "Skyler" }))
    expect(exec).toHaveBeenCalledWith("insertText", false, "Skyler")
    // Not typed twice.
    expect(input.value).toBe("Walter White")
  })

  it("cuts only once the copy worked", async () => {
    render(<Controlled onChange={vi.fn()} />)
    const input = screen.getByLabelText("Name") as HTMLInputElement
    input.focus()
    input.setSelectionRange(0, 7)
    const state = captureText(input)
    await act(() => runTextAction(input, state, "cut", { copy: () => Promise.resolve(false), clipboard: null }))
    expect(input.value).toBe("Walter White")
    const copy = vi.fn(() => Promise.resolve(true))
    await act(() => runTextAction(input, state, "cut", { copy, clipboard: null }))
    expect(copy).toHaveBeenCalledWith("Walter ")
    expect(input.value).toBe("White")
  })

  it("selects all of it", async () => {
    render(<Controlled onChange={vi.fn()} />)
    const input = screen.getByLabelText("Name") as HTMLInputElement
    input.focus()
    input.setSelectionRange(2, 2)
    await act(() => runTextAction(input, captureText(input), "selectAll", { copy: vi.fn(), clipboard: null }))
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 12])
  })

  it("keeps a text area's lines", () => {
    const area = document.createElement("textarea")
    const input = document.createElement("input")
    expect(pastable(area, "a\nb")).toBe("a\nb")
    expect(pastable(input, "a\r\nb\rc")).toBe("a b c")
  })
})

describe("revealCaret", () => {
  /** A field 200 px wide inside its padding, holding 60 characters of 8 px: it scrolls. */
  function long() {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      font: "",
      letterSpacing: "",
      measureText: (text: string) => ({ width: text.length * 8 }),
    } as unknown as CanvasRenderingContext2D)
    render(<input aria-label="Name" style={{ padding: "0 12px" }} defaultValue={"x".repeat(60)} />)
    const field = screen.getByLabelText("Name") as HTMLInputElement
    let left = 0
    const scrolled = vi.fn((to: number) => (left = to))
    Object.defineProperties(field, {
      clientWidth: { value: 224, configurable: true },
      scrollWidth: { value: 504, configurable: true },
      scrollLeft: { get: () => left, set: scrolled },
    })
    return { field, scrolled }
  }

  it("scrolls a field to a caret past its right edge", () => {
    const { field, scrolled } = long()
    field.setSelectionRange(60, 60)
    revealCaret(field)
    // The caret at 480 px shows 1 px inside the right edge.
    expect(scrolled).toHaveBeenLastCalledWith(281)
  })

  it("scrolls back to a caret left of the view", () => {
    const { field, scrolled } = long()
    field.scrollLeft = 281
    field.setSelectionRange(10, 10)
    revealCaret(field)
    expect(scrolled).toHaveBeenLastCalledWith(80)
  })

  it("leaves a caret in view, and a field that doesn't scroll, alone", () => {
    const { field, scrolled } = long()
    field.scrollLeft = 281
    scrolled.mockClear()
    field.setSelectionRange(50, 50)
    revealCaret(field)
    expect(scrolled).not.toHaveBeenCalled()
    Object.defineProperty(field, "scrollWidth", { value: 224 })
    field.setSelectionRange(60, 60)
    revealCaret(field)
    expect(scrolled).not.toHaveBeenCalled()
  })
})

function Controlled({ onChange }: { onChange: (value: string) => void }) {
  const [value, setValue] = useState("Walter White")
  return (
    <input
      aria-label="Name"
      value={value}
      onChange={(e) => {
        setValue(e.target.value)
        onChange(e.target.value)
      }}
    />
  )
}
