import type * as React from "react"
import { useId } from "react"
import { Switch } from "@/components/ui/switch"
import { cn } from "@/lib/utils"

/**
 * One option in a dialog: what it does on the left, its switch on the right,
 * in an inset box. The whole row is the target. Settings use `SettingRow`.
 */
export function OptionRow({
  label,
  description,
  className,
  ...props
}: Omit<React.ComponentProps<typeof Switch>, "children" | "title"> & {
  label: React.ReactNode
  description?: React.ReactNode
}) {
  const id = useId()
  return (
    <label
      className={cn(
        "flex cursor-pointer items-center justify-between gap-4 rounded-lg border border-hairline bg-inset px-3.5 py-3 transition-opacity has-[:disabled]:cursor-default has-[:disabled]:opacity-50",
        className,
      )}
    >
      <span>
        <span id={`${id}-label`} className="block text-sm text-fg-1">
          {label}
        </span>
        {description && (
          <span id={`${id}-description`} className="block text-xs text-fg-3">
            {description}
          </span>
        )}
      </span>
      <Switch aria-labelledby={`${id}-label`} aria-describedby={description ? `${id}-description` : undefined} {...props} />
    </label>
  )
}
