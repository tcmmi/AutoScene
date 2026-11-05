import { useState } from "react"
import { useNavigate, useParams } from "@tanstack/react-router"
import type { PluginInfo } from "@/bindings"
import { DotsThree } from "@/components/icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { Hero } from "@/components/ui/hero"
import { contextKit, dropdownKit } from "@/components/ui/menu-actions"
import { Page } from "@/components/ui/page"
import { Switch } from "@/components/ui/switch"
import { TopBar } from "@/components/ui/topbar"
import { useAmbientOf } from "@/lib/ambient"
import { usePluginBanner, usePlugins } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { useLoadAgain } from "./load-again"
import { PluginSettingsSection } from "./PluginSettingsSection"
import { setPluginEnabled } from "./plugin-actions"
import { authorLine, pluginPicture, trustLabel } from "./plugin-items"
import { PluginItems } from "./plugin-menu"
import { UninstallDialog } from "./UninstallDialog"

/** The window title's name of a plugin's settings: `Plugins · <its name>`. */
export function PluginCrumb({ id }: { id: string }) {
  const { data } = usePlugins()
  return <>Plugins · {data?.find((p) => p.id === id)?.name ?? "Plugin"}</>
}

/** A plugin's settings, its own screen under Plugins (`/plugins/$id`). */
export function PluginSettingsPage() {
  const { id } = useParams({ from: "/plugins/$id" })
  // One screen per plugin: going from one plugin's screen straight to
  // another's (Back, a rail tile's menu) must not carry a slider still
  // settling, a field being typed in or a review over to the next one.
  return <PluginScreen key={id} id={id} />
}

/**
 * The screen of the plugin `id`: its picture as the hero, with what it says
 * of itself, then its settings. One that isn't installed (or a list that
 * couldn't be read) says so in the middle of the view.
 */
export function PluginScreen({ id }: { id: string }) {
  const { data: plugins } = usePlugins()
  const p = plugins?.find((x) => x.id === id)
  return (
    <Page topbar={<TopBar overlay={p != null} back={{ label: "Plugins", to: "/plugins" }} title={p?.name} />}>
      {p ? (
        <>
          <PluginHero p={p} />
          <div className="max-w-4xl px-10 pt-2 pb-14">
            <PluginSettingsSection id={id} />
          </div>
        </>
      ) : (
        <PluginSettingsSection id={id} />
      )}
    </Page>
  )
}

/**
 * A plugin's hero. Its picture is always blurred into the backdrop, so
 * nothing AutoScene says of it sits on the plugin's own sharp pixels: its
 * banner, else its icon. Sharp, it is framed beside the name. The name, the
 * trust and the facts are AutoScene's, drawn beside the picture, never on it.
 */
export function PluginHero({ p }: { p: PluginInfo }) {
  const { data: banner } = usePluginBanner(p)
  const icon = pluginPicture(p.icon)
  // The icon shows until the banner is in hand.
  const picture = banner ?? icon
  const ambient = useAmbientOf(picture, banner ? `plugin-banner:${p.bannerHash}` : undefined)
  const navigate = useNavigate()
  const [removing, setRemoving] = useState(false)
  const uninstall = () => setRemoving(true)
  const again = useLoadAgain()
  const loadAgain = () => void again.loadAgain(p)
  const trust = trustLabel(p)
  // AutoScene took it back: it doesn't run, whatever its switch said.
  const withdrawn = p.withdrawn != null
  const dim = !p.enabled || withdrawn
  const by = authorLine(p)
  return (
    <>
      <ContextArea menu={<PluginItems kit={contextKit} p={p} settings={false} onLoadAgain={loadAgain} onUninstall={uninstall} />}>
        <Hero src={picture} blur dimmed={dim} ambient={ambient} className="min-h-[clamp(320px,44vh,460px)]">
          <div className="flex items-end justify-between gap-8">
            <div className="flex min-w-0 items-end gap-7">
              {banner ? (
                <img
                  src={banner}
                  alt=""
                  draggable={false}
                  className={cn("picture-edge h-28 w-56 shrink-0 rounded-2xl object-cover shadow-lift", dim && "opacity-60")}
                />
              ) : (
                icon && (
                  <span
                    className={cn(
                      "flex size-28 shrink-0 items-center justify-center rounded-[28px] bg-white/[0.08] shadow-lift ring-1 ring-white/15 backdrop-blur-md",
                      dim && "opacity-60",
                    )}
                  >
                    <img src={icon} alt="" draggable={false} className="size-16 object-contain" />
                  </span>
                )
              )}
              <div className="min-w-0 pb-1">
                <h1
                  className={cn(
                    "display leading-[1.05] break-words",
                    p.name.length > 28 ? "text-5xl" : "text-6xl",
                    // Turned off, the name dims with the picture: one without
                    // a picture says so too, not only by its switch.
                    dim ? "text-white/60" : "text-white",
                  )}
                >
                  {p.name}
                </h1>
                <div className="mt-4 flex flex-wrap items-center gap-2">
                  <Badge tone={trust.tone === "danger" ? "danger" : "media"}>{trust.label}</Badge>
                  {p.sandbox && <Badge tone="media">Sandboxed</Badge>}
                  <Badge tone="media">version {p.version}</Badge>
                  {p.license && <Badge tone="media">{p.license}</Badge>}
                  {by && <span className="pl-1 text-sm text-white/70">{by}</span>}
                </div>
                {p.description && <p className="mt-4 max-w-2xl text-md leading-relaxed text-white/80">{p.description}</p>}
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-3 pb-1">
              <label className="flex items-center gap-3 text-sm font-medium text-white/80">
                Turned on
                <Switch checked={p.enabled && !withdrawn} disabled={withdrawn} onCheckedChange={(on) => void setPluginEnabled(p, on)} />
              </label>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button size="icon-lg" variant="glass" aria-label={`${p.name} options`}>
                    <DotsThree weight="bold" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <PluginItems kit={dropdownKit} p={p} settings={false} onLoadAgain={loadAgain} onUninstall={uninstall} />
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
        </Hero>
      </ContextArea>
      {/* Outside the right-click area, which a right-click in them would reach
          through their portal. Gone, it leaves for Plugins. */}
      <UninstallDialog p={removing ? p : null} onClose={() => setRemoving(false)} onUninstalled={() => navigate({ to: "/plugins" })} />
    </>
  )
}
