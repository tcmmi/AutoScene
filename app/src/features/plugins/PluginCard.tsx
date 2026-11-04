import { Link } from "@tanstack/react-router"
import { toast } from "sonner"
import type { PluginInfo } from "@/bindings"
import { DotsThree } from "@/components/icons"
import { Artwork } from "@/components/ui/artwork"
import { Badge } from "@/components/ui/badge"
import { ContextArea } from "@/components/ui/context-area"
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { contextKit, dropdownKit } from "@/components/ui/menu-actions"
import { Switch } from "@/components/ui/switch"
import { TruncatedText } from "@/components/ui/tooltip"
import { commands, errorMessage } from "@/lib/ipc"
import { usePluginBanner } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { keepPlugin } from "./plugin-actions"
import { pluginPicture, reviewedPackage, trustLabel } from "./plugin-items"
import { PluginItems } from "./plugin-menu"

/** Why a plugin AutoScene hasn't reviewed doesn't run, on its card. */
const NOT_ALLOWED = "Doesn't run until the setting below allows it."

/**
 * A plugin as its artwork, in the language of a project's card: the art opens
 * its settings, ⋯ shows on hover. Under the art, AutoScene's caption: how far
 * it trusts the plugin, who the plugin says it is by and its version, and its
 * switch; then why it doesn't run, when it is broken or not allowed. None of
 * that is ever drawn on the plugin's own pixels.
 */
export function PluginCard({
  p,
  onLoadAgain,
  onUninstall,
}: {
  p: PluginInfo
  onLoadAgain: () => void
  onUninstall: () => void
}) {
  const trust = trustLabel(p)
  const by = p.author && (reviewedPackage(p) ? `By ${p.author}` : `Says it is by ${p.author}`)
  const facts = [by, `version ${p.version}`].filter(Boolean).join(" · ")
  // The backend's reason sends people to Plugins, where the card already is:
  // the card's own words point at the setting under the cards instead.
  const blocked = p.broken == null && p.enabled && p.blockedReason != null ? NOT_ALLOWED : null
  const setEnabled = async (on: boolean) => {
    try {
      keepPlugin(await commands.pluginSetEnabled(p.id, on))
    } catch (e) {
      toast.error(on ? "Couldn't turn the plugin on" : "Couldn't turn the plugin off", { description: errorMessage(e) })
    }
  }
  return (
    <ContextArea menu={<PluginItems kit={contextKit} p={p} onLoadAgain={onLoadAgain} onUninstall={onUninstall} />}>
      <div role="group" aria-label={p.name}>
        {/* The art and its ⋯ rise together; the caption stays. */}
        <div className="group/art relative transition-transform duration-[var(--dur-med)] ease-out hover:-translate-y-1 has-[a:focus-visible]:-translate-y-1">
          <Link to="/plugins/$id" params={{ id: p.id }} className="block rounded-2xl">
            <PluginArt
              p={p}
              className={cn(
                "aspect-[2/1] rounded-2xl picture-edge transition-[box-shadow,opacity] duration-[var(--dur-med)] group-hover/art:shadow-lift",
                // Turned off, it dims, as its settings' picture does.
                !p.enabled && "opacity-60",
              )}
            />
          </Link>
          <DropdownMenu>
            <DropdownMenuTrigger
              aria-label={`${p.name} options`}
              className={cn(
                "absolute top-3 right-3 flex size-8 items-center justify-center rounded-full border border-white/15 bg-black/40 text-white opacity-0 backdrop-blur-md transition-opacity",
                "group-hover/art:opacity-100 hover:bg-black/60 focus-visible:opacity-100 data-[state=open]:opacity-100",
              )}
            >
              <DotsThree className="size-5" weight="bold" />
            </DropdownMenuTrigger>
            <DropdownMenuContent>
              <PluginItems kit={dropdownKit} p={p} onLoadAgain={onLoadAgain} onUninstall={onUninstall} />
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        {/* The facts have a line of their own: beside the badge, three cards to a row would cut them. */}
        <div className="mt-3 flex items-center justify-between gap-3 pl-0.5">
          <Badge tone={trust.tone} className="min-w-0 truncate">
            {trust.label}
          </Badge>
          <Switch aria-label={`Enable ${p.name}`} checked={p.enabled} onCheckedChange={(on) => void setEnabled(on)} />
        </div>
        <TruncatedText className="mt-1.5 pl-0.5 text-sm text-fg-2">{facts}</TruncatedText>
        {/* Broken is a fault; not allowed is the person's choice, which the setting below changes. */}
        {p.broken != null ? (
          <p className="mt-1.5 pl-0.5 text-sm text-danger">{p.broken}</p>
        ) : (
          blocked && <p className="mt-1.5 pl-0.5 text-sm text-fg-3">{blocked}</p>
        )}
      </div>
    </ContextArea>
  )
}

/**
 * A plugin's art, 2:1: its banner with its name on the dark bottom; else its
 * icon blurred into a backdrop, the sharp icon on glass and the name; else its
 * name set large as the picture, as a TV app shows a title without artwork.
 * Until its banner is in hand, the icon or the name shows.
 */
function PluginArt({ p, className }: { p: PluginInfo; className?: string }) {
  const { data: banner } = usePluginBanner(p)
  const picture = pluginPicture(banner)
  const icon = pluginPicture(p.icon)
  const art = picture != null || icon != null
  return (
    <Artwork
      src={picture}
      className={className}
      imgClassName="group-hover/art:scale-[1.04] duration-[600ms]"
      fallback={
        <div aria-hidden className="artwork-fallback absolute inset-0 -z-10 overflow-hidden">
          {icon && (
            <img
              src={icon}
              alt=""
              draggable={false}
              className="size-full scale-150 object-cover blur-2xl saturate-150 transition-transform duration-[600ms] ease-out group-hover/art:scale-[1.6]"
            />
          )}
        </div>
      }
    >
      {art ? (
        <>
          <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/85 via-black/20 to-transparent" />
          <div className="absolute inset-x-0 bottom-0 p-4">
            {picture == null && icon && (
              <span className="mb-3 flex size-14 items-center justify-center rounded-2xl border border-white/15 bg-white/10 shadow-lg backdrop-blur-md">
                <img src={icon} alt="" draggable={false} className="size-9 object-contain" />
              </span>
            )}
            <div className="display truncate text-2xl text-white drop-shadow-[0_2px_12px_rgb(0_0_0/0.6)]">{p.name}</div>
          </div>
        </>
      ) : (
        <div className="absolute inset-x-0 bottom-0 p-5">
          <div className="display line-clamp-3 text-3xl break-words text-fg-1">{p.name}</div>
        </div>
      )}
    </Artwork>
  )
}
