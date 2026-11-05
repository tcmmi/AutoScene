import { describe, expect, it } from "vitest"
import conf from "../../../src-tauri/tauri.conf.json"
import page from "../../../index.html?raw"

/**
 * The style sources a built app gets. Tauri gives each <style> of the page
 * a nonce and adds it to style-src, unless told not to, and a browser
 * ignores 'unsafe-inline' beside a nonce: the policy in the config is then
 * not the one that holds. A dev build embeds no page, so it can't show this.
 */
function shippedStyleSources() {
  const { csp, dangerousDisableAssetCspModification: untouched } = conf.app.security as {
    csp: string
    dangerousDisableAssetCspModification?: boolean | string[]
  }
  const directives = csp.split(";").map((d) => d.trim().split(" "))
  const sources = directives.find(([name]) => name === "style-src")!.slice(1)
  const kept = untouched === true || (Array.isArray(untouched) && untouched.includes("style-src"))
  return !kept && /<style[\s>]/.test(page) ? [...sources.filter((s) => s !== "'unsafe-inline'"), "'nonce-…'"] : sources
}

describe("toasts in a built app", () => {
  // Sonner puts its rules (where a toast goes) into a <style> when it loads,
  // and Radix's dialogs do the same for their scroll lock. Blocked, every
  // toast lands below the window, unseen.
  it("the page may add its own styles", () => {
    expect(shippedStyleSources()).toContain("'unsafe-inline'")
  })
})
