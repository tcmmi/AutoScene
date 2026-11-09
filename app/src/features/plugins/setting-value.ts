import type { SettingSpec, SettingValue } from "@/bindings"

/** A plugin's setting values as they are read: a key may be missing. */
export type Values = Partial<Record<string, SettingValue>>

/**
 * The value of `s` in `values`, else its default; one of another type (its
 * plugin.json changed) counts as none. Only the object's own keys count: a
 * setting may be called `constructor`.
 */
export function valueOf(s: SettingSpec, values: Values | undefined): SettingValue {
  const v = values && Object.hasOwn(values, s.key) ? values[s.key] : undefined
  const k = s.kind
  switch (k.type) {
    case "boolean":
      return typeof v === "boolean" ? v : k.default
    case "number":
      return typeof v === "number" ? v : k.default
    case "string":
      return typeof v === "string" ? v : (k.default ?? "")
    case "select":
      return typeof v === "string" && k.options.some((o) => o.value === v) ? v : k.default
    case "secret":
      return null
  }
}
