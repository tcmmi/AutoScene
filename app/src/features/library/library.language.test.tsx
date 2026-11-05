import type * as React from "react"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { CharacterShot, GraphicsClass, ImportCandidate, MediaItem, ProjectSummary, ScanDefaults } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { stackCards } from "@/features/shots/groups"
import { ShotStack } from "@/features/shots/ShotStack"
import { fastScanHint, newProjectHint, NOTICE_FACTS, noticeFoot, noticeLead, settingsLine } from "@/lib/fast-scan"
import { CONTENT_LABEL, contentLabel, footageBlurb } from "@/lib/footage"
import { PSEUDO } from "@/lib/i18n"
import { useNewProject } from "@/state/new-project"
import { inLanguage } from "@/test/i18n"
import { FootagePicker } from "./FootagePicker"
import { NewProjectDialog } from "./NewProjectDialog"
import { ProjectDetailsDialog } from "./ProjectDialogs"

const candidate = (n: number): ImportCandidate => ({
  path: `D:\\Show\\Show - 0${n}.mkv`,
  fileName: `Show - 0${n}.mkv`,
  season: null,
  episode: n,
  label: `E0${n}`,
  sizeBytes: 1e9,
  duplicate: false,
  refused: null,
})

function defaults(cls: GraphicsClass): ScanDefaults {
  return {
    computer: { class: cls, name: "" },
    choice: "auto",
    footage: (["live", "mixed", "animation", "anime"] as const).map((content) => ({
      content,
      fast: cls === "integrated" && content !== "animation",
    })),
    notice: false,
  }
}

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => vi.fn(), useRouterState: () => false }))
vi.mock("@/lib/ipc", () => ({
  commands: {
    importScan: vi.fn(() => Promise.resolve([candidate(1), candidate(2), candidate(3)])),
    settingsGet: vi.fn(() => Promise.resolve({ findBodies: false, fastScan: false })),
    scanDefaultsGet: vi.fn(() => Promise.resolve(defaults("integrated"))),
  },
  errorMessage: (e: unknown) => String(e),
}))

let queryClient: QueryClient
const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)

beforeEach(() => {
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
})
afterEach(() => {
  cleanup()
  useNewProject.setState({ open: false, seed: [] })
})

const ro = {
  common: { action: { cancel: "Anulează" } },
  library: {
    newProject: {
      title: "Proiect nou",
      fastScan: "Scanare rapidă",
      create: "Creează proiectul",
      videos_one: "{{count}} videoclip",
      videos_few: "{{count}} videoclipuri",
      videos_other: "{{count}} de videoclipuri",
      includeFile: "Include {{name}}",
      skipFile: "Omite {{name}}",
    },
    footage: {
      label: "Material",
      live: { label: "Acțiune live", blurb: "Filmat cu actori adevărați." },
      stills: "Cadre: {{credit}}",
    },
    details: { title: "Detaliile proiectului" },
  },
}

describe("the New project dialog in another language", () => {
  it("counts the videos with the language's plural form, and the file buttons name the file", async () => {
    useNewProject.getState().show(["D:\\Show"])
    await inLanguage("ro", ro)
    render(<NewProjectDialog />, { wrapper })
    expect(await screen.findByText("3 videoclipuri · 2,8 GB", { exact: false })).toBeTruthy()
    expect(screen.getByText("Proiect nou")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Creează proiectul" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Anulează" })).toBeTruthy()
    expect(screen.getByRole("switch", { name: "Scanare rapidă" })).toBeTruthy()
    expect(screen.getAllByRole("button", { name: /^Omite Show - 0\d\.mkv$/ })).toHaveLength(3)
  })

  it("shows nothing plain in the pseudo-locale: every word the dialog owns is a catalog word", async () => {
    useNewProject.getState().show(["D:\\Show"])
    await inLanguage(PSEUDO)
    render(<NewProjectDialog />, { wrapper })
    await screen.findByText(/^\[3 /)
    const dialog = screen.getByRole("dialog")
    const plain = [...dialog.querySelectorAll("label, button, h2, p, span")]
      .filter((el) => el.children.length === 0 && /[A-Za-z]{3,}/.test(el.textContent ?? ""))
      .map((el) => el.textContent ?? "")
      // The person's own data: the files' names, the sizes, the labels of the shots.
      // And the dialog's own close button, which is the shared dialog's word (components/ui).
      .filter((text) => !/^Show - 0\d\.mkv$|^E0\d$|GB|^\[|^Close$/.test(text))
    expect(plain).toEqual([])
  })
})

describe("what the footage can be", () => {
  it("names the footage in the language in use, in the picker and where other screens read the label", async () => {
    expect([contentLabel("live"), CONTENT_LABEL.animation, CONTENT_LABEL.anime, CONTENT_LABEL.mixed]).toEqual([
      "Live action",
      "Animation",
      "Anime",
      "Mixed",
    ])
    await inLanguage("ro", ro)
    expect(CONTENT_LABEL.live).toBe("Acțiune live")
    expect(footageBlurb("live")).toBe("Filmat cu actori adevărați.")
    render(<FootagePicker value="live" onChange={() => {}} />)
    expect(screen.getByLabelText("Material")).toBeTruthy()
    expect(screen.getAllByRole("radio")[0].textContent).toBe("Acțiune live")
  })
})

