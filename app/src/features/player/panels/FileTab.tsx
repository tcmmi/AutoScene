import { useTranslation } from "react-i18next"
import { Copy, FolderOpen } from "@/components/icons"
import { toast } from "sonner"
import type { MediaItem, Rational } from "@/bindings"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
  ContextMenuSeparator,
} from "@/components/ui/context-menu"
import { Select } from "@/components/ui/select"
import { commands, errorMessage } from "@/lib/ipc"
import { audioOption, codecLabel, currentAudio, formatFlags, releaseTitle } from "@/lib/media"
import { copyText, reveal } from "@/lib/os"
import { formatFps } from "@/lib/timecode"
import { cn } from "@/lib/utils"
import { TabBody } from "./TabBody"

/**
 * The File tab: the file's name, picture, length, flags, audio track and
 * folder. `frames` is the episode's frame count (the transport's counter
 * shows frame numbers, which count from 0).
 */
export function FileTab({
  pid,
  media: m,
  fps,
  frames,
  tc,
}: {
  pid: string
  media: MediaItem
  fps: Rational
  frames: number
  tc: (f: number) => string
}) {
  const { t } = useTranslation("player")
  const audio = m.info?.audio ?? []
  const flags = formatFlags(m)
  const setAudio = (v: string) =>
    void commands
      .mediaSetAudio(pid, m.id, Number(v))
      .catch((e) => toast.error(t("file.audioFailed"), { description: errorMessage(e) }))
  const menu = (
    <>
      <ContextMenuItem onSelect={() => reveal(m.path)}>
        <FolderOpen /> {t("file.showInFolder")}
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => void copyText(m.path, "file path")}>
        <Copy /> {t("file.copyPath")}
      </ContextMenuItem>
      {audio.length > 1 && (
        <>
          <ContextMenuSeparator />
          <ContextMenuLabel>{t("file.audioTrack")}</ContextMenuLabel>
          <ContextMenuRadioGroup value={String(currentAudio(m))} onValueChange={setAudio}>
            {audio.map(audioOption).map((o) => (
              <ContextMenuRadioItem key={o.value} value={o.value}>
                <span className="truncate">
                  {o.label}
                  {o.description && <span className="text-fg-3"> · {o.description}</span>}
                </span>
              </ContextMenuRadioItem>
            ))}
          </ContextMenuRadioGroup>
        </>
      )}
    </>
  )
  return (
    <TabBody label={t("details.file")} menu={menu}>
      <p className="text-md font-medium text-fg-1">{releaseTitle(m.fileName)}</p>
      <p data-selectable className="mt-0.5 text-xs [overflow-wrap:anywhere] text-fg-3">
        {m.fileName}
      </p>
      {m.info && (
        <p className="timecode mt-2 text-xs text-fg-2">
          {t("file.video", {
            width: m.info.video.width,
            height: m.info.video.height,
            fps: formatFps(fps),
            codec: codecLabel(m.info.video.codec),
          })}
        </p>
      )}
      <p className={cn("timecode text-xs text-fg-2", m.info ? "mt-0.5" : "mt-2")}>
        {t("file.length", { time: tc(frames), count: frames })}
      </p>
      {flags.length > 0 && (
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {flags.map((f) => (
            <Badge key={f} tone="outline" size="sm">
              {f}
            </Badge>
          ))}
        </div>
      )}
      {audio.length > 1 && (
        <div className="mt-3">
          <Select
            size="sm"
            aria-label={t("file.audioTrack")}
            className="w-full"
            value={String(currentAudio(m))}
            onChange={setAudio}
            options={audio.map(audioOption)}
          />
        </div>
      )}
      <Button size="sm" variant="ghost" className="mt-3 -ml-2" onClick={() => reveal(m.path)}>
        <FolderOpen /> {t("file.showInFolder")}
      </Button>
    </TabBody>
  )
}
