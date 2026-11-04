import { Link } from "@tanstack/react-router"
import { toast } from "sonner"
import type { CatalogPlugin, PluginInfo } from "@/bindings"
import { ArrowsClockwise, CircleNotch, DotsThree } from "@/components/icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { contextKit, dropdownKit } from "@/components/ui/menu-actions"
import { Switch } from "@/components/ui/switch"
import { TruncatedText } from "@/components/ui/tooltip"
import { commands, errorMessage } from "@/lib/ipc"
import { usePluginBanner } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { getsPlugin, usePluginReview } from "@/state/plugin-review"
import { updatePlugin } from "./catalog-actions"
import { keepPlugin } from "./plugin-actions"
import { PluginArt } from "./PluginArt"
import { reviewedPackage, trustLabel } from "./plugin-items"
import { PluginItems } from "./plugin-menu"

/** Why a plugin AutoScene hasn't reviewed doesn't run, on its card. */
const NOT_ALLOWED = "Doesn't run until the setting below allows it."

/**
 * A plugin as its artwork, in the language of a project's card: the art opens
 * its settings, ⋯ shows on hover. Under the art, AutoScene's caption: how far
 * it trusts the plugin, who the plugin says it is by and its version, and its
 * switch; then why it doesn't run, when it is broken or not allowed. None of
 * that is ever drawn on the plugin's own pixels. When the catalog offers a
 * newer version (`update`), Update… sits before the switch. A plugin AutoScene
 * withdrew says "Withdrawn" in place of its trust, with the catalog's reason
 * under it, and its switch is off and can't be turned on.
 */
export function PluginCard({
  p,
  update,
  onLoadAgain,
  onUninstall,
}: {
  p: PluginInfo
  update?: CatalogPlugin
  onLoadAgain: () => void
  onUninstall: () => void
}) {
  // Its update is being got, or waits its turn: the button spins, and
  // waits (a second click would only ask again).
  const getting = usePluginReview((s) => getsPlugin(s, p.id))
  const onUpdate = update && (() => void updatePlugin(update))
  const { data: banner } = usePluginBanner(p)
  const trust = trustLabel(p)
  // AutoScene took it back: it doesn't run, whatever its switch said.
  const withdrawn = p.withdrawn != null
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
    <ContextArea menu={<PluginItems kit={contextKit} p={p} onLoadAgain={onLoadAgain} onUninstall={onUninstall} onUpdate={onUpdate} />}>
      <div role="group" aria-label={p.name}>
        {/* The art and its ⋯ rise together; the caption stays. */}
        <div className="group/art relative transition-transform duration-[var(--dur-med)] ease-out hover:-translate-y-1 has-[a:focus-visible]:-translate-y-1">
          <Link to="/plugins/$id" params={{ id: p.id }} className="block rounded-2xl">
            <PluginArt
              name={p.name}
              icon={p.icon}
              banner={banner}
              className={cn(
                "aspect-[2/1] rounded-2xl picture-edge transition-[box-shadow,opacity] duration-[var(--dur-med)] group-hover/art:shadow-lift",
                // Turned off or withdrawn, it dims, as its settings' picture does.
                (!p.enabled || withdrawn) && "opacity-60",
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
              <PluginItems kit={dropdownKit} p={p} onLoadAgain={onLoadAgain} onUninstall={onUninstall} onUpdate={onUpdate} />
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        {/* The facts have a line of their own: beside the badge, three cards to a row would cut them. */}
        <div className="mt-3 flex items-center justify-between gap-3 pl-0.5">
          <Badge tone={trust.tone} className="min-w-0 truncate">
            {trust.label}
          </Badge>
          {update && (
            <Button size="sm" className="ml-auto" disabled={getting} onClick={onUpdate}>
              {getting ? <CircleNotch className="animate-spin" /> : <ArrowsClockwise />} Update…
            </Button>
          )}
          <Switch
            aria-label={`Enable ${p.name}`}
            checked={p.enabled && !withdrawn}
            disabled={withdrawn}
            onCheckedChange={(on) => void setEnabled(on)}
          />
        </div>
        <TruncatedText className="mt-1.5 pl-0.5 text-sm text-fg-2">{facts}</TruncatedText>
        {/* Withdrawn is AutoScene's word; broken is a fault; not allowed is the person's choice, which the setting below changes. */}
        {p.withdrawn != null ? (
          <p className="mt-1.5 pl-0.5 text-sm text-danger">{p.withdrawn}</p>
        ) : p.broken != null ? (
          <p className="mt-1.5 pl-0.5 text-sm text-danger">{p.broken}</p>
        ) : (
          blocked && <p className="mt-1.5 pl-0.5 text-sm text-fg-3">{blocked}</p>
        )}
      </div>
    </ContextArea>
  )
}
