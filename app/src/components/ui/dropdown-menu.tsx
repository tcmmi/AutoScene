import type * as React from "react"
import { DropdownMenu as M } from "radix-ui"
import { CaretRight, Check } from "@/components/icons"
import { cn } from "@/lib/utils"
import { MenuShortcut } from "./context-menu"
import { afterClose, menuItem, menuSurface } from "./menu-base"

export const DropdownMenu = M.Root
export const DropdownMenuTrigger = M.Trigger
export const DropdownMenuRadioGroup = M.RadioGroup
export const DropdownMenuSub = M.Sub

export function DropdownMenuContent({
  className,
  align = "end",
  sideOffset = 6,
  collisionPadding = 8,
  onCloseAutoFocus,
  ...props
}: React.ComponentProps<typeof M.Content>) {
  return (
    <M.Portal>
      <M.Content
        align={align}
        sideOffset={sideOffset}
        collisionPadding={collisionPadding}
        // Back to the ⋯ button only when the focus would be lost: an entry
        // that opened a field or a dialog keeps it there.
        onCloseAutoFocus={(e) => {
          onCloseAutoFocus?.(e)
          const active = document.activeElement
          if (active !== null && active !== document.body) e.preventDefault()
        }}
        className={cn(
          menuSurface,
          // As the context menu: a long list (plugins decide how long) scrolls inside the window.
          "max-h-[var(--radix-dropdown-menu-content-available-height)] overflow-y-auto data-[state=open]:animate-pop-in",
          className,
        )}
        {...props}
      />
    </M.Portal>
  )
}

const item = menuItem

/** `shortcut` as the keys read, e.g. ["Ctrl", "E"] — like the context menu's items. */
export function DropdownMenuItem({
  className,
  destructive,
  shortcut,
  children,
  onSelect,
  ...props
}: React.ComponentProps<typeof M.Item> & { destructive?: boolean; shortcut?: string[] }) {
  return (
    <M.Item
      className={cn(
        item,
        destructive && "text-danger data-[highlighted]:bg-danger-subtle data-[highlighted]:text-danger [&_svg]:text-danger",
        className,
      )}
      onSelect={afterClose(onSelect)}
      {...props}
    >
      {children}
      <MenuShortcut keys={shortcut} />
    </M.Item>
  )
}

/**
 * A choice to tick among several. The menu stays open: one usually ticks more
 * than one. `hint` says what it does, on a line of its own under the name (not
 * a tooltip, which would take the first Esc from the menu).
 */
export function DropdownMenuCheckboxItem({
  className,
  children,
  hint,
  onSelect,
  ...props
}: React.ComponentProps<typeof M.CheckboxItem> & { hint?: React.ReactNode }) {
  return (
    <M.CheckboxItem
      className={cn(
        item,
        "relative pl-9 data-[state=checked]:text-fg-1 data-[state=checked]:[&_svg]:text-accent",
        hint != null && "h-auto py-1.5",
        className,
      )}
      onSelect={(e) => {
        e.preventDefault()
        onSelect?.(e)
      }}
      {...props}
    >
      <M.ItemIndicator className="absolute left-3 flex items-center">
        <Check weight="bold" />
      </M.ItemIndicator>
      {hint == null ? (
        children
      ) : (
        <span>
          {children}
          <span className="block text-xs text-fg-3">{hint}</span>
        </span>
      )}
    </M.CheckboxItem>
  )
}

/** One choice of a `DropdownMenuRadioGroup`. */
export function DropdownMenuRadioItem({ className, children, ...props }: React.ComponentProps<typeof M.RadioItem>) {
  return (
    <M.RadioItem
      className={cn(item, "relative pl-9 data-[state=checked]:text-fg-1 data-[state=checked]:[&_svg]:text-accent", className)}
      {...props}
    >
      <M.ItemIndicator className="absolute left-3 flex items-center">
        <Check weight="bold" />
      </M.ItemIndicator>
      {children}
    </M.RadioItem>
  )
}

/** An entry that opens a list of its own (Right, or resting on it), inside a `DropdownMenuSub`. */
export function DropdownMenuSubTrigger({ className, children, ...props }: React.ComponentProps<typeof M.SubTrigger>) {
  return (
    <M.SubTrigger className={cn(item, "data-[state=open]:bg-active data-[state=open]:text-fg-1", className)} {...props}>
      {children}
      <CaretRight className="-mr-1" />
    </M.SubTrigger>
  )
}

/** The list a `DropdownMenuSubTrigger` opens. As long as it likes: it scrolls inside the window. */
export function DropdownMenuSubContent({
  className,
  sideOffset = 8,
  alignOffset = -6,
  collisionPadding = 8,
  ...props
}: React.ComponentProps<typeof M.SubContent>) {
  return (
    <M.Portal>
      <M.SubContent
        sideOffset={sideOffset}
        alignOffset={alignOffset}
        collisionPadding={collisionPadding}
        className={cn(
          menuSurface,
          "max-h-[var(--radix-dropdown-menu-content-available-height)] overflow-y-auto data-[state=open]:animate-pop-in",
          className,
        )}
        {...props}
      />
    </M.Portal>
  )
}

export function DropdownMenuSeparator() {
  return <M.Separator className="mx-2 my-1.5 h-px bg-hairline" />
}

export function DropdownMenuLabel({ className, ...props }: React.ComponentProps<typeof M.Label>) {
  return <M.Label className={cn("eyebrow px-3 pt-2 pb-1", className)} {...props} />
}
