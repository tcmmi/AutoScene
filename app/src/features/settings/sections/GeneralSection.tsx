import { useEffect } from "react"
import { FolderOpen } from "@/components/icons"
import { toast } from "sonner"
import { useTranslation } from "react-i18next"
import type { AppSettings, ScanChoice, UiLanguage } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Segmented } from "@/components/ui/segmented"
import { Select } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { settingsLine } from "@/lib/fast-scan"
import { presetDescription, presetName } from "@/lib/format"
import { applySettings, LANGUAGE_NAMES, OFFERED, t } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { PREFERRED_AUDIO_LANGUAGES, pickFolder } from "@/lib/media"
import { reveal } from "@/lib/os"
import { isMadePack, qk, queryClient, useAppInfo, usePresets, useScanDefaults, useSettings, useUiLocale } from "@/lib/queries"
import { PathText, SettingRow, SettingsGroup } from "../primitives"
import { HardwareGroup } from "./HardwareGroup"

async function save(next: AppSettings) {
  try {
    const s = await commands.settingsUpdate(next)
    queryClient.setQueryData(qk.settings, s)
    void queryClient.invalidateQueries({ queryKey: qk.appInfo })
    void queryClient.invalidateQueries({ queryKey: ["exportDir"] })
  } catch (e) {
    toast.error(t("settings:saveFailed"), { description: errorMessage(e) })
  }
}

/**
 * The language applies at once and with no toast: the screen switching says it. The whole tree is made again
 * in the new language (`LanguageRoot`), so the row the person was on is gone: the page's next mount puts focus
 * back on it, once, and Tab goes on from there, not from the top of the window. A pack made in the old language
 * is made again: the backend wrote its files' names, and an export would write them in the new one.
 */
let focusLanguageRow = false
async function chooseLanguage(settings: AppSettings, language: UiLanguage) {
  await save({ ...settings, language })
  focusLanguageRow = true
  await applySettings()
  void queryClient.invalidateQueries({ predicate: isMadePack })
  // The new tree is up by now. With none (the same language chosen again), nothing is left waiting for focus.
  setTimeout(() => (focusLanguageRow = false))
}

/** Fast Scan's default for new projects: only `scanDefaultSet` changes it, never a copy of the settings. */
async function chooseScan(choice: ScanChoice) {
  try {
    queryClient.setQueryData(qk.scanDefaults, await commands.scanDefaultSet(choice))
    void queryClient.invalidateQueries({ queryKey: qk.settings })
  } catch (e) {
    toast.error(t("settings:saveFailed"), { description: errorMessage(e) })
  }
}

/** The three choices of Fast Scan, by the word the catalog has for each (settings:general.characters.fastScan.<word>). */
const SCAN_CHOICES = [
  { value: "auto", word: "auto" },
  { value: "fast", word: "on" },
  { value: "full", word: "off" },
] as const satisfies { value: ScanChoice; word: string }[]

