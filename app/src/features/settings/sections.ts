import { isAppSection, type SettingsSection } from "@/lib/actions"
import { t } from "@/lib/i18n"

/** A section's name in the language in use: the list, the page heading and the window title read it. */
export function sectionLabel(section: SettingsSection): string {
  return t(`settings:section.${section}`)
}

/** The name of a section by its route (any text), "Settings" for one AutoScene doesn't have. */
export function settingsLabel(section: string): string {
  return isAppSection(section) ? sectionLabel(section) : t("settings:page.title")
}
