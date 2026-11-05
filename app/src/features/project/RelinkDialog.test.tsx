import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { MediaItem } from "@/bindings"
import { useRelink } from "@/state/relink"
import { RelinkDialog } from "./RelinkDialog"
import { relinkAnyway } from "./relink"

vi.mock("./relink", () => ({ relinkAnyway: vi.fn(() => Promise.resolve()) }))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useRelink.setState({ question: null })
})

const media = { id: "m", fileName: "E01.mkv", label: "S01E01" } as MediaItem

describe("RelinkDialog", () => {
  it("Use it anyway is busy while it runs: a second click, Esc and Cancel do nothing", async () => {
    const user = userEvent.setup()
    let finish = () => {}
    vi.mocked(relinkAnyway).mockImplementationOnce(() => new Promise((done) => (finish = () => done())))
    render(<RelinkDialog />)
    act(() => useRelink.getState().ask({ pid: "p", media, path: "D:/x/other.mkv", reason: "It is shorter." }))
    const dialog = await screen.findByRole("dialog")
    const button = within(dialog).getByRole("button", { name: "Use it anyway" })
    await user.click(button)
    await waitFor(() => expect(button.getAttribute("aria-busy")).toBe("true"))
    await user.click(button)
    await user.keyboard("{Escape}")
    expect(relinkAnyway).toHaveBeenCalledOnce()
    expect(screen.getByRole("dialog")).toBeTruthy()
    finish()
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  })
})
