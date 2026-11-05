import { useState } from "react"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { Trash } from "@/components/icons"
import { ConfirmDialog } from "./confirm-dialog"
import { OptionRow } from "./option-row"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }))

beforeEach(() => vi.mocked(toast.error).mockClear())
afterEach(cleanup)

/** A promise the test ends, for what a running action looks like. */
function pending() {
  let resolve = () => {}
  const promise = new Promise<void>((done) => {
    resolve = done
  })
  return { promise, resolve: () => resolve() }
}

function Asking({ onConfirm, onClosed }: { onConfirm: () => Promise<unknown>; onClosed?: () => void }) {
  const [open, setOpen] = useState(true)
  const [too, setToo] = useState(false)
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) onClosed?.()
      }}
      title="Remove it?"
      description="It goes for good."
      answer="Remove"
      icon={<Trash />}
      failure="Couldn't remove it"
      onConfirm={onConfirm}
    >
      <OptionRow label="Delete its data too" checked={too} onCheckedChange={setToo} />
    </ConfirmDialog>
  )
}

/** A modal dialog turns pointer events off around it, so user-event won't go there. Radix listens from the next tick on, and a dialog waits for the click. */
async function clickOutside() {
  await act(() => new Promise((done) => setTimeout(done)))
  fireEvent.pointerDown(document.body)
  await act(async () => {
    fireEvent.click(document.body)
  })
}

const answer = () => screen.getByRole("button", { name: "Remove" }) as HTMLButtonElement
const cancel = () => screen.getByRole("button", { name: "Cancel" }) as HTMLButtonElement

describe("ConfirmDialog", () => {
  it("asks: title, description, what goes with the answer, Cancel and one answer", () => {
    render(<Asking onConfirm={() => Promise.resolve()} />)
    expect(screen.getByRole("dialog", { name: "Remove it?" })).toBeTruthy()
    expect(screen.getByText("It goes for good.")).toBeTruthy()
    expect(screen.getByRole("switch", { name: "Delete its data too" })).toBeTruthy()
    expect(cancel().className).toContain("hover:bg-hover")
    expect(answer().className).toContain("bg-danger-subtle")
  })

  it("Cancel closes without answering, and so do Esc and the ×", async () => {
    const user = userEvent.setup()
    for (const leave of [() => user.click(cancel()), () => user.keyboard("{Escape}"), () => user.click(screen.getByRole("button", { name: "Close" }))]) {
      const onConfirm = vi.fn(() => Promise.resolve())
      const onClosed = vi.fn()
      render(<Asking onConfirm={onConfirm} onClosed={onClosed} />)
      await leave()
      expect(onClosed).toHaveBeenCalledOnce()
      expect(onConfirm).not.toHaveBeenCalled()
      expect(screen.queryByRole("dialog")).toBeNull()
      cleanup()
    }
  })

  it("while it runs nothing closes it, and it closes itself once done", async () => {
    const user = userEvent.setup()
    const run = pending()
    const onClosed = vi.fn()
    render(<Asking onConfirm={() => run.promise} onClosed={onClosed} />)
    await user.click(answer())

    expect(answer().getAttribute("aria-busy")).toBe("true")
    expect(cancel().disabled).toBe(true)
    await user.keyboard("{Escape}")
    await user.click(cancel())
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull()
    await clickOutside()
    expect(onClosed).not.toHaveBeenCalled()
    expect(screen.getByRole("dialog")).toBeTruthy()

    await act(async () => run.resolve())
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    expect(onClosed).toHaveBeenCalledOnce()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("a click outside closes it when nothing runs, so the test above proves something", async () => {
    const onClosed = vi.fn()
    render(<Asking onConfirm={() => Promise.resolve()} onClosed={onClosed} />)
    await clickOutside()
    expect(onClosed).toHaveBeenCalledOnce()
  })

  it("while it runs what goes with the answer is off", async () => {
    const user = userEvent.setup()
    const run = pending()
    render(<Asking onConfirm={() => run.promise} />)
    const row = screen.getByRole("switch", { name: "Delete its data too" })
    await user.click(row)
    expect(row.getAttribute("aria-checked")).toBe("true")

    await user.click(answer())
    // jsdom has no inert: the attribute is what the browser acts on, for every kind of child.
    expect(row.closest("[inert]")).not.toBeNull()
    await act(async () => run.resolve())
  })

  it("answers once, however fast the clicks come", async () => {
    const user = userEvent.setup()
    const run = pending()
    const onConfirm = vi.fn(() => run.promise)
    render(<Asking onConfirm={onConfirm} />)
    await user.dblClick(answer())
    expect(onConfirm).toHaveBeenCalledOnce()
    await act(async () => run.resolve())
  })

  it("a failed action stays open, says why and can be tried again", async () => {
    const user = userEvent.setup()
    const onClosed = vi.fn()
    const onConfirm = vi.fn(() => Promise.reject({ code: "io", message: "The disk is full" }))
    render(<Asking onConfirm={onConfirm} onClosed={onClosed} />)
    await user.click(answer())

    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't remove it", { description: "The disk is full" }))
    expect(screen.getByRole("dialog")).toBeTruthy()
    expect(onClosed).not.toHaveBeenCalled()
    await waitFor(() => expect(answer().disabled).toBe(false))
    expect(cancel().disabled).toBe(false)
    expect(answer().hasAttribute("aria-busy")).toBe(false)

    onConfirm.mockResolvedValueOnce(undefined as never)
    await user.click(answer())
    await waitFor(() => expect(onClosed).toHaveBeenCalledOnce())
  })

  it("an action that throws before it awaits is a failure like any other", async () => {
    const user = userEvent.setup()
    render(
      <Asking
        onConfirm={() => {
          throw new Error("No such plugin")
        }}
      />,
    )
    await user.click(answer())
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't remove it", { description: "No such plugin" }))
    expect(screen.getByRole("dialog")).toBeTruthy()
  })

  it("opened again after an answer it isn't held", async () => {
    const user = userEvent.setup()
    function Twice() {
      const [open, setOpen] = useState(true)
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>
            Ask
          </button>
          <ConfirmDialog
            open={open}
            onOpenChange={setOpen}
            title="Remove it?"
            answer="Remove"
            failure="Couldn't remove it"
            onConfirm={() => Promise.resolve()}
          />
        </>
      )
    }
    render(<Twice />)
    await user.click(answer())
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    await user.click(screen.getByRole("button", { name: "Ask", hidden: true }))
    expect(answer().disabled).toBe(false)
    expect(cancel().disabled).toBe(false)
  })

  it("without anything to go with the answer there is no empty group", () => {
    render(
      <ConfirmDialog open onOpenChange={() => {}} title="Remove it?" answer="Remove" failure="No" onConfirm={() => Promise.resolve()} />,
    )
    expect(screen.getByRole("dialog").querySelector("[inert]")).toBeNull()
    expect(answer().querySelector("svg")).toBeNull()
  })
})
