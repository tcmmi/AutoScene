import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { MediaItem, Msg } from "@/bindings"
import { useRelink } from "@/state/relink"
import { inLanguage } from "@/test/i18n"
import { RelinkDialog } from "./RelinkDialog"
import { relinkAnyway } from "./relink"

vi.mock("./relink", () => ({ relinkAnyway: vi.fn(() => Promise.resolve()) }))

const reason: Msg = {
  key: "relink.length",
  args: { length: "40:00", was: "52:11" },
  text: "It's 40:00 long instead of 52:11.",
}
const media = { id: "m", fileName: "E01.mkv", label: "S01E01" } as MediaItem
const asked = () => useRelink.getState().ask({ pid: "p", media, path: "D:/x/other.mkv", reason })

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useRelink.setState({ question: null })
})

describe("RelinkDialog", () => {
  it("Use it anyway is busy while it runs: a second click, Esc and Cancel do nothing", async () => {
    const user = userEvent.setup()
    let finish = () => {}
    vi.mocked(relinkAnyway).mockImplementationOnce(() => new Promise((done) => (finish = () => done())))
    render(<RelinkDialog />)
    act(asked)
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

  it("says how the file differs, in English as it always did", () => {
    render(<RelinkDialog />)
    act(asked)
    return expect(screen.findByText(/^It doesn't look like the same episode\. It's 40:00 long instead of 52:11\. If you use it anyway/)).resolves.toBeTruthy()
  })

  it("says how the file differs in the UI's language", async () => {
    await inLanguage("ro", { backend: { relink: { length: "Durează {{length}} în loc de {{was}}." } } })
    render(<RelinkDialog />)
    act(asked)
    expect(await screen.findByText(/Durează 40:00 în loc de 52:11\./)).toBeTruthy()
  })
})
