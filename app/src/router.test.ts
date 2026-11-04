import { createMemoryHistory, createRouter } from "@tanstack/react-router"
import { describe, expect, it } from "vitest"
import { routeTree } from "./router"

function routerAt(path: string) {
  return createRouter({ routeTree, history: createMemoryHistory({ initialEntries: [path] }) })
}

/** Where the app ends up when it opens `path`. */
async function land(path: string) {
  const router = routerAt(path)
  await router.load()
  return router.state.location.pathname
}

describe("old links", () => {
  it("Settings' Plugins goes to Plugins, and Settings' own sections stay", async () => {
    expect(await land("/settings/plugins")).toBe("/plugins")
    expect(await land("/settings/general")).toBe("/settings/general")
    expect(await land("/settings/plugin.dev.a.b")).toBe("/settings/plugin.dev.a.b")
  })

  it("Back from where the old link led goes to the page before it, not to the link", async () => {
    const router = routerAt("/")
    await router.load()
    await router.navigate({ href: "/settings/plugins" })
    expect(router.state.location.pathname).toBe("/plugins")
    expect(router.history.length).toBe(2)
    router.history.back()
    await router.load()
    expect(router.state.location.pathname).toBe("/")
  })
})
