import { toast } from "sonner"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { ProjectSummary } from "@/bindings"
import { describeActive } from "@/state/activity"
import { inLanguage } from "@/test/i18n"
import { copyText, type CopyKind } from "./os"
import { addedFiles, projectFacts } from "./projects"

const { clipboardWrite } = vi.hoisted(() => ({ clipboardWrite: vi.fn() }))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }))
vi.mock("./ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("./ipc")>()
  return { ...ipc, commands: { ...ipc.commands, clipboardWrite } }
})

beforeEach(() => vi.clearAllMocks())

// What the app said before its words moved to the catalog, word for word.
const KINDS: [CopyKind, string][] = [
  ["timecode", "timecode"],
  ["timecodes", "timecodes"],
  ["file path", "file path"],
  ["folder path", "folder path"],
  ["path", "path"],
  ["error", "error"],
  ["message", "message"],
  ["plugin id", "plugin id"],
]

describe("copying text, in English", () => {
  it.each(KINDS)("says %s as it always did", async (kind, noun) => {
    clipboardWrite.mockResolvedValueOnce(undefined)
    await copyText("00:01", kind)
    expect(toast).toHaveBeenCalledWith(`Copied ${noun}`, { description: "00:01" })
    clipboardWrite.mockRejectedValueOnce({ code: "internal", message: "No access." })
    await copyText("00:01", kind)
    expect(toast.error).toHaveBeenCalledWith(`Couldn't copy the ${noun}`, { description: "No access." })
  })

  it("copies quietly without a kind, and quietly for a kind outside the set", async () => {
    clipboardWrite.mockResolvedValue(undefined)
    await copyText("x")
    await copyText("x", "nonsense" as CopyKind)
    expect(toast).not.toHaveBeenCalled()
    clipboardWrite.mockRejectedValueOnce({ code: "internal", message: "No access." })
    await copyText("x", "nonsense" as CopyKind)
    expect(toast.error).toHaveBeenCalledWith("Couldn't copy", { description: "No access." })
  })
})

const project = (kind: "movie" | "series", mediaCount: number) =>
  ({ kind, mediaCount, content: "live" }) as unknown as ProjectSummary

describe("whole-message rewrites, in English", () => {
  it("a project's facts", () => {
    expect(projectFacts(project("series", 1))).toBe("Series · Live action · 1 episode")
    expect(projectFacts(project("series", 2))).toBe("Series · Live action · 2 episodes")
    expect(projectFacts(project("series", 1204))).toBe("Series · Live action · 1204 episodes")
    expect(projectFacts(project("movie", 1))).toBe("Movie · Live action · 1 file")
    expect(projectFacts(project("movie", 2))).toBe("Movie · Live action · 2 files")
  })

  it("what a toast says after files were added", () => {
    expect(addedFiles(0)).toBe("Nothing new to add")
    expect(addedFiles(0, "Those files are already in this project")).toBe("Those files are already in this project")
    expect(addedFiles(1)).toBe("Added 1 file")
    expect(addedFiles(2)).toBe("Added 2 files")
    expect(addedFiles(1204)).toBe("Added 1204 files")
  })

  it("what the Activity button tells assistive tech", () => {
    expect(describeActive(0, 0)).toBe("Nothing running")
    expect(describeActive(1, 0)).toBe("1 running")
    expect(describeActive(2, 3)).toBe("2 running, 3 waiting")
    expect(describeActive(0, 1)).toBe("1 waiting")
  })

  it("picks the plural form by count and writes the digits from n, in another language", async () => {
    // Romanian has a "few" form (2 to 19): count picks it, n stays the number as it is.
    await inLanguage("ro", { shell: { files: { added_one: "{{n}} fișier", added_few: "{{n}} fișiere", added_other: "{{n}} de fișiere" } } })
    expect(addedFiles(1)).toBe("1 fișier")
    expect(addedFiles(3)).toBe("3 fișiere")
    expect(addedFiles(100)).toBe("100 de fișiere")
  })
})
