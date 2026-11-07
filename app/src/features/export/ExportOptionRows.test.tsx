import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { ExportPreset } from "@/bindings"
import { ExportOptionRows, NO_EXPORT_OPTIONS } from "./ExportOptionRows"

const h264 = { id: "h264-edit", video: { codec: "h264" } } as unknown as ExportPreset
const bitPerfect = { id: "bit-perfect", video: { codec: "lossless" } } as unknown as ExportPreset

afterEach(cleanup)

describe("ExportOptionRows", () => {
  it("turns an option on and leaves the other as it is", async () => {
    const onChange = vi.fn()
    render(<ExportOptionRows preset={h264} options={{ removeBars: false, matchLoudness: true }} onChange={onChange} />)
    await userEvent.click(screen.getByRole("switch", { name: "Remove black bars" }))
    expect(onChange).toHaveBeenCalledWith({ removeBars: true, matchLoudness: true })
  })

  it("offers nothing for Bit-Perfect, which keeps the source as it is", () => {
    render(<ExportOptionRows preset={bitPerfect} options={NO_EXPORT_OPTIONS} onChange={vi.fn()} />)
    expect(screen.queryByRole("switch")).toBeNull()
  })
})
