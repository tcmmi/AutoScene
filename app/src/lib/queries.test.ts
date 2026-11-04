import { createElement, type ReactNode } from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"
import { pluginInfo } from "@/test/plugins"
import { commands } from "./ipc"
import { firstFailure, qk, queryClient, usePluginBanner, usePlugins } from "./queries"

vi.mock("./ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./ipc")>()),
  commands: { pluginsList: vi.fn(), pluginBanner: vi.fn() },
}))

const ok = () => ({ isError: false, error: null, refetch: vi.fn() })
const failed = (error: unknown) => ({ isError: true, error, refetch: vi.fn() })

describe("firstFailure", () => {
  it("is nothing while every query is fine (or still loading)", () => {
    expect(firstFailure(ok(), ok())).toBeNull()
  })

  it("names the first failure and fetches only the failed queries again", () => {
    const [a, b, c] = [ok(), failed("disk"), failed("db")]
    const f = firstFailure(a, b, c)!
    expect(f.error).toBe("disk")
    f.retry()
    expect(a.refetch).not.toHaveBeenCalled()
    expect(b.refetch).toHaveBeenCalledOnce()
    expect(c.refetch).toHaveBeenCalledOnce()
  })

  it("keeps what a query showed when only its refetch failed", () => {
    expect(firstFailure(ok(), { ...failed("offline"), data: [] })).toBeNull()
  })
})

describe("usePluginBanner", () => {
  const art = "data:image/jpeg;base64,/9j/4AAQ"
  const wrapper = ({ children }: { children: ReactNode }) => createElement(QueryClientProvider, { client: queryClient }, children)

  it("a banner is fetched once per hash", async () => {
    queryClient.clear()
    let hash = "a1"
    vi.mocked(commands.pluginsList).mockImplementation(() =>
      Promise.resolve([pluginInfo({ id: "dev.a.one", name: "One", bannerHash: hash })]),
    )
    vi.mocked(commands.pluginBanner).mockResolvedValue(art)
    const { result } = renderHook(() => usePluginBanner(usePlugins().data?.[0] ?? { id: "", bannerHash: null }), { wrapper })
    await waitFor(() => expect(result.current.data).toBe(art))
    // plugins:changed, as a settings save sends it, twice: the list again, the banner not.
    await queryClient.invalidateQueries({ queryKey: qk.plugins })
    await queryClient.invalidateQueries({ queryKey: qk.plugins })
    expect(commands.pluginsList).toHaveBeenCalledTimes(3)
    expect(commands.pluginBanner).toHaveBeenCalledExactlyOnceWith("dev.a.one")
    // A new picture (an update, or a changed folder file) comes with a new hash.
    hash = "b2"
    await queryClient.invalidateQueries({ queryKey: qk.plugins })
    await waitFor(() => expect(commands.pluginBanner).toHaveBeenCalledTimes(2))
  })

  it("asks for none without a hash, and shows only a picture in hand", async () => {
    queryClient.clear()
    vi.mocked(commands.pluginBanner).mockReset()
    const none = renderHook(() => usePluginBanner({ id: "dev.a.none", bannerHash: null }), { wrapper })
    expect(none.result.current.data).toBeUndefined()
    vi.mocked(commands.pluginBanner).mockResolvedValue("https://github.com/someone.png")
    const { result } = renderHook(() => usePluginBanner({ id: "dev.a.link", bannerHash: "c3" }), { wrapper })
    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    expect(result.current.data).toBeNull()
    expect(commands.pluginBanner).toHaveBeenCalledExactlyOnceWith("dev.a.link")
    // A data: URL too, unless it is a base64 PNG, JPEG or WebP: no SVG, no page, nothing unencoded.
    for (const [hash, url] of [
      ["d4", "data:image/svg+xml;base64,PHN2Zy8+"],
      ["e5", "data:text/html;base64,PGgxPg=="],
      ["f6", "data:image/png,x"],
    ]) {
      vi.mocked(commands.pluginBanner).mockResolvedValue(url)
      const { result } = renderHook(() => usePluginBanner({ id: "dev.a.data", bannerHash: hash }), { wrapper })
      await waitFor(() => expect(result.current.isSuccess).toBe(true))
      expect(result.current.data, url).toBeNull()
    }
  })
})
