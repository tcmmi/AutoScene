import { useTranslation } from "react-i18next"
import type { ExportOptions, ExportPreset } from "@/bindings"
import { OptionRow } from "@/components/ui/option-row"

/** What an export changes of its clips. Bit-Perfect keeps the source as it is, so it offers neither. */
export function ExportOptionRows({
  preset,
  options,
  onChange,
}: {
  preset: ExportPreset | undefined
  options: ExportOptions
  onChange: (options: ExportOptions) => void
}) {
  const { t } = useTranslation("export")
  if (!preset || preset.video.codec === "lossless") return null
  return (
    <div className="flex flex-col gap-2">
      <OptionRow
        label={t("option.removeBars")}
        description={t("option.removeBarsHint")}
        checked={options.removeBars}
        onCheckedChange={(removeBars) => onChange({ ...options, removeBars })}
      />
      <OptionRow
        label={t("option.matchLoudness")}
        description={t("option.matchLoudnessHint")}
        checked={options.matchLoudness}
        onCheckedChange={(matchLoudness) => onChange({ ...options, matchLoudness })}
      />
    </div>
  )
}
