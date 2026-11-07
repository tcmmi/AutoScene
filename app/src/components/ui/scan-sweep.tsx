import { cn } from "@/lib/utils"

/**
 * One gold line crossing a picture, once, the moment something finished
 * reading it. Mount it when that happens. Waiting work stands still, so this
 * never loops. Its parent must be positioned, and clip what overflows.
 */
export function ScanSweep({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        "pointer-events-none absolute inset-y-0 w-px animate-scan-sweep bg-accent opacity-0 shadow-[0_0_10px_2px_var(--accent)]",
        className,
      )}
    />
  )
}
