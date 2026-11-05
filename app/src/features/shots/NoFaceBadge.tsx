import { UserCircleDashed } from "@/components/icons"
import { Badge } from "@/components/ui/badge"

/** A shot where the character's face doesn't show: their body does (turned away, too small). */
export function NoFaceBadge({ className }: { className?: string }) {
  return (
    <Badge tone="picture" size="sm" className={className}>
      <UserCircleDashed weight="bold" aria-hidden />
      No face
    </Badge>
  )
}
