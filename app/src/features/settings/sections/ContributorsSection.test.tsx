import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { openExternalUrl } from "@/lib/os"
import { inLanguage } from "@/test/i18n"
import { ContributorsSection } from "./ContributorsSection"

vi.mock("@/lib/os", () => ({
  openExternalUrl: vi.fn(),
}))

afterEach(cleanup)

describe("ContributorsSection", () => {
  it("renders contributors and links to their profiles", () => {
    render(<ContributorsSection />)

    expect(screen.getByText("Community Contributors")).toBeTruthy()
    expect(screen.getByText("Contribute on GitHub")).toBeTruthy()

    expect(screen.getByText("tcmmi")).toBeTruthy()
    expect(screen.getByText("0nsku")).toBeTruthy()
    expect(screen.getByText("ihyledd")).toBeTruthy()
    expect(screen.getByText("Moongetsu")).toBeTruthy()

    const contributeButton = screen.getByRole("button", { name: /Contribute on GitHub/i })
    fireEvent.click(contributeButton)
    expect(openExternalUrl).toHaveBeenCalledWith("https://github.com/tcmmi/AutoScene")

    const profileButton = screen.getByRole("button", { name: /Open Moongetsu's GitHub profile/i })
    fireEvent.click(profileButton)
    expect(openExternalUrl).toHaveBeenCalledWith("https://github.com/moongetsu")
  })

  it("says what each person did in the UI's language", async () => {
    render(<ContributorsSection />)
    expect(screen.getByText("Creator, media engine, models, and core architecture")).toBeTruthy()
    cleanup()
    await inLanguage("ro")
    render(<ContributorsSection />)
    expect(screen.getByText("Creator, motorul media, modelele și arhitectura de bază")).toBeTruthy()
    expect(screen.queryByText(/media engine/)).toBeNull()
  })
})