export function GeneralSection() {
  const { t } = useTranslation("settings")
  const { data: info } = useAppInfo()
  const { data: settings } = useSettings()
  const { data: presets } = usePresets()
  const preset = presets?.find((p) => p.id === settings?.exportPresetId)
  const { data: scan } = useScanDefaults()
  const { data: locale } = useUiLocale()

  const languageLabel = t("general.language.app.label")
  const languageReady = settings != null && locale != null
  useEffect(() => {
    if (!focusLanguageRow || !languageReady) return
    focusLanguageRow = false
    const row = [...document.querySelectorAll<HTMLElement>('button[role="combobox"]')].find((b) => b.getAttribute("aria-label") === languageLabel)
    row?.focus()
  }, [languageReady, languageLabel])

  const changeLibrary = async () => {
    if (!settings) return
    const dir = await pickFolder(t("general.storage.library.pickTitle"))
    if (dir) await save({ ...settings, libraryRoot: dir })
  }

  return (
    <>
      <SettingsGroup title={t("general.language.title")} description={t("general.language.description")}>
        <SettingRow label={t("general.language.app.label")}>
          <Select
            aria-label={t("general.language.app.label")}
            className="w-[260px]"
            value={settings && (settings.language ?? "system")}
            disabled={!settings || !locale}
            onChange={(language) => settings && void chooseLanguage(settings, language as UiLanguage)}
            options={[
              {
                value: "system",
                label: t("general.language.app.automatic", { language: LANGUAGE_NAMES[locale?.automatic ?? "en"] }),
              },
              ...OFFERED.map((lang) => ({ value: lang, label: LANGUAGE_NAMES[lang] })),
            ]}
          />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title={t("general.storage.title")} description={t("general.storage.description")}>
        <SettingRow
          label={t("general.storage.library.label")}
          description={
            settings && (
              <>
                <PathText path={settings.libraryRoot} />
                <span className="mt-1 block text-fg-3">{t("general.storage.library.note")}</span>
              </>
            )
          }
        >
          <Button size="sm" variant="ghost" disabled={!settings} onClick={() => settings && reveal(settings.libraryRoot)}>
            <FolderOpen /> {t("reveal")}
          </Button>
          <Button size="sm" disabled={!settings} onClick={() => void changeLibrary()}>
            {t("general.storage.library.change")}
          </Button>
        </SettingRow>
        <SettingRow label={t("general.storage.appData")} description={info && <PathText path={info.dataDir} />}>
          <Button size="sm" variant="ghost" disabled={!info} onClick={() => info && reveal(info.dataDir)}>
            <FolderOpen /> {t("reveal")}
          </Button>
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title={t("general.characters.title")} description={t("general.characters.description")}>
        <SettingRow
          label={t("general.characters.bodies.label")}
          description={t("general.characters.bodies.description")}
        >
          <Switch
            aria-label={t("general.characters.bodies.label")}
            checked={settings?.findBodies ?? false}
            disabled={!settings}
            onCheckedChange={(on) => {
              if (!settings) return
              // On Automatic, live action keeps the full scan while it is on.
              const scanChanged = () => queryClient.invalidateQueries({ queryKey: qk.scanDefaults })
              void save({ ...settings, findBodies: on }).then(scanChanged)
            }}
          />
        </SettingRow>
        <SettingRow
          label={t("general.characters.fastScan.label")}
          description={
            <span className="block text-pretty">
              {t("general.characters.fastScan.description")}
              {scan && info && (
                <span aria-live="polite" className="mt-2 block text-fg-1">
                  {settingsLine(scan, info.platform)}
                </span>
              )}
            </span>
          }
        >
          <Segmented<ScanChoice | "">
            aria-label={t("general.characters.fastScan.aria")}
            value={scan?.choice ?? ""}
            disabled={!scan}
            onChange={(choice) => choice && void chooseScan(choice)}
            options={SCAN_CHOICES.map((c) => ({ value: c.value, label: t(`general.characters.fastScan.${c.word}`) }))}
          />
        </SettingRow>
        <SettingRow
          label={t("general.characters.names.label")}
          description={t("general.characters.names.description")}
        >
          <Switch
            aria-label={t("general.characters.names.label")}
            checked={settings?.anilist ?? false}
            disabled={!settings}
            onCheckedChange={(on) => settings && void save({ ...settings, anilist: on })}
          />
        </SettingRow>
      </SettingsGroup>

      <HardwareGroup />

      <SettingsGroup title={t("general.audio.title")} description={t("general.audio.description")}>
        <SettingRow label={t("general.audio.language.label")} description={t("general.audio.language.description")}>
          <Select
            aria-label={t("general.audio.language.aria")}
            className="w-[260px]"
            value={settings && (settings.audioLanguage || "default")}
            disabled={!settings}
            onChange={(lang) => settings && void save({ ...settings, audioLanguage: lang === "default" ? "" : lang })}
            options={PREFERRED_AUDIO_LANGUAGES.map((l) => ({ ...l, value: l.value || "default" }))}
          />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title={t("general.export.title")} description={t("general.export.description")}>
        <SettingRow label={t("general.export.format.label")} description={preset && presetDescription(preset)}>
          <Select
            aria-label={t("general.export.format.aria")}
            className="w-[260px]"
            value={settings?.exportPresetId}
            disabled={!settings}
            onChange={(id) => settings && void save({ ...settings, exportPresetId: id })}
            options={(presets ?? []).map((p) => ({ value: p.id, label: presetName(p) }))}
          />
        </SettingRow>
      </SettingsGroup>
    </>
  )
}
