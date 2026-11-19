import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { CloudStatus } from "@/bindings"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { CloudSection } from "./CloudSection"

const { state } = vi.hoisted(() => ({ state: { cents: 420, enabled: false, available: true } }))

/** As `commands::cloud` answers: a balance, never a key. */
function status(): CloudStatus {
  const euros = (c: number) => `€${Math.floor(c / 100)}.${String(c % 100).padStart(2, "0")}`
  return {
    available: state.available,
    enabled: state.enabled,
    account: state.available
      ? {
          balance: euros(state.cents),
          minutes: state.cents,
          supportId: "AS-7F3K-29QD",
          betaUntil: null,
          blocked: false,
          paused: false,
          prices: { standard: "€0.01", fast: "€0.005", best: "€0.0125" },
          topUps: [500, 1000, 2000].map((cents) => ({ cents, label: euros(cents) })),
        }
      : null,
    error: null,
  }
}

vi.mock("@/lib/ipc", async (original) => ({
  ...(await original<typeof import("@/lib/ipc")>()),
  commands: {
    cloudStatus: vi.fn(async () => status()),
    cloudSetEnabled: vi.fn(async (on: boolean) => {
      state.enabled = on
      return status()
    }),
    cloudRedeem: vi.fn(async (code: string) => {
      if (code !== "ABCD-EFGH-JKMN-PQRS") throw { code: "invalid", message: "That code doesn't exist or has expired." }
      state.cents += 500
      return status()
    }),
    cloudAddFunds: vi.fn(async () => null),
  },
}))

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }))

const open = () =>
  render(
    <QueryClientProvider client={queryClient}>
      <CloudSection />
    </QueryClientProvider>,
  )

beforeEach(() => {
  Object.assign(state, { cents: 420, enabled: false, available: true })
  queryClient.clear()
  vi.clearAllMocks()
})
afterEach(cleanup)

describe("Settings → Cloud", () => {
  it("shows the balance, never a key", async () => {
    open()
    expect(await screen.findByText("€4.20")).toBeTruthy()
    expect(screen.getByText("About 420 minutes of video")).toBeTruthy()
    expect(screen.getByText("AS-7F3K-29QD")).toBeTruthy()
    expect(screen.getByText("Standard €0.01 · Fast Scan €0.005 · Best €0.0125")).toBeTruthy()
  })

  it("sends analyses to the cloud with its switch", async () => {
    open()
    const use = await screen.findByRole("switch", { name: "Analyse in the cloud" })
    await userEvent.click(use)
    expect(commands.cloudSetEnabled).toHaveBeenCalledWith(true)
    await waitFor(() => expect(use.getAttribute("aria-checked")).toBe("true"))
  })

  it("adds a code's money to the balance, and says why a code won't do", async () => {
    open()
    const field = await screen.findByRole("textbox", { name: "Redeem a code" })
    await userEvent.type(field, "nope")
    await userEvent.click(screen.getByRole("button", { name: "Redeem" }))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    await userEvent.clear(field)
    await userEvent.type(field, "ABCD-EFGH-JKMN-PQRS")
    await userEvent.click(screen.getByRole("button", { name: "Redeem" }))
    expect(await screen.findByText("€9.20")).toBeTruthy()
    expect(toast.success).toHaveBeenCalledWith("Code redeemed")
  })

  it("opens the payment page for a top-up", async () => {
    open()
    await userEvent.click(await screen.findByRole("button", { name: "€10.00" }))
    expect(commands.cloudAddFunds).toHaveBeenCalledWith(1000)
  })

  it("says when the cloud isn't there yet", async () => {
    state.available = false
    open()
    expect(await screen.findByText("AutoScene Cloud isn't available yet.")).toBeTruthy()
    expect(screen.queryByRole("switch")).toBeNull()
  })
})
