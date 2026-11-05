import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { MediaItem } from "@/bindings"
import { MissingBanner } from "./MissingBanner"
import { locateFolder } from "./relink"

vi.mock("./relink", () => ({ locateFolder: vi.fn(() => Promise.resolve(true)) }))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const lost = { id: "m", status: "missing" } as MediaItem

describe("MissingBanner", () => {
  it("looks in one folder at a time: Locate folder is busy, and a second click does nothing", async () => {
    const user = userEvent.setup()
    let finish = () => {}
    vi.mocked(locateFolder).mockImplementationOnce(() => new Promise((done) => (finish = () => done(true))))
    render(<MissingBanner pid="p" media={[lost]} />)
    const button = screen.getByRole("button", { name: "Locate folder…" })
    await user.click(button)
    await waitFor(() => expect(button.getAttribute("aria-busy")).toBe("true"))
    await user.click(button)
    expect(locateFolder).toHaveBeenCalledOnce()
    finish()
    await waitFor(() => expect(button.getAttribute("aria-busy")).toBeNull())
  })
})
