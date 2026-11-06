import { useQuery } from "@tanstack/react-query"
import type { TFunction } from "i18next"
import { toast } from "sonner"
import { Trans, useTranslation } from "react-i18next"
import { ArrowSquareOut, FileText } from "@/components/icons"
import { AutoSceneMark } from "@/components/brand/AutoSceneMark"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { useAppActions } from "@/lib/actions"
import { formatBytes } from "@/lib/format"
import { say } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { openExternalUrl } from "@/lib/os"
import { useAppInfo, useHardware, useMediaTools } from "@/lib/queries"
import { SettingRow, SettingsGroup } from "../primitives"
import { PURPOSE } from "./model-purpose"

const PLATFORM_LABEL = { windows: "Windows", macos: "macOS", linux: "Linux" } as const

/** Third-party components shipped inside the app. */
const LICENSES: { name: string; license: string; url: string }[] = [
  { name: "Tauri", license: "MIT / Apache-2.0", url: "https://tauri.app" },
  { name: "React", license: "MIT", url: "https://react.dev" },
  { name: "Inter", license: "SIL Open Font License 1.1", url: "https://rsms.me/inter" },
  { name: "Instrument Serif", license: "SIL Open Font License 1.1", url: "https://github.com/Instrument/instrument-serif" },
  { name: "Geist Mono", license: "SIL Open Font License 1.1", url: "https://vercel.com/font" },
  { name: "Radix UI", license: "MIT", url: "https://www.radix-ui.com" },
  { name: "Phosphor Icons", license: "MIT", url: "https://phosphoricons.com" },
  { name: "uv", license: "MIT / Apache-2.0", url: "https://github.com/astral-sh/uv" },
]

/** What the footage picker's example stills are from (cropped and scaled). */
const STILLS: { name: string; by: string }[] = [
  { name: "Maze Runner", by: "20th Century Studios" },
  { name: "Stranger Things", by: "Netflix" },
  { name: "One Piece", by: "Netflix" },
  { name: "The Amazing Digital Circus", by: "Glitch Productions" },
  { name: "Arcane", by: "Riot Games" },
  { name: "Demon Slayer", by: "ufotable" },
  { name: "The Apothecary Diaries", by: "TOHO animation, OLM" },
  { name: "Chainsaw Man", by: "MAPPA" },
  { name: "Deadpool & Wolverine", by: "Marvel Studios" },
  { name: "Solo Leveling", by: "A-1 Pictures" },
]

/** The ONNX Runtime row: its version when known, and how it computes on this system (a whole label each). */
function onnxLabel(t: TFunction<"settings">, version: string | undefined, platform: "windows" | "macos" | "linux" | undefined): string {
  if (platform === "windows") {
    return version ? t("about.ai.onnx.directMlVersion", { version }) : t("about.ai.onnx.directMl")
  }
  if (platform === "macos") {
    return version ? t("about.ai.onnx.coreMlVersion", { version }) : t("about.ai.onnx.coreMl")
  }
  return version ? t("about.ai.onnx.plainVersion", { version }) : t("about.ai.onnx.plain")
}

/**
 * The AI models and their licences. Asked again each time About opens:
 * an analysis may have downloaded one since.
 */
function useModels() {
  return useQuery({ queryKey: ["models"], queryFn: () => commands.models(), staleTime: 0 })
}

