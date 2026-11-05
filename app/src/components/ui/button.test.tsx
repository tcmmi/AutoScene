import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { CaretDown, Trash } from "@/components/icons"
import { Button } from "./button"
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./dropdown-menu"

afterEach(cleanup)

const spinners = (el: Element) => el.querySelectorAll("svg.animate-spin").length

describe("Button busy", () => {
  it("takes the first icon's place, so spinner and icon never show together", () => {
    render(
      <Button busy>
        <Trash className="size-5" data-testid="trash" /> Uninstall
      </Button>,
    )
    const button = screen.getByRole("button", { name: "Uninstall" })
    expect(spinners(button)).toBe(1)
    expect(screen.queryByTestId("trash")).toBeNull()
    expect(button.querySelectorAll("svg").length).toBe(1)
    // The icon's own size goes to the spinner: the label stays where it was.
    expect(button.querySelector("svg")?.getAttribute("class")).toContain("size-5")
    expect(button.firstElementChild?.tagName.toLowerCase()).toBe("svg")
  })

  it("an icon after the label is not swapped: label and icon are hidden, one spinner stands over them", () => {
    render(
      <Button busy>
        Order <CaretDown data-testid="caret" />
      </Button>,
    )
    const button = screen.getByRole("button", { name: "Order" })
    expect(screen.getByTestId("caret").closest(".opacity-0")).not.toBeNull()
    expect(spinners(button)).toBe(1)
    expect(button.querySelector("svg.animate-spin")?.parentElement?.className).toContain("absolute inset-0")
  })

  it("only an icon is swapped: a span or a ring as first child survives", () => {
    render(
      <Button busy>
        <span data-testid="ring" /> Save
      </Button>,
    )
    const button = screen.getByRole("button", { name: "Save" })
    expect(screen.getByTestId("ring").closest(".opacity-0")).not.toBeNull()
    expect(button.querySelectorAll("svg").length).toBe(1)
    expect(spinners(button)).toBe(1)
  })

  it("a bare svg is an icon", () => {
    render(
      <Button busy>
        <svg data-testid="own" className="size-5" /> Save
      </Button>,
    )
    expect(screen.queryByTestId("own")).toBeNull()
    expect(spinners(screen.getByRole("button"))).toBe(1)
  })

  it("keeps the focus, stays lit, and Enter does nothing", async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    const { rerender } = render(<Button onClick={onClick}>Remove</Button>)
    const button = screen.getByRole("button", { name: "Remove" }) as HTMLButtonElement
    button.focus()
    rerender(
      <Button busy onClick={onClick}>
        Remove
      </Button>,
    )
    expect(document.activeElement).toBe(button)
    expect(button.disabled).toBe(false)
    expect(button.getAttribute("aria-disabled")).toBe("true")
    await user.keyboard("{Enter}")
    expect(onClick).not.toHaveBeenCalled()
  })

  it("skips blank text before the icon, the {' '} a line break leaves", () => {
    render(
      <Button busy>
        {" "}
        <Trash data-testid="trash" /> Uninstall
      </Button>,
    )
    expect(screen.queryByTestId("trash")).toBeNull()
  })

  it("without an icon, the label keeps the width and its name, out of sight under the spinner", () => {
    render(<Button busy>Remove</Button>)
    const button = screen.getByRole("button", { name: "Remove" })
    const label = screen.getByText("Remove")
    expect(label.className).toContain("opacity-0")
    expect(button.className).toContain("relative")
    const spinner = button.querySelector("svg.animate-spin")
    expect(spinner?.parentElement?.className).toContain("absolute inset-0")
  })

  it("a fragment counts as no icon: its contents stay whole", () => {
    render(
      <Button busy>
        <>
          <Trash data-testid="trash" /> Remove
        </>
      </Button>,
    )
    expect(screen.getByTestId("trash")).toBeTruthy()
    expect(spinners(screen.getByRole("button"))).toBe(1)
  })

  it("an icon-only button keeps its name", () => {
    render(
      <Button busy size="icon" aria-label="Remove">
        <Trash />
      </Button>,
    )
    const button = screen.getByRole("button", { name: "Remove" })
    expect(spinners(button)).toBe(1)
    expect(button.querySelectorAll("svg").length).toBe(1)
  })

  it("says so and takes no clicks; done, it is the button it was", async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    const { rerender } = render(
      <Button busy onClick={onClick}>
        <Trash /> Remove
      </Button>,
    )
    const button = screen.getByRole("button", { name: "Remove" })
    expect(button.getAttribute("aria-busy")).toBe("true")
    await user.click(button)
    expect(onClick).not.toHaveBeenCalled()

    rerender(
      <Button onClick={onClick}>
        <Trash /> Remove
      </Button>,
    )
    expect(button.hasAttribute("aria-busy")).toBe(false)
    expect(spinners(button)).toBe(0)
    await user.click(button)
    expect(onClick).toHaveBeenCalledOnce()
  })

  it("leaves a disabled button disabled when it ends", () => {
    render(
      <Button busy={false} disabled>
        Save
      </Button>,
    )
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true)
  })

  it("not busy, the children are untouched", () => {
    render(
      <Button>
        <Trash data-testid="trash" /> Remove
      </Button>,
    )
    const button = screen.getByRole("button", { name: "Remove" })
    expect(screen.getByTestId("trash").parentElement).toBe(button)
    expect(button.hasAttribute("aria-busy")).toBe(false)
    expect(button.className).not.toContain("relative")
  })

  it("a link is never busy", () => {
    const { container } = render(
      <Button asChild busy>
        <a href="#x">Open</a>
      </Button>,
    )
    const link = screen.getByRole("link", { name: "Open" })
    expect(link.parentElement).toBe(container)
    expect(link.className).toContain("rounded-full")
    expect(link.hasAttribute("aria-busy")).toBe(false)
    expect(link.hasAttribute("busy")).toBe(false)
    expect(spinners(link)).toBe(0)
  })
})

describe("Button open state", () => {
  const menu = (variant: "ghost" | "glass", size: "icon-sm" | "md") => (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant={variant} size={size} aria-label="Options">
          <Trash />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuItem>One</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )

  it("a ghost icon that opens a menu says so in its classes, and the menu sets the state", async () => {
    const user = userEvent.setup()
    render(menu("ghost", "icon-sm"))
    const trigger = screen.getByRole("button", { name: "Options" })
    expect(trigger.className).toContain("data-[state=open]:bg-hover")
    expect(trigger.className).toContain("data-[state=open]:text-fg-1")
    expect(trigger.getAttribute("data-state")).toBe("closed")
    await user.click(trigger)
    expect(trigger.getAttribute("data-state")).toBe("open")
  })

  it("is only for ghost icons: a labelled ghost or a glass icon looks as it did", () => {
    render(
      <>
        {menu("ghost", "md")}
        {menu("glass", "icon-sm")}
      </>,
    )
    for (const trigger of screen.getAllByRole("button")) expect(trigger.className).not.toContain("data-[state=open]")
  })
})
