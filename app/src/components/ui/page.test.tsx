import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { AppMenu } from "@/components/shell/AppMenu"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { Page } from "./page"

beforeEach(() => {
  // The router puts each place at its top, and the view remembers where it was scrolled: jsdom scrolls neither.
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  Object.defineProperty(Element.prototype, "scrollTo", { configurable: true, value: () => {} })
})
afterEach(() => {
  cleanup()
  Reflect.deleteProperty(Element.prototype, "scrollTo")
})

describe("Page", () => {
  it("offers its menu on its empty space, above the full app list", async () => {
    const root = createRootRoute({
      component: () => (
        <AppMenu>
          <div>
            <Page menu={<ContextMenuItem>Add files…</ContextMenuItem>}>
              <p>Nothing here</p>
            </Page>
          </div>
        </AppMenu>
      ),
    })
    const here = createRoute({ getParentRoute: () => root, path: "/", component: () => null })
    const router = createRouter({ routeTree: root.addChildren([here]), history: createMemoryHistory({ initialEntries: ["/"] }) })
    render(<RouterProvider router={router} />)
    fireEvent.contextMenu(await screen.findByText("Nothing here"), { clientX: 40, clientY: 40 })
    // Each entry as it reads without its keys.
    const entries = within(await screen.findByRole("menu"))
      .getAllByRole("menuitem")
      .map((item) => {
        const label = item.cloneNode(true) as HTMLElement
        for (const keys of label.querySelectorAll(".timecode")) keys.remove()
        return label.textContent?.trim()
      })
    // The page's own entry first, then the app's list from its start.
    expect(entries.slice(0, 3)).toEqual(["Add files…", "Back", "Command palette…"])
  })
})
