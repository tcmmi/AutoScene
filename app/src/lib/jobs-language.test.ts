import { toast } from "sonner"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { JobInfo } from "@/bindings"
import { inLanguage } from "@/test/i18n"
import { cancelJob, cancelLabel, stopFailed, stopsFirst } from "./jobs"

const { jobCancel } = vi.hoisted(() => ({ jobCancel: vi.fn() }))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }))
vi.mock("./ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("./ipc")>()
  return { ...ipc, commands: { ...ipc.commands, jobCancel } }
})

const job = (kind: JobInfo["kind"]) => ({ id: "j1", kind }) as JobInfo

beforeEach(() => vi.clearAllMocks())

describe("stopping a job", () => {
  it("says in one sentence per kind that it couldn't: a plugin's run is stopped, other work cancelled", async () => {
    jobCancel.mockRejectedValue({ code: "internal", message: "Nobody answered." })
    cancelJob(job("plugin"))
    cancelJob(job("analyze"))
    await vi.waitFor(() => expect(toast.error).toHaveBeenCalledTimes(2))
    expect(vi.mocked(toast.error).mock.calls).toEqual([
      ["Couldn't stop the job", { description: "Nobody answered." }],
      ["Couldn't cancel the job", { description: "Nobody answered." }],
    ])
  })

  it("has each sentence whole, so another language can inflect it", async () => {
    // No lowercased fragment is spliced into a sentence: a language gives each kind its own words.
    await inLanguage("ro", {
      shell: { activity: { stopFailed: "Lucrarea nu a putut fi oprită", cancelFailed: "Lucrarea nu a putut fi anulată" } },
    })
    expect(stopFailed("plugin")).toBe("Lucrarea nu a putut fi oprită")
    expect(stopFailed("ingest")).toBe("Lucrarea nu a putut fi anulată")
    jobCancel.mockRejectedValue({ code: "internal", message: "Nimeni nu a răspuns." })
    cancelJob(job("export"))
    await vi.waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Lucrarea nu a putut fi anulată", { description: "Nimeni nu a răspuns." }),
    )
  })

  it("names the button by what the job does", async () => {
    expect(cancelLabel("ingest")).toBe("Cancel indexing")
    expect(cancelLabel("plugin")).toBe("Stop")
    await inLanguage("ro", { shell: { jobs: { cancelIndexing: "Anulează indexarea" } }, common: { action: { stop: "Oprește" } } })
    expect(cancelLabel("ingest")).toBe("Anulează indexarea")
    expect(cancelLabel("plugin")).toBe("Oprește")
  })

  it("lists the work that stops first in the language's own forms and list", async () => {
    await inLanguage("ro", {
      shell: {
        jobs: {
          stopsFirst: "Mai întâi se oprește: {{list}}.",
          stops: { indexing: "indexarea", export_one: "un export", export_few: "{{n}} exporturi", export_other: "{{n}} de exporturi" },
        },
      },
    })
    expect(stopsFirst(["ingest", "export"])).toBe("Mai întâi se oprește: indexarea și un export.")
    expect(stopsFirst(["export", "export", "export"])).toBe("Mai întâi se oprește: 3 exporturi.")
    expect(stopsFirst(Array.from({ length: 20 }, () => "export" as const))).toBe("Mai întâi se oprește: 20 de exporturi.")
  })
})
