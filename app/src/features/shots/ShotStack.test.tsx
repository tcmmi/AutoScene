import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { CharacterShot, MediaItem } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { stackCards } from "./groups"
import { ShotStack } from "./ShotStack"

const media = { id: "e1", label: "E01", fps: { num: 24, den: 1 } } as MediaItem

/** `n` shots of a little over two seconds, the first at 05:13; the second has the biggest face. */
function card(n: number, more: Partial<CharacterShot> = {}) {
  const at = (5 * 60 + 13) * 24
  const shots = Array.from({ length: n }, (_, i) => ({
    mediaId: "e1",
    start: at + i * 60,
    end: at + i * 60 + 53,
    sprite: `s${i}.jpg`,
    cells: 4,
    trackId: i + 1,
    face: `f${i}.jpg`,
    faceHeight: i === 1 ? 0.5 : 0.2,
    ...more,
  }))
  return stackCards([{ shots }])[0]
}

function show(n: number, { open = false, more = {} }: { open?: boolean; more?: Partial<CharacterShot> } = {}) {
  const onToggle = vi.fn()
  const onWatch = vi.fn()
  const c = card(n, more)
  render(<ShotStack card={c} media={media} open={open} onToggle={onToggle} onWatch={onWatch} />, { wrapper: TooltipProvider })
  const stack = screen.getByRole("button", { expanded: open })
  return { c, stack, onToggle, onWatch }
}

/** The pointer `share` of the way across a 400 px wide stack. */
async function hover(stack: HTMLElement, share: number) {
  const pile = stack.parentElement!
  vi.spyOn(pile, "getBoundingClientRect").mockReturnValue({ left: 0, width: 400 } as DOMRect)
  await act(async () => {
    fireEvent.pointerMove(stack, { clientX: share * 400 })
  })
}

const sprite = (stack: HTMLElement) => stack.querySelector("img")?.getAttribute("src")

beforeEach(() => {
  HTMLImageElement.prototype.decode = () => Promise.resolve()
})
afterEach(cleanup)

