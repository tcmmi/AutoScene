import { createMemoryHistory, createRouter } from "@tanstack/react-router"
import { describe, expect, it } from "vitest"
import { routeTree } from "./router"

function routerAt(path: string) {
  return createRouter({ routeTree, history: createMemoryHistory({ initialEntries: [path] }) })
}

/** Where the app ends up when it opens `path`, and what it shows there. */
async function land(path: string) {
  const router = routerAt(path)
  await router.load()
  const last = router.state.matches.at(-1)!
  return { path: router.state.location.pathname, route: last.routeId, params: last.params as Record<string, string> }
}

// Plugins and each plugin's settings were in Settings: an old link, from a
// plugin's README or a bookmark, still finds them.
describe("old links", () => {
  it("/settings/plugins lands on /plugins", async () => {
    expect(await land("/settings/plugins")).toMatchObject({ path: "/plugins", route: "/plugins" })
  })

  it("/settings/plugin.dev.a.b lands on /plugins/dev.a.b", async () => {
    expect(await land("/settings/plugin.dev.a.b")).toMatchObject({
      path: "/plugins/dev.a.b",
      route: "/plugins/$id",
      params: { id: "dev.a.b" },
    })
  })

  it("/settings/nonsense shows General", async () => {
    expect(await land("/settings/nonsense")).toMatchObject({ route: "/settings/$section", params: { section: "general" } })
    // Settings' own sections stay where they are.
    expect(await land("/settings/shortcuts")).toMatchObject({ path: "/settings/shortcuts", params: { section: "shortcuts" } })
  })

  it("Back from where the old link led goes to the page before it, not to the link", async () => {
    const router = routerAt("/")
    await router.load()
    await router.navigate({ href: "/settings/plugin.dev.a.b" })
    expect(router.state.location.pathname).toBe("/plugins/dev.a.b")
    expect(router.history.length).toBe(2)
    router.history.back()
    await router.load()
    expect(router.state.location.pathname).toBe("/")
  })
})
