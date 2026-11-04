import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet } from "@tanstack/react-router"
import { cleanup, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { qk, queryClient } from "@/lib/queries"
import { pluginInfo } from "@/test/plugins"
import { SettingsPage } from "./SettingsPage"

// What each section shows is its own test's.
vi.mock("./sections/GeneralSection", () => ({ GeneralSection: () => <p>General's settings</p> }))
vi.mock("./sections/ShortcutsSection", () => ({ ShortcutsSection: () => null }))
vi.mock("./sections/UpdatesSection", () => ({ UpdatesSection: () => null }))
vi.mock("./sections/AboutSection", () => ({ AboutSection: () => null }))
vi.mock("./sections/ContributorsSection", () => ({ ContributorsSection: () => null }))

function show(path: string) {
  const root = createRootRoute({ component: Outlet })
  const settings = createRoute({ getParentRoute: () => root, path: "/settings/$section", component: SettingsPage })
  const router = createRouter({ routeTree: root.addChildren([settings]), history: createMemoryHistory({ initialEntries: [path] }) })
  render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  queryClient.clear()
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
})
afterEach(cleanup)

describe("Settings", () => {
  // A plugin's settings are on its own screen under Plugins.
  it("Settings lists no plugins", async () => {
    const setting = { key: "loud", title: "Loud", description: null, kind: { type: "boolean", default: false } } as const
    queryClient.setQueryData(qk.plugins, [pluginInfo({ id: "dev.a.hello", name: "Hello", settings: [setting] })])
    show("/settings/general")
    expect(await screen.findByText("General's settings")).toBeTruthy()
    const nav = screen.getByRole("navigation", { name: "Settings" })
    expect(within(nav).getAllByRole("link").map((l) => l.textContent)).toEqual([
      "General",
      "Discord",
      "Shortcuts",
      "Updates",
      "About",
      "Contributors",
    ])
    expect(screen.queryByText("Hello")).toBeNull()
    expect(screen.queryByText(/Plugin/)).toBeNull()
  })
})
