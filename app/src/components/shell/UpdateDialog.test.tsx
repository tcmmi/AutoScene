import { act, cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { useFastScanNotice } from "@/state/fast-scan-notice"
import { useUpdates } from "@/state/updates"
import { UpdateDialog } from "./UpdateDialog"

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: { updateCheck: vi.fn() },
}))

afterEach(() => {
  cleanup()
  useUpdates.setState({ status: null, dialogOpen: false, phase: "idle" })
  useFastScanNotice.setState({ phase: "done" })
})

describe("UpdateDialog", () => {
  it("waits while the notice is open", () => {
    useFastScanNotice.setState({ phase: "open" })
    useUpdates.setState({
      phase: "available",
      dialogOpen: true,
      status: { state: "available", version: "1.1.0", currentVersion: "1.0.3-BETA", date: null, notes: null, simulated: false },
    } as never)
    render(<UpdateDialog />)
    expect(screen.queryByRole("dialog")).toBeNull()

    act(() => useFastScanNotice.setState({ phase: "done" }))
    expect(screen.getByRole("dialog", { name: "AutoScene 1.1.0 is available" })).toBeTruthy()
  })
})
