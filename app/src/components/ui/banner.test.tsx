import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { MediaItem } from "@/bindings"
import { MissingBanner } from "@/features/project/MissingBanner"
import { Banner } from "./banner"

vi.mock("@/features/project/relink", () => ({ locateFolder: vi.fn() }))

afterEach(cleanup)

const classes = (el: Element) => [...el.classList].sort()

describe("Banner", () => {
  it("MissingBanner looks as it did", () => {
    const media = [{ status: "missing" }, { status: "ready" }] as MediaItem[]
    render(<MissingBanner pid="p" media={media} className="mb-6" />)
    const root = screen.getByRole("status")
    // The banner it was before the shared one: the same classes on the same box.
    const before = "flex flex-wrap items-center gap-x-4 gap-y-3 rounded-2xl border border-accent-line/50 bg-accent-subtle px-5 py-4 mb-6"
    expect(classes(root)).toEqual(before.split(" ").sort())
    expect(root.querySelector("p.text-md.font-medium.text-fg-1")?.textContent).toBe("1 episode can't be found")
    expect(root.querySelector("p.text-sm.text-fg-2")?.textContent).toMatch(/^Moved, renamed/)
    expect(screen.getByRole("button", { name: "Locate folder…" })).toBeTruthy()
  })

  it("MissingBanner shows nothing when nothing is missing", () => {
    render(<MissingBanner pid="p" media={[{ status: "ready" } as MediaItem]} />)
    expect(screen.queryByRole("status")).toBeNull()
  })

  it("is gold for what the person can fix and red for what was taken away", () => {
    render(
      <>
        <Banner tone="accent" icon={<i />} title="Gold" />
        <Banner tone="danger" icon={<i />} title="Red" description="Why." actions={<button type="button">Do</button>} />
      </>,
    )
    const [gold, red] = screen.getAllByRole("status")
    expect(gold.className).toContain("bg-accent-subtle")
    expect(red.className).toContain("bg-danger-subtle")
    expect(red.className).not.toContain("accent")
    expect(gold.querySelector("p.text-sm")).toBeNull()
    expect(red.querySelector("p.text-sm")?.textContent).toBe("Why.")
    expect(screen.getByRole("button", { name: "Do" })).toBeTruthy()
  })
})