describe("Fast Scan's sentences", () => {
  it("are said again when the language changes, whole, never a word put into one", async () => {
    const d = defaults("integrated")
    expect(settingsLine(d, "macos")).toBe(
      "Here, Automatic means Fast Scan: this Mac analyses video on its built-in graphics. 3D animation keeps the full scan.",
    )
    expect(NOTICE_FACTS.map((f) => f.title)).toEqual(["Much faster", "Less sure who is who"])
    await inLanguage("ro", {
      library: {
        fastScan: {
          settings: {
            integratedMac: "Aici, Automat înseamnă Scanare rapidă: acest Mac analizează video pe grafica integrată.",
            keeps: "Animația 3D păstrează scanarea completă.",
          },
          suits: { integrated: "Se potrivește graficii integrate." },
          cost: { live: "Mult mai rapid, găsește mai puține fețe." },
          hint: { full: "Fețele se găsesc complet." },
          fastScanSuits: { integrated: "Scanarea rapidă se potrivește graficii integrate." },
          lead: {
            integratedMac: "Acest Mac analizează video pe grafica integrată, așa că proiectele noi încep cu Scanare rapidă.",
          },
          notice: {
            faster: { title: "Mult mai rapid" },
            change: "Poți schimba oricând implicitul din Setări.",
            keeps: "Animația 3D păstrează scanarea completă.",
          },
        },
      },
    })
    expect(settingsLine(d, "macos")).toBe(
      "Aici, Automat înseamnă Scanare rapidă: acest Mac analizează video pe grafica integrată. Animația 3D păstrează scanarea completă.",
    )
    expect(newProjectHint("live", false, d)).toBe("Mult mai rapid, găsește mai puține fețe. Se potrivește graficii integrate.")
    expect(fastScanHint("live", false, false, d)).toBe(
      "Fețele se găsesc complet. Scanarea rapidă se potrivește graficii integrate.",
    )
    expect(noticeLead({ class: "integrated", name: "" }, "macos")).toBe(
      "Acest Mac analizează video pe grafica integrată, așa că proiectele noi încep cu Scanare rapidă.",
    )
    expect(NOTICE_FACTS[0].title).toBe("Mult mai rapid")
    expect(NOTICE_FACTS[1].title).toBe("Less sure who is who")
    expect(noticeFoot(d)).toBe("Animația 3D păstrează scanarea completă. Poți schimba oricând implicitul din Setări.")
  })

  it("has a sentence for every graphics class and platform", () => {
    for (const cls of ["forAi", "notForAi", "integrated", "processor", "unknown"] as const) {
      for (const platform of ["windows", "macos", "linux"] as const) {
        const line = settingsLine(defaults(cls), platform)
        expect(line, `${cls} ${platform}`).toMatch(/^Here, Automatic means (Fast Scan|the full scan): /)
        expect(line).not.toMatch(/library:|undefined|\[object/)
      }
    }
  })
})

describe("Edit details in another language", () => {
  it("says its title and leaves the project's own title as it is", async () => {
    const project = { id: "p", title: "Breaking Bad", kind: "series", content: "live", fastScan: false } as ProjectSummary
    await inLanguage("ro", ro)
    render(<ProjectDetailsDialog project={project} onOpenChange={() => {}} />, { wrapper })
    expect(await screen.findByText("Detaliile proiectului")).toBeTruthy()
    expect((screen.getByDisplayValue("Breaking Bad") as HTMLInputElement).value).toBe("Breaking Bad")
  })
})

describe("a stack of shots in another language", () => {
  const media = { id: "e1", label: "E01", fps: { num: 24, den: 1 } } as MediaItem
  const shots = (n: number): CharacterShot[] =>
    Array.from({ length: n }, (_, i) => ({
      mediaId: "e1",
      start: 7512 + i * 60,
      end: 7512 + i * 60 + 53,
      sprite: `s${i}.jpg`,
      cells: 4,
      trackId: i + 1,
      face: `f${i}.jpg`,
      faceHeight: 0.2,
    })) as CharacterShot[]
  const stack = (n: number) =>
    render(
      <ShotStack card={stackCards([{ shots: shots(n) }])[0]} media={media} open={false} onToggle={vi.fn()} onWatch={vi.fn()} />,
      { wrapper: TooltipProvider },
    )

  it("names itself with the language's plural form, whole, with the length after it", async () => {
    await inLanguage("ro", {
      library: {
        shots: {
          count_one: "{{count}} cadru",
          count_few: "{{count}} cadre",
          count_other: "{{count}} de cadre",
          countAt_one: "{{count}} cadru la {{where}}",
          countAt_few: "{{count}} cadre la {{where}}",
          countAt_other: "{{count}} de cadre la {{where}}",
          withoutFace: "fără fața lor",
          watchFromWhere: "Privește de la {{where}}",
        },
      },
    })
    const view = stack(3)
    expect(screen.getByRole("button", { expanded: false }).getAttribute("aria-label")).toBe("3 cadre la E01 · 05:13, 6.6 s")
    expect(screen.getByRole("button", { name: "Privește de la E01 · 05:13" })).toBeTruthy()
    await act(async () => view.unmount())
    stack(20)
    expect(screen.getByRole("button", { expanded: false }).getAttribute("aria-label")).toContain("20 de cadre la E01")
  })
})
