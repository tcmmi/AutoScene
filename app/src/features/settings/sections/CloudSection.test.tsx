import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { CloudLimits, CloudStage, CloudStatus } from "@/bindings"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { CloudSection } from "./CloudSection"

const { state } = vi.hoisted(() => ({
  state: { cents: 420, enabled: false, available: true, stage: "open" as CloudStage, limits: null as CloudLimits | null },
}))

/** As `commands::cloud` answers: a balance, never a key; no account before the launch. */
function status(): CloudStatus {
  const euros = (c: number) => `€${Math.floor(c / 100)}.${String(c % 100).padStart(2, "0")}`
  const launched = state.available && state.stage !== "hidden"
  return {
    available: state.available,
    enabled: state.enabled,
    stage: state.available ? state.stage : "hidden",
    account: launched
      ? {
          balance: euros(state.cents),
          minutes: state.cents,
          supportId: "AS-7F3K-29QD",
          betaUntil: null,
          blocked: false,
          paused: false,
          prices: { standard: "€0.01", fast: "€0.005", best: "€0.0125" },
          topUps: [500, 1000, 2000].map((cents) => ({ cents, label: euros(cents) })),
          limits: state.limits,
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
  Object.assign(state, { cents: 420, enabled: false, available: true, stage: "open", limits: null })
  queryClient.clear()
  vi.clearAllMocks()
})
afterEach(cleanup)

describe("Settings → Cloud", () => {
  it("shows an account's limits and what it used of them", async () => {
    state.limits = {
      day: { used: "€0.40", limit: "€2.00" },
      week: null,
      month: { used: "€4.00", limit: "€10.00" },
      scansToday: 2,
      scansPerDay: 5,
      maxMinutes: 60,
      kinds: ["live_fast", "anime_fast", "scenes"],
    }
    open()
    expect(await screen.findByText("Your limits")).toBeTruthy()
    expect(screen.getByText("Today: €0.40 of €2.00")).toBeTruthy()
    expect(screen.getByText("This month: €4.00 of €10.00")).toBeTruthy()
    expect(screen.queryByText(/This week/)).toBeNull()
    expect(screen.getByText("2 of 5 scans today")).toBeTruthy()
    expect(screen.getByText("Up to 60 minutes of video a scan")).toBeTruthy()
    expect(screen.getByText("Only: Live action with Fast Scan, Anime with Fast Scan, Whole scenes")).toBeTruthy()
    expect(screen.getByText("Days, weeks (from Monday) and months count in UTC.")).toBeTruthy()
  })

  it("has no limits row for an account without limits", async () => {
    open()
    await screen.findByText("€4.20")
    expect(screen.queryByText("Your limits")).toBeNull()
  })

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

  it("shows nothing but that before the launch", async () => {
    state.stage = "hidden"
    open()
    expect(await screen.findByText("AutoScene Cloud isn't available yet.")).toBeTruthy()
    expect(screen.queryByRole("switch")).toBeNull()
    expect(screen.queryByRole("textbox", { name: "Redeem a code" })).toBeNull()
  })

  it("keeps a switch left on in reach while the cloud is closed", async () => {
    Object.assign(state, { stage: "hidden", enabled: true })
    open()
    expect(
      await screen.findByText("AutoScene Cloud is off for now. Turn this off to analyse on this computer."),
    ).toBeTruthy()
    const use = screen.getByRole("switch", { name: "Analyse in the cloud" })
    await userEvent.click(use)
    expect(commands.cloudSetEnabled).toHaveBeenCalledWith(false)
    await waitFor(() => expect(screen.queryByRole("switch")).toBeNull())
  })

  it("takes codes in the beta, but no money", async () => {
    state.stage = "beta"
    open()
    expect(await screen.findByText("Closed beta")).toBeTruthy()
    expect(screen.getByRole("textbox", { name: "Redeem a code" })).toBeTruthy()
    expect(screen.queryByRole("button", { name: "€10.00" })).toBeNull()
    expect(screen.queryByText("Add funds")).toBeNull()
  })
})
