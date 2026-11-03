import fs from "node:fs"
import http from "node:http"
import type { AddressInfo } from "node:net"
import os from "node:os"
import path from "node:path"
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import { GOLDEN_SESSION, type MockAsp, mockAspHandler, mockAspState } from "./vite.mock-asp.ts"

const golden = path.join(import.meta.dirname, "src", "plugin-kit", "page-csp.golden.txt")
const examples = path.join(import.meta.dirname, "..", "examples", "plugins")
const session = "fedcba9876543210fedcba9876543210"

let state: MockAsp
let server: http.Server
let base: string
let tmp: string

beforeAll(async () => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), "autoscene-mock-asp-"))
  fs.writeFileSync(path.join(tmp, "clip.mp4"), Buffer.alloc(3 << 20, 7))
  state = mockAspState({ examples, golden, kit: tmp, fixtures: tmp })
  state.sessions.set(session, "dev.autoscene.hello-page")
  state.files.set(`${session}/a1`, path.join(tmp, "clip.mp4"))
  server = http.createServer(mockAspHandler(state))
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve))
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
})

afterAll(async () => {
  await new Promise((resolve) => server.close(resolve))
  fs.rmSync(tmp, { recursive: true, force: true })
})

describe("the mock's plugin pages", () => {
  it("the mock sends the page CSP", async () => {
    // As Rust makes it for this origin (tests/pages.rs checks the file), for the page's own session.
    const csp = fs.readFileSync(golden, "utf8").trim().replaceAll(GOLDEN_SESSION, session)
    const page = await fetch(`${base}/${session}/ui/index.html`)
    expect(page.status).toBe(200)
    expect(page.headers.get("content-security-policy")).toBe(csp)
    expect(csp).toContain(`script-src http://localhost:5174/${session}/ http://localhost:5174/_as/;`)
    expect(csp.endsWith("frame-ancestors http://localhost:5173")).toBe(true)
    expect(page.headers.get("content-type")).toBe("text/html; charset=utf-8")
    expect(page.headers.get("x-content-type-options")).toBe("nosniff")
    expect(page.headers.get("access-control-allow-origin")).toBe("*")

    // A refusal of the session has it too; one outside a session allows nothing.
    const missing = await fetch(`${base}/${session}/ui/nothing.html`)
    expect([missing.status, missing.headers.get("content-security-policy")]).toEqual([404, csp])
    for (const outside of [`/${session}/plugin.json`, `/${session}/ui/../plugin.json`, `/${"0".repeat(32)}/ui/index.html`]) {
      expect((await fetch(`${base}${outside}`)).status, outside).toBe(404)
    }
    const nowhere = await fetch(`${base}/ui/index.html`)
    expect(nowhere.headers.get("content-security-policy")).toBe(
      "default-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors http://localhost:5173",
    )

    // A given file comes in parts of at most 1 MiB.
    const part = await fetch(`${base}/${session}/~f/a1`, { headers: { range: "bytes=0-" } })
    expect([part.status, part.headers.get("content-range")]).toEqual([206, `bytes 0-${(1 << 20) - 1}/${3 << 20}`])
    expect((await part.arrayBuffer()).byteLength).toBe(1 << 20)
  })
})
