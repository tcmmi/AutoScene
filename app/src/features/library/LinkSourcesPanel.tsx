import { useEffect, useId, useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import { CheckCircle, LinkSimple, Warning } from "@/components/icons"
import { toast } from "sonner"
import type { LinkItem, SettingValue } from "@/bindings"
import { Button } from "@/components/ui/button"
import { fmt, say } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { qk, queryClient, usePluginSettings } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { type SourceItem } from "@/features/plugins/plugin-items"
import { SettingControl } from "@/features/plugins/PluginSettingsSection"
import { valueOf } from "@/features/plugins/setting-value"

/** How long the panel waits after the last key before it reads the links. */
const READ_AFTER_MS = 250

/**
 * The field for links that plugins' sources download (New project, a
 * project's Add links…): paste links, see which plugin takes each and why one
 * can't be added, set what the sources show beside the links (a quality),
 * and add those that can be. The backend reads the links
 * (`source_links_read`); the page only shows what it said.
 */
export function LinkSourcesPanel({
  sources,
  initial = "",
  onAdd,
  onCancel,
  className,
}: {
  sources: SourceItem[]
  initial?: string
  onAdd: (items: LinkItem[]) => void
  onCancel?: () => void
  className?: string
}) {
  const { t } = useTranslation(["library", "common"])
  const fieldId = useId()
  const [text, setText] = useState(initial)
  const [read, setRead] = useState<{ text: string; items: LinkItem[] }>({ text: "", items: [] })
  const reading = text.trim() !== "" && read.text !== text

  useEffect(() => {
    if (text.trim() === "") return
    let stale = false
    const timer = window.setTimeout(() => {
      commands
        .sourceLinksRead(text)
        .then((items) => !stale && setRead({ text, items }))
        .catch((e) => {
          if (stale) return
          setRead({ text, items: [] })
          toast.error(t("links.readFailed"), { description: errorMessage(e) })
        })
    }, READ_AFTER_MS)
    return () => {
      stale = true
      window.clearTimeout(timer)
    }
  }, [text, t])

  const items = text.trim() === "" ? [] : read.items
  const usable = items.filter((i) => !i.refused && i.pluginId)
  const titles = [...new Set(sources.map((s) => s.source.title))]
  const plugins = [...new Set(sources.map((s) => s.pluginName))]

  return (
    <div className={cn("flex flex-col gap-3 p-4", className)}>
      <div className="flex flex-col gap-1.5">
        <label htmlFor={fieldId} className="text-sm font-medium text-fg-2">
          {t("links.label")}
        </label>
        <textarea
          id={fieldId}
          autoFocus
          rows={3}
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          value={text}
          placeholder={t("links.placeholder")}
          onChange={(e) => setText(e.target.value)}
          className={cn(
            "min-h-20 w-full resize-y rounded-lg border border-soft bg-white/[0.05] px-3 py-2 font-mono text-sm text-fg-1",
            "transition-[border-color,box-shadow,background-color] duration-[var(--dur-fast)] outline-none placeholder:font-sans placeholder:text-fg-3",
            "hover:border-strong focus-visible:border-accent-line focus-visible:bg-white/[0.07] focus-visible:ring-4 focus-visible:ring-accent-subtle",
          )}
        />
        <p className="text-sm text-fg-3">
          {t("links.takes", { sources: fmt.list(titles), plugins: fmt.list(plugins) })}
        </p>
      </div>

      {items.length > 0 && (
        <ul className="flex max-h-36 flex-col gap-1 overflow-y-auto" aria-live="polite">
          {items.map((i, n) => (
            <li key={`${n}:${i.text}`} className="flex items-start gap-2 text-sm">
              {i.refused ? (
                <Warning className="mt-0.5 size-3.5 shrink-0 text-danger" />
              ) : (
                <CheckCircle className="mt-0.5 size-3.5 shrink-0 text-ok" />
              )}
              <span className="min-w-0 flex-1">
                <span className={cn("block truncate", i.refused ? "text-fg-3" : "text-fg-2")}>{i.short || i.text}</span>
                {i.refused ? (
                  <span className="block text-danger">{say(i.refused)}</span>
                ) : (
                  <span className="block text-fg-3">
                    {t("links.takenBy", { source: i.sourceTitle ?? "", plugin: i.pluginName ?? "" })}
                  </span>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}

      {sources.some((s) => s.settings.length > 0) && (
        <div className="flex flex-col gap-2">
          {sources
            .filter((s) => s.settings.length > 0)
            .map((s) => (
              <SourceSettings key={s.key} source={s} />
            ))}
          <p className="text-sm text-fg-3">{t("links.settingsNote")}</p>
        </div>
      )}

      <div className="flex flex-col gap-0.5 text-sm text-fg-3">
        <p>{t("links.where")}</p>
        <p>{t("links.rights")}</p>
      </div>

      <div className="flex justify-end gap-2">
        {onCancel && (
          <Button size="sm" variant="ghost" onClick={onCancel}>
            {t("common:action.cancel")}
          </Button>
        )}
        <Button
          size="sm"
          variant="primary"
          busy={reading}
          disabled={reading || usable.length === 0}
          onClick={() => onAdd(usable)}
        >
          <LinkSimple /> {usable.length > 0 ? t("links.add", { count: usable.length }) : t("links.addNone")}
        </Button>
      </div>
    </div>
  )
}

/** The settings a source shows beside the links, saved as its plugin's own. */
function SourceSettings({ source }: { source: SourceItem }) {
  const { t } = useTranslation("plugins")
  const { data: values } = usePluginSettings(source.pluginId)
  const save = async (key: string, value: SettingValue) => {
    queryClient.setQueryData<Record<string, SettingValue>>(qk.pluginSettings(source.pluginId), (old) =>
      old ? { ...old, [key]: value } : old,
    )
    try {
      queryClient.setQueryData(
        qk.pluginSettings(source.pluginId),
        await commands.pluginSettingsSet(source.pluginId, { [key]: value }),
      )
    } catch (e) {
      toast.error(t("saveFailed"), { description: errorMessage(e) })
      void queryClient.invalidateQueries({ queryKey: qk.pluginSettings(source.pluginId) })
    }
  }
  const shown = useMemo(() => source.settings, [source.settings])
  return (
    <>
      {shown.map((spec) => (
        <div key={spec.key} className="flex flex-wrap items-center justify-between gap-3">
          <span className="text-sm font-medium text-fg-2">{spec.title}</span>
          <SettingControl
            spec={spec}
            value={valueOf(spec, values)}
            ready={values !== undefined}
            onSave={(v) => void save(spec.key, v)}
          />
        </div>
      ))}
    </>
  )
}
