import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { AppError } from "@/bindings"
import { PuzzlePiece } from "@/components/icons"
import { ProjectLoadError } from "@/features/project/ProjectLoadError"
import { commands } from "@/lib/ipc"
import { inLanguage } from "@/test/i18n"
import { ErrorState } from "./error-state"

vi.mock("@tanstack/react-router", () => ({ Link: () => null }))

vi.mock("@/lib/ipc", async (real) => ({
  ...(await real<typeof import("@/lib/ipc")>()),
  commands: { openLogsDir: vi.fn(() => Promise.resolve()) },
}))

afterEach(cleanup)

const appError = (code: string) => ({ code, message: "The plugin list is damaged" })

const failed = (code: AppError["code"]): AppError => ({
  code,
  message: "The disk is full.",
  msg: { key: "file.diskFull", text: "The disk is full." },
})

const romanian = { backend: { file: { diskFull: "Discul este plin." } } }

describe("ErrorState", () => {
  it("Try again goes busy while it retries, and takes no second click", async () => {
    const user = userEvent.setup()
    const onRetry = vi.fn()
    render(<ErrorState error={appError("not_found")} onRetry={onRetry} retrying />)
    const again = screen.getByRole("button", { name: "Try again" })
    expect(again.getAttribute("aria-busy")).toBe("true")
    expect(again.querySelectorAll("svg").length).toBe(1)
    expect(again.querySelector("svg.animate-spin")).toBeTruthy()
    await user.click(again)
    expect(onRetry).not.toHaveBeenCalled()
  })

  it("Try again works once it isn't retrying", async () => {
    const user = userEvent.setup()
    const onRetry = vi.fn()
    render(<ErrorState error={appError("not_found")} onRetry={onRetry} retrying={false} />)
    await user.click(screen.getByRole("button", { name: "Try again" }))
    expect(onRetry).toHaveBeenCalledOnce()
  })

  it("draws the icon it is given, the warning without one", () => {
    const { container, rerender } = render(<ErrorState error={appError("not_found")} />)
    const warning = container.querySelector("svg")?.innerHTML
    rerender(<ErrorState error={appError("not_found")} icon={<PuzzlePiece data-testid="piece" />} />)
    expect(screen.getByTestId("piece")).toBeTruthy()
    expect(container.querySelector("svg")?.innerHTML).not.toBe(warning)
  })

  it("puts other ways out between Try again and Open logs", () => {
    render(<ErrorState error={appError("io")} onRetry={() => {}} actions={<button type="button">Go to Plugins</button>} />)
    expect(screen.getAllByRole("button").map((b) => b.textContent?.trim())).toEqual(["Try again", "Go to Plugins", "Open logs"])
  })

  it("offers the logs for the app's own faults only, with or without actions", async () => {
    const user = userEvent.setup()
    const { rerender } = render(<ErrorState error={appError("not_found")} actions={<button type="button">Go to Plugins</button>} />)
    expect(screen.queryByRole("button", { name: "Open logs" })).toBeNull()
    rerender(<ErrorState error={appError("internal")} actions={<button type="button">Go to Plugins</button>} />)
    await user.click(screen.getByRole("button", { name: "Open logs" }))
    expect(commands.openLogsDir).toHaveBeenCalledOnce()
  })

  it("a bug's raw words stay small and copyable under the plain sentence", () => {
    render(<ErrorState error={new Error("x is undefined")} />)
    expect(screen.getByText("Something went wrong while showing this.")).toBeTruthy()
    expect(screen.getByText("x is undefined").hasAttribute("data-selectable")).toBe(true)
  })
})

describe("what went wrong", () => {
  it("reads in the UI's language on an error screen", async () => {
    render(<ErrorState error={failed("io")} />)
    expect(screen.getByText("The disk is full.")).toBeTruthy()
    await inLanguage("ro", romanian)
    render(<ErrorState error={failed("io")} />)
    expect(screen.getByText("Discul este plin.")).toBeTruthy()
  })

  it("reads in the UI's language where a project is gone", async () => {
    await inLanguage("ro", romanian)
    render(<ProjectLoadError error={failed("not_found")} onRetry={() => {}} />)
    expect(screen.getByText("Discul este plin.")).toBeTruthy()
  })
})
