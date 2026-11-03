import { useState } from "react"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it } from "vitest"
import { Checkbox } from "./checkbox"

afterEach(cleanup)

function Ticked() {
  const [on, setOn] = useState(false)
  return <Checkbox label="Don't show this again" checked={on} onCheckedChange={(v) => setOn(v === true)} />
}

describe("Checkbox", () => {
  it("the label toggles it; Space toggles it", async () => {
    const user = userEvent.setup()
    render(<Ticked />)
    const box = screen.getByRole("checkbox", { name: "Don't show this again" })
    expect(box.getAttribute("aria-checked")).toBe("false")

    await user.click(screen.getByText("Don't show this again"))
    expect(box.getAttribute("aria-checked")).toBe("true")
    expect(document.activeElement).toBe(box)

    await user.keyboard(" ")
    expect(box.getAttribute("aria-checked")).toBe("false")
    await user.keyboard(" ")
    expect(box.getAttribute("aria-checked")).toBe("true")
  })
})
