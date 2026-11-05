import { useRef } from "react"
import { useTranslation } from "react-i18next"
import { MagnifyingGlass, X } from "@/components/icons"
import { Input } from "@/components/ui/input"
import { cn } from "@/lib/utils"

/**
 * The pill that finds things in a list. `label` is its placeholder and its
 * name for screen readers. Escape clears it, as the × does.
 */
export function SearchField({
  value,
  onChange,
  label,
  className,
}: {
  value: string
  onChange: (value: string) => void
  label: string
  className?: string
}) {
  const { t } = useTranslation("ui")
  const input = useRef<HTMLInputElement>(null)
  return (
    <div role="search" aria-label={label} className={cn("relative w-64", className)}>
      <MagnifyingGlass className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-fg-3" />
      <Input
        ref={input}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) onChange("")
        }}
        placeholder={label}
        aria-label={label}
        className="h-9 rounded-full pr-9 pl-10 text-sm"
      />
      {value && (
        <button
          type="button"
          onClick={() => {
            onChange("")
            input.current?.focus()
          }}
          aria-label={t("search.clear")}
          className="absolute top-1/2 right-1.5 flex size-6 -translate-y-1/2 items-center justify-center rounded-full text-fg-3 transition-colors hover:bg-hover hover:text-fg-1"
        >
          <X className="size-3.5" />
        </button>
      )}
    </div>
  )
}
