import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { UserCircleDashed } from "@/components/icons"
import { CARD_GRID } from "@/lib/grid"
import { Avatar } from "./avatar"
import { Badge } from "./badge"
import { CardMenuButton } from "./card-menu-button"
import { CardPlayButton } from "./card-play-button"
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./dropdown-menu"
import { Skeleton } from "./skeleton"
import { ToggleChip } from "./toggle-chip"

afterEach(cleanup)

const classes = (el: Element | null | undefined) => [...(el?.classList ?? [])].sort()
const list = (s: string) => s.split(" ").sort()

describe("Skeleton", () => {
  it("is the app's shimmer, in the shape it is given", () => {
    const { container } = render(<Skeleton className="aspect-video rounded-2xl" data-testid="s" />)
    expect(classes(container.firstElementChild)).toEqual(list("skeleton aspect-video rounded-2xl"))
    expect(screen.getByTestId("s")).toBeTruthy()
  })

  it("rounds a little without a shape; shimmer={false} stands still", () => {
    const { container } = render(<Skeleton shimmer={false} />)
    expect(classes(container.firstElementChild)).toEqual(list("rounded-lg bg-hover"))
  })
})

describe("Badge", () => {
  it("is the size it always was without one", () => {
    const { container } = render(<Badge tone="outline">HDR</Badge>)
    expect(classes(container.firstElementChild)).toEqual(
      list("inline-flex h-6 items-center gap-1.5 whitespace-nowrap rounded-full px-2.5 text-xs font-medium [&_svg]:size-3.5 border border-soft text-fg-2"),
    )
  })

  it('size="sm" is what FileTab and EpisodesPage write by hand', () => {
    const { container } = render(
      <Badge tone="outline" size="sm">
        HDR
      </Badge>,
    )
    const box = classes(container.firstElementChild)
    for (const c of ["h-5", "px-2", "text-2xs"]) expect(box).toContain(c)
    for (const c of ["h-6", "px-2.5", "text-xs"]) expect(box).not.toContain(c)
  })

  it('tone="picture" size="sm" is the chip on a card\'s picture, 20 px tall at 11 px', () => {
    const { container } = render(
      <Badge tone="picture" size="sm" className="timecode absolute right-2 bottom-2">
        0:12
      </Badge>,
    )
    expect(classes(container.firstElementChild)).toEqual(
      list(
        "inline-flex items-center whitespace-nowrap rounded-full font-normal h-5 gap-1 px-2 text-2xs [&_svg]:size-3 bg-black/65 text-white/90 backdrop-blur-md timecode absolute right-2 bottom-2",
      ),
    )
  })

  it("a chip of words can ask for its weight back, with its icon at 12 px", () => {
    const { container } = render(
      <Badge tone="picture" size="sm" className="font-medium">
        <UserCircleDashed weight="bold" /> No face
      </Badge>,
    )
    const box = classes(container.firstElementChild)
    expect(box).toContain("font-medium")
    expect(box).not.toContain("font-normal")
  })
})

describe("ToggleChip", () => {
  it("says whether it is pressed, and looks it", async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    const { rerender } = render(
      <ToggleChip pressed={false} onClick={onClick}>
        Wide
      </ToggleChip>,
    )
    const chip = screen.getByRole("button", { name: "Wide" })
    expect(chip.getAttribute("aria-pressed")).toBe("false")
    expect(chip.className).toContain("bg-hover")
    await user.click(chip)
    expect(onClick).toHaveBeenCalledOnce()
    rerender(<ToggleChip pressed>Wide</ToggleChip>)
    expect(chip.getAttribute("aria-pressed")).toBe("true")
    expect(chip.className).toContain("bg-accent-subtle")
  })

  it("can open a menu in place of being pressed", async () => {
    const user = userEvent.setup()
    render(
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ToggleChip pressed aria-haspopup="menu" aria-pressed={undefined}>
            Filters
          </ToggleChip>
        </DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuItem>One</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>,
    )
    const chip = screen.getByRole("button", { name: "Filters" })
    expect(chip.hasAttribute("aria-pressed")).toBe(false)
    expect(chip.className).toContain("bg-accent-subtle")
    await user.click(chip)
    expect(screen.getByRole("menuitem", { name: "One" })).toBeTruthy()
  })
})

describe("CardMenuButton", () => {
  it("opens the menu it sits in, and stays up while it is open", async () => {
    const user = userEvent.setup()
    render(
      <DropdownMenu>
        <CardMenuButton aria-label="Dune options" />
        <DropdownMenuContent>
          <DropdownMenuItem>Rename</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>,
    )
    const trigger = screen.getByRole("button", { name: "Dune options" })
    expect(trigger.className).toContain("data-[state=open]:opacity-100")
    await user.click(trigger)
    expect(trigger.getAttribute("data-state")).toBe("open")
    expect(screen.getByRole("menuitem", { name: "Rename" })).toBeTruthy()
  })

  it("shows over either kind of card, and to the keyboard", () => {
    render(
      <DropdownMenu>
        <CardMenuButton aria-label="Dune options" />
      </DropdownMenu>,
    )
    const box = classes(screen.getByRole("button", { name: "Dune options" }))
    for (const c of ["opacity-0", "group-hover:opacity-100", "focus-visible:opacity-100"]) expect(box).toContain(c)
    expect(box).not.toContain("group-hover/art")
  })

  it("over the picture, only a group/art shows it, not any group around", () => {
    render(
      <DropdownMenu>
        <CardMenuButton aria-label="Dune options" over="picture" />
      </DropdownMenu>,
    )
    const box = classes(screen.getByRole("button", { name: "Dune options" }))
    expect(box).toContain("group-hover/art:opacity-100")
    expect(box).not.toContain("group-hover:opacity-100")
  })
})

describe("CardPlayButton", () => {
  it("is a plain button with its name, out of sight until hover or focus", async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    render(<CardPlayButton aria-label="Watch from 0:12" onClick={onClick} />)
    const button = screen.getByRole("button", { name: "Watch from 0:12" })
    expect(button.getAttribute("type")).toBe("button")
    const box = classes(button)
    for (const c of ["opacity-0", "group-hover:opacity-100", "focus-visible:opacity-100", "size-7"]) expect(box).toContain(c)
    await user.click(button)
    expect(onClick).toHaveBeenCalledOnce()
  })
})

describe("Avatar", () => {
  it("is an empty circle without a picture, and draws one with it", () => {
    const { container, rerender } = render(<Avatar />)
    expect(container.querySelector("img")).toBeNull()
    expect(classes(container.firstElementChild)).toEqual(list("size-8 shrink-0 overflow-hidden rounded-full bg-inset"))
    rerender(<Avatar path={null} />)
    expect(container.querySelector("img")).toBeNull()
    rerender(<Avatar path="/faces/walt.webp" />)
    const img = container.querySelector("img")
    expect(img?.getAttribute("src")).toBe("/faces/walt.webp")
    expect(img?.getAttribute("alt")).toBe("")
  })

  it("className sets its size and backdrop; ring is the gold of who is in the shot", () => {
    const { container } = render(<Avatar ring className="size-14 bg-raised picture-edge" />)
    expect(classes(container.firstElementChild)).toEqual(
      list("size-14 shrink-0 overflow-hidden rounded-full bg-raised picture-edge ring-2 ring-accent/70"),
    )
  })
})

describe("CARD_GRID", () => {
  it("is the string LibraryPage and the plugin views each carry today", () => {
    expect(CARD_GRID).toBe("grid grid-cols-[repeat(auto-fill,minmax(300px,1fr))] gap-x-6 gap-y-8")
  })
})