describe("a stack of shots", () => {
  it("says how many shots it holds and how long they run together", () => {
    const { stack } = show(5)
    expect(stack.textContent).toContain("5 shots · 11 s")
    expect(stack.getAttribute("aria-label")).toBe("5 shots at E01 · 05:13, 11 s")
  })

  it("rests on its lead shot", () => {
    const { stack } = show(5)
    expect(sprite(stack)).toBe("s1.jpg")
  })

  it("opens on a click, and a second click on an open one closes it", () => {
    const { c, stack, onToggle } = show(3)
    fireEvent.click(stack)
    expect(onToggle).toHaveBeenLastCalledWith(c, true)
    cleanup()
    const open = show(3, { open: true })
    fireEvent.click(open.stack)
    expect(open.onToggle).toHaveBeenLastCalledWith(open.c, false)
  })

  it("opens on Space, once however long the key is held, and watches on Enter", () => {
    const { c, stack, onToggle, onWatch } = show(3)
    fireEvent.keyDown(stack, { key: " " })
    fireEvent.keyDown(stack, { key: " ", repeat: true })
    expect(onToggle).toHaveBeenCalledOnce()
    expect(onWatch).not.toHaveBeenCalled()
    fireEvent.keyDown(stack, { key: "Enter" })
    expect(onWatch).toHaveBeenCalledWith(c)
    expect(onToggle).toHaveBeenCalledOnce()
  })

  it("keeps Space from reaching the button: its keyup would toggle a second time", () => {
    const { stack } = show(3)
    expect(fireEvent.keyDown(stack, { key: " " })).toBe(false)
  })

  it("tells where it is while closed, and nothing while open", async () => {
    const { stack } = show(3)
    act(() => stack.focus())
    expect((await screen.findByRole("tooltip")).textContent).toContain("3 shots close together")
    cleanup()
    const open = show(3, { open: true })
    act(() => open.stack.focus())
    await act(async () => {})
    expect(screen.queryByRole("tooltip")).toBeNull()
  })

  it("points at the band only while it is open", () => {
    const { stack } = show(3)
    expect(stack.getAttribute("aria-controls")).toBeNull()
    cleanup()
    expect(show(3, { open: true }).stack.getAttribute("aria-controls")).toBe("shot-band")
  })

  it("watches from the first frame with its Play button, without opening", () => {
    const { c, onToggle, onWatch } = show(3)
    fireEvent.click(screen.getByRole("button", { name: "Watch from E01 · 05:13" }))
    expect(onWatch).toHaveBeenCalledWith(c)
    expect(onToggle).not.toHaveBeenCalled()
  })

  it("scrubs to the shot under the pointer, and back to its lead when it leaves", async () => {
    const { stack } = show(4)
    await hover(stack, 0.8)
    await waitFor(() => expect(sprite(stack)).toBe("s3.jpg"))
    await hover(stack, 0.1)
    await waitFor(() => expect(sprite(stack)).toBe("s0.jpg"))
    fireEvent.pointerLeave(stack.parentElement!)
    expect(sprite(stack)).toBe("s1.jpg")
  })

  it("scrubs again when the pointer comes back to the same shot", async () => {
    const { stack } = show(4)
    await hover(stack, 0.8)
    await waitFor(() => expect(sprite(stack)).toBe("s3.jpg"))
    fireEvent.pointerLeave(stack.parentElement!)
    await hover(stack, 0.8)
    await waitFor(() => expect(sprite(stack)).toBe("s3.jpg"))
  })

  it("changes shot only once its sprite is decoded, and loads no more than the neighbours", async () => {
    const asked: string[] = []
    const finish: (() => void)[] = []
    HTMLImageElement.prototype.decode = function () {
      asked.push(this.getAttribute("src")!)
      return new Promise((done) => finish.push(done))
    }
    const { stack } = show(43)
    await hover(stack, 21.5 / 43)
    expect(asked).toEqual(["s21.jpg", "s20.jpg", "s22.jpg"])
    expect(sprite(stack)).toBe("s1.jpg")
    await act(async () => finish[0]())
    expect(sprite(stack)).toBe("s21.jpg")
  })

  it("keeps a sprite that won't decode from stopping the scrub", async () => {
    HTMLImageElement.prototype.decode = () => Promise.reject(new Error("broken"))
    const { stack } = show(4)
    await hover(stack, 0.8)
    await waitFor(() => expect(sprite(stack)).toBe("s3.jpg"))
  })

  it("goes back to its lead when an edit takes the shot under the pointer", async () => {
    const c = card(4)
    const props = { media, open: false, onToggle: vi.fn(), onWatch: vi.fn() }
    const view = render(<ShotStack card={c} {...props} />, { wrapper: TooltipProvider })
    const stack = screen.getByRole("button", { expanded: false })
    await hover(stack, 0.8)
    await waitFor(() => expect(sprite(stack)).toBe("s3.jpg"))
    view.rerender(<ShotStack card={{ ...c, shots: c.shots.slice(0, 2) }} {...props} />)
    expect(sprite(stack)).toBe("s1.jpg")
  })

  it("marks the shot under the pointer on a bar of one segment per shot", async () => {
    const { stack } = show(5)
    await hover(stack, 0.5)
    const bar = stack.querySelector(".gap-0\\.5")!
    expect([...bar.children].map((s) => s.className.match(/bg-[^ ]+/)?.[0])).toEqual([
      "bg-white/50",
      "bg-white/50",
      "bg-accent",
      "bg-white/20",
      "bg-white/20",
    ])
    expect(stack.textContent).toContain("5 shots")
  })

  it("counts instead above twelve shots, while the pointer is on it", async () => {
    const { stack } = show(43)
    expect(stack.textContent).toContain("43 shots")
    await hover(stack, 6.5 / 43)
    expect(stack.textContent).toContain("7 / 43")
    expect(stack.querySelector(".gap-0\\.5")).toBeNull()
    fireEvent.pointerLeave(stack.parentElement!)
    expect(stack.textContent).toContain("43 shots")
  })

  it("says so when none of its shots has their face", () => {
    const { stack } = show(3, { more: { faceless: true } })
    expect(stack.textContent).toContain("No face")
    expect(stack.getAttribute("aria-label")).toBe("3 shots at E01 · 05:13, 6.6 s, without their face")
  })

  it("leaves the badges of a mixed stack to the shots in its band", () => {
    const c = card(3)
    c.shots[0].faceless = true
    render(<ShotStack card={c} media={media} open={false} onToggle={vi.fn()} onWatch={vi.fn()} />, { wrapper: TooltipProvider })
    expect(screen.getByRole("button", { expanded: false }).textContent).not.toContain("No face")
  })
})
