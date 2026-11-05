import { useTranslation } from "react-i18next"
import { settingsLabel } from "@/features/settings/sections"

/**
 * The window title's names for the screens that have a fixed one (see
 * `WindowTitle`). They are components, not text kept at module scope, so
 * the title follows the language.
 */
export function NameCrumb({ name }: { name: "plugins" | "cast" | "review" | "exports" }) {
  const { t } = useTranslation("shell")
  return <>{t(`crumb.${name}`)}</>
}

/** "Settings · Shortcuts". */
export function SettingsCrumb({ section }: { section: string }) {
  const { t } = useTranslation("shell")
  return <>{t("crumb.settings", { section: settingsLabel(section) })}</>
}
