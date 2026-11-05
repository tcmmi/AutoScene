import { toast } from "sonner"
import { useTranslation } from "react-i18next"
import type { AppSettings, DiscordLines, DiscordRpcSettings } from "@/bindings"
import { AutoSceneMark } from "@/components/brand/AutoSceneMark"
import { Switch } from "@/components/ui/switch"
import { t } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { qk, queryClient, useDiscordPreview, useSettings } from "@/lib/queries"
import { SettingRow, SettingsGroup } from "../primitives"

let saving = Promise.resolve()
let unsaved = 0

/**
 * Moves the switch now and saves after the saves before it, on the newest
 * settings: a save takes a while, and one built from what was on screen would
 * bring back a name the person had just turned off. When the last one fails,
 * the switches go back to what the library holds.
 */
function change(patch: DiscordRpcSettings) {
  const changed = (s: AppSettings) => ({ ...s, discordRpc: { ...s.discordRpc, ...patch } })
  queryClient.setQueryData<AppSettings>(qk.settings, (s) => s && changed(s))
  unsaved++
  saving = saving.then(async () => {
    try {
      const settings = queryClient.getQueryData<AppSettings>(qk.settings) ?? (await commands.settingsGet())
      const saved = await commands.settingsUpdate(changed(settings))
      if (unsaved === 1) queryClient.setQueryData(qk.settings, saved)
    } catch (e) {
      toast.error(t("settings:saveFailed"), { description: errorMessage(e) })
      if (unsaved === 1) await queryClient.invalidateQueries({ queryKey: qk.settings })
    } finally {
      unsaved--
    }
  })
}

// What a status may name: the setting and the catalog's word for it (settings:discord.names.<name>).
const NAMES = [
  { key: "showProject", name: "project" },
  { key: "showEpisode", name: "episode" },
  { key: "showCharacter", name: "character" },
] as const

export function DiscordSection() {
  const { t } = useTranslation("settings")
  const { data: settings } = useSettings()
  const show: DiscordRpcSettings = settings?.discordRpc ?? {}
  const { data: preview } = useDiscordPreview(show)
  // Turning it on sends the names kept from before at once, so they stay in
  // reach while it is off.
  const names = show.enabled || NAMES.some(({ key }) => show[key])

  return (
    <>
      <SettingsGroup title={t("discord.status.title")} description={t("discord.status.description")}>
        <SettingRow
          label={t("discord.show.label")}
          description={t("discord.show.description")}
        >
          <Switch
            aria-label={t("discord.show.label")}
            checked={show.enabled ?? false}
            disabled={!settings}
            onCheckedChange={(enabled) => change({ enabled })}
          />
        </SettingRow>
        <SettingRow
          label={show.enabled ? t("discord.seeing.on") : t("discord.seeing.off")}
          description={t("discord.seeing.description")}
          below={
            <div className="grid grid-cols-2 gap-3">
              {preview?.map((lines) => <Status key={`${lines.details} ${lines.state}`} lines={lines} />)}
            </div>
          }
        />
      </SettingsGroup>
      {names && (
        <SettingsGroup title={t("discord.names.title")} description={t("discord.names.description")}>
          {NAMES.map(({ key, name }) => (
            <SettingRow key={key} label={t(`discord.names.${name}`)}>
              <Switch aria-label={t(`discord.names.${name}`)} checked={show[key] ?? false} onCheckedChange={(on) => change({ [key]: on })} />
            </SettingRow>
          ))}
        </SettingsGroup>
      )}
    </>
  )
}

/** One status, in AutoScene's own look: a picture of what is said, not of Discord. */
function Status({ lines }: { lines: DiscordLines }) {
  return (
    <div className="flex min-w-0 items-center gap-3 rounded-xl bg-white/[0.04] p-3">
      <AutoSceneMark className="size-12" />
      <div className="min-w-0 text-sm">
        <div className="truncate font-semibold text-fg-1">AutoScene</div>
        <div className="truncate text-fg-2">{lines.details}</div>
        {lines.state && <div className="truncate text-fg-2">{lines.state}</div>}
      </div>
    </div>
  )
}
