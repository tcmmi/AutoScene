import { useEffect, useRef } from "react"
import { Outlet } from "@tanstack/react-router"
import { NewProjectDialog } from "@/features/library/NewProjectDialog"
import { InstallReviewDialog } from "@/features/plugins/InstallReviewDialog"
import { useOpenedPackages } from "@/features/plugins/use-opened-packages"
import { RelinkDialog } from "@/features/project/RelinkDialog"
import { useAppActions, useGlobalBindings } from "@/lib/actions"
import { installBrowserGuards } from "@/lib/browser-guards"
import { startEventBridge } from "@/lib/events"
import { useKeymapSync } from "@/lib/keymap"
import { useLayerSafetyNet } from "@/lib/layers"
import { isTauri, isWindows } from "@/lib/platform"
import { useDiscordRPC } from "@/lib/useDiscordRPC"
import { startUpdateSchedule } from "@/state/updates"
import { useUi } from "@/state/ui"
import { ActivityDrawer } from "./ActivityDrawer"
import { AppMenu } from "./AppMenu"
import { CommandPalette } from "./CommandPalette"
import { DropZone } from "./DropZone"
import { FastScanNotice } from "./FastScanNotice"
import { Rail } from "./Rail"
import { TextMenu } from "./TextMenu"
import { UpdateDialog } from "./UpdateDialog"
import { WindowControls } from "./WindowControls"
import { WindowTitle } from "./WindowTitle"

/**
 * The window: the rail on the left (on the native material), the view on
 * the rest, its top bar floating over it. The Windows caption buttons float
 * over the top-right corner so every view has them.
 */
export function AppShell() {
  useGlobalBindings()
  useKeymapSync()
  useLayerSafetyNet()
  useOpenedPackages()
  useDiscordRPC()
  useEffect(() => startUpdateSchedule(), [])
  // The bridge lives as long as the window: it asks for where to go when it needs to.
  const { goPlugins } = useAppActions()
  const go = useRef(goPlugins)
  useEffect(() => {
    go.current = goPlugins
  })
  useEffect(() => startEventBridge({ goPlugins: () => go.current() }), [])
  useEffect(() => installBrowserGuards({ dev: import.meta.env.DEV }), [])
  const railHidden = useUi((s) => s.sidebarCollapsed)

  // Every right-click is the app's: what the clicked thing offers, then the app's own entries.
  return (
    <AppMenu>
      <div className="flex h-full">
        {!railHidden && <Rail />}
        <main className="relative min-w-0 flex-1 overflow-hidden bg-canvas">
          <Outlet />
        </main>
        {isWindows && isTauri && <WindowControls />}
        <WindowTitle />
        <CommandPalette />
        <ActivityDrawer />
        <UpdateDialog />
        <NewProjectDialog />
        <RelinkDialog />
        <InstallReviewDialog />
        <DropZone />
        <FastScanNotice />
        <TextMenu />
      </div>
    </AppMenu>
  )
}
