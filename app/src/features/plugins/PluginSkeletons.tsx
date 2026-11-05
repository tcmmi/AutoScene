import { Skeleton } from "@/components/ui/skeleton"
import { CARD_GRID } from "@/lib/grid"

/** Where the plugin cards will be while the list loads: three, as tall as their artwork. */
export function PluginSkeletons() {
  return (
    <div className={CARD_GRID}>
      {Array.from({ length: 3 }, (_, i) => (
        <Skeleton key={i} className="aspect-[2/1] rounded-2xl" />
      ))}
    </div>
  )
}
