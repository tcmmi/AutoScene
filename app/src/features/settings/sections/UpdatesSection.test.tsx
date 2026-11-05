import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { UpdatesSection } from "./UpdatesSection"

const { updates } = vi.hoisted(() => ({
  updates: {
    phase: "idle",
    auto: true,
    status: null,
    error: null,
    lastChecked: null,
    check: vi.fn(),
    setAuto: vi.fn(),
    setDialogOpen: vi.fn(),
  } as Record<string, unknown>,
}))
vi.mock("@/state/updates", () => ({ useUpdates: () => updates, releaseNotesUrl: () => "https://example.test" }))
vi.mock("@/lib/queries", () => ({ useAppInfo: () => ({ data: { version: "1.0.0" } }) }))
vi.mock("@/lib/os", () => ({ openExternalUrl: vi.fn() }))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  updates.phase = "idle"
})

const show = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <UpdatesSection />
    </QueryClientProvider>,
  )

describe("UpdatesSection", () => {
  it("Check for updates checks once; while it checks the button is busy and a click does nothing", async () => {
    const user = userEvent.setup()
    const { unmount } = show()
    const idle = screen.getByRole("button", { name: "Check for updates" })
    expect(idle.getAttribute("aria-busy")).toBeNull()
    await user.click(idle)
    expect(updates.check).toHaveBeenCalledOnce()
    unmount()
    updates.phase = "checking"
    show()
    const busy = screen.getByRole("button", { name: "Check for updates" })
    expect(busy.getAttribute("aria-busy")).toBe("true")
    await user.click(busy)
    expect(updates.check).toHaveBeenCalledOnce()
  })
})
