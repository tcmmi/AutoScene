import { useState } from "react"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it } from "vitest"
import { OptionRow } from "./option-row"

afterEach(cleanup)

function Row({ disabled }: { disabled?: boolean }) {
  const [on, setOn] = useState(false)
  return <OptionRow label="Delete its analysis too" description="Shots, faces and the cast." checked={on} disabled={disabled} onCheckedChange={setOn} />
}

const row = () => screen.getByRole("switch", { name: "Delete its analysis too" })

describe("OptionRow", () => {
  it("the whole row is the target: its label, its description and the switch", async () => {
    const user = userEvent.setup()
    render(<Row />)
    expect(row().getAttribute("aria-checked")).toBe("false")
    await user.click(screen.getByText("Delete its analysis too"))
    expect(row().getAttribute("aria-checked")).toBe("true")
    await user.click(screen.getByText("Shots, faces and the cast."))
    expect(row().getAttribute("aria-checked")).toBe("false")
    await user.click(row())
    expect(row().getAttribute("aria-checked")).toBe("true")
  })

  it("the switch is named by the label alone; the description describes it", () => {
    render(<Row />)
    expect(document.getElementById(row().getAttribute("aria-describedby") ?? "")?.textContent).toBe("Shots, faces and the cast.")
  })

  it("does without a description", () => {
    render(<OptionRow label="Keep it" checked={false} onCheckedChange={() => {}} />)
    expect(screen.getByRole("switch", { name: "Keep it" }).hasAttribute("aria-describedby")).toBe(false)
  })

  it("disabled, nothing in it turns it", async () => {
    const user = userEvent.setup()
    render(<Row disabled />)
    await user.click(screen.getByText("Delete its analysis too"))
    expect(row().getAttribute("aria-checked")).toBe("false")
  })

  it("disabled, the whole row looks it: dimmed, no hand", () => {
    const { container } = render(<Row disabled />)
    const cls = container.querySelector("label")?.className ?? ""
    expect(cls).toContain("has-[:disabled]:opacity-50")
    expect(cls).toContain("has-[:disabled]:cursor-default")
  })

  it("className goes on the row, the box the two dialogs draw today", () => {
    const { container } = render(<OptionRow label="Keep it" className="mt-3" />)
    const label = container.querySelector("label")
    expect([...(label?.classList ?? [])].sort()).toEqual(
      "flex cursor-pointer items-center justify-between gap-4 rounded-lg border border-hairline bg-inset px-3.5 py-3 transition-opacity has-[:disabled]:cursor-default has-[:disabled]:opacity-50 mt-3"
        .split(" ")
        .sort(),
    )
  })
})