export function AboutSection() {
  const { t } = useTranslation("settings")
  const { data: info } = useAppInfo()
  const { data: tools } = useMediaTools()
  const { data: models } = useModels()
  const { data: hardware } = useHardware()
  const a = useAppActions()
  return (
    <>
      <div className="relative mb-10 flex items-center gap-6 overflow-hidden rounded-3xl border border-white/[0.08] bg-[radial-gradient(120%_140%_at_0%_0%,rgb(224_180_92/0.16),transparent_55%),rgb(255_255_255/0.03)] p-7">
        <AutoSceneMark className="size-20 drop-shadow-[0_12px_32px_rgb(224_180_92/0.35)]" />
        <div className="min-w-0 flex-1">
          <div className="display text-4xl text-fg-1">AutoScene</div>
          <div className="mt-1 text-md text-fg-2">
            <Trans t={t} i18nKey="about.version" values={{ version: info?.version ?? "–" }} components={{ mono: <span className="timecode text-fg-1" /> }} />
            {info && (
              <span className="text-fg-3">
                {" "}
                · {PLATFORM_LABEL[info.platform]} {info.arch}
              </span>
            )}
          </div>
        </div>
        <Button size="sm" onClick={() => void a.openLogs()}>
          <FileText /> {t("about.openLogs")}
        </Button>
      </div>

      <SettingsGroup title={t("about.media.title")} description={t("about.media.description")}>
        <SettingRow
          // i18n-ignore: a product name and its version
          label={tools?.ready ? `FFmpeg ${tools.version}` : "FFmpeg"}
          description={
            tools ? (
              tools.ready ? (
                <span data-selectable className="font-mono">{tools.path}</span>
              ) : (
                <span className="text-danger">{tools.error && say(tools.error)}</span>
              )
            ) : (
              t("about.checking")
            )
          }
        >
          {/* i18n-ignore: licence names */}
          <span className="text-sm text-fg-3">LGPL / GPL</span>
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title={t("about.ai.title")} description={t("about.ai.description")}>
        <SettingRow label={onnxLabel(t, hardware?.runtime, info?.platform)}>
          {/* i18n-ignore: a licence name */}
          <span className="text-sm text-fg-3">MIT</span>
        </SettingRow>
        {hardware?.accel.installed && (
          <SettingRow // i18n-ignore: a product name and its version
            label={`NVIDIA TensorRT for RTX ${hardware.accel.engine}`}
            description={t("about.ai.nvidiaDescription")}
          >
            <Button size="sm" variant="ghost" onClick={() => void openExternalUrl(hardware.accel.licence)}>
              <ArrowSquareOut /> {t("about.ai.nvidiaLicence")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                commands.accelNotices().catch((e: unknown) => toast.error(t("about.ai.noticesFailed"), { description: errorMessage(e) }))
              }}
            >
              <FileText /> {t("about.ai.notices")}
            </Button>
          </SettingRow>
        )}
      </SettingsGroup>

      <SettingsGroup
        title={t("about.models.title")}
        description={t("about.models.description")}
      >
        {models?.map((m) => (
          <SettingRow
            key={m.id}
            label={Object.hasOwn(PURPOSE, m.id) ? t(PURPOSE[m.id as keyof typeof PURPOSE]) : m.purpose}
            description={
              <span data-selectable>
                {m.source}
                <span className="mt-1 flex flex-wrap items-center gap-x-2 text-fg-3">
                  <span>{m.installed ? t("about.models.downloaded") : t("about.models.notDownloaded")} · {formatBytes(m.sizeBytes ?? 0)}</span>
                  <span>·</span>
                  <span>{m.license}</span>
                </span>
              </span>
            }
            className="min-h-14 py-3"
          >
            {m.nonCommercial && <Badge tone="accent">{t("about.models.nonCommercial")}</Badge>}
          </SettingRow>
        )) ?? <SettingRow label={t("about.checking")} />}
      </SettingsGroup>

      <SettingsGroup title={t("about.licenses.title")} description={t("about.licenses.description")}>
        {LICENSES.map((l) => (
          <SettingRow key={l.name} label={l.name} description={l.url} className="min-h-12 py-2.5">
            <span className="text-sm text-fg-3">{l.license}</span>
          </SettingRow>
        ))}
      </SettingsGroup>

      <SettingsGroup
        title={t("about.stills.title")}
        description={t("about.stills.description")}
      >
        {STILLS.map((f) => (
          <SettingRow key={f.name} label={f.name} className="min-h-12 py-2.5">
            <span className="text-sm text-fg-3">{f.by}</span>
          </SettingRow>
        ))}
      </SettingsGroup>
    </>
  )
}
