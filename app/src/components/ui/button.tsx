import * as React from "react"
import { Slot } from "radix-ui"
import { cva, type VariantProps } from "class-variance-authority"
import * as icons from "@/components/icons"
import { CircleNotch } from "@/components/icons"
import { cn } from "@/lib/utils"

/**
 * Glass pills. `primary` is the one main action of a screen (bright white
 * glass); `secondary` and `glass` are dark glass that sits back, on the dark
 * and on pictures; `accent` (gold glass) is for what is selected or running.
 * `cta` is the solid warm-white pill with near-black text (DESIGN.md "The main
 * action"), for the answer most people give on a surface that leads with a
 * picture (the first-start notice).
 */
const buttonVariants = cva(
  [
    "inline-flex shrink-0 select-none items-center justify-center gap-2 whitespace-nowrap rounded-full font-medium",
    "transition-[background-color,border-color,color,box-shadow,transform] duration-[var(--dur-fast)] ease-out",
    "disabled:pointer-events-none disabled:opacity-40 aria-busy:pointer-events-none active:scale-[0.97]",
    "[&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  ],
  {
    variants: {
      variant: {
        primary:
          "border border-white/40 bg-white/[0.18] font-semibold text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.3)] backdrop-blur-xl hover:border-white/60 hover:bg-white/[0.26] active:bg-white/[0.22]",
        cta: "bg-cta font-semibold text-on-cta shadow-[0_10px_30px_-12px_rgb(255_255_255/0.5)] hover:bg-cta-hover active:bg-cta/90",
        accent:
          "border border-accent/60 bg-accent/[0.18] font-semibold text-accent shadow-[inset_0_1px_0_rgb(224_180_92/0.4)] backdrop-blur-xl hover:border-accent hover:bg-accent/[0.26] active:bg-accent/[0.22]",
        secondary:
          "border border-white/10 bg-black/40 text-fg-1 backdrop-blur-xl shadow-lg hover:border-white/20 hover:bg-black/60 active:bg-black/70",
        glass:
          "border border-white/10 bg-black/30 text-white backdrop-blur-xl backdrop-saturate-150 hover:border-white/20 hover:bg-black/50 active:bg-black/60",
        ghost: "text-fg-2 hover:bg-hover hover:text-fg-1 active:bg-active",
        danger: "border border-danger/40 bg-danger-subtle text-danger hover:border-danger/70 hover:bg-danger/20",
      },
      size: {
        sm: "h-8 px-3.5 text-sm",
        md: "h-9 px-4 text-md",
        lg: "h-11 px-6 text-md [&_svg]:size-[18px]",
        icon: "size-9",
        "icon-sm": "size-8 [&_svg]:size-[15px]",
        "icon-lg": "size-11 [&_svg]:size-5",
      },
    },
    compoundVariants: [
      // A menu opened from a ghost icon: it stays lit while the menu is up.
      {
        variant: "ghost",
        size: ["icon", "icon-sm", "icon-lg"],
        class: "data-[state=open]:bg-hover data-[state=open]:text-fg-1",
      },
    ],
    defaultVariants: { variant: "secondary", size: "md" },
  },
)

// No `title`: the browser's tooltip isn't the app's (wrap in `Tooltip`).
export type ButtonProps = Omit<React.ComponentProps<"button">, "title"> &
  VariantProps<typeof buttonVariants> & { asChild?: boolean; busy?: boolean }

const ICONS = new Set<unknown>(Object.values(icons))

/** A leading icon: one of the app's own (`components/icons`) or a bare `<svg>`. A ring, a badge or a key hint is not one. */
function isIcon(child: unknown): child is React.ReactElement<{ className?: string }> {
  return React.isValidElement(child) && (child.type === "svg" || ICONS.has(child.type))
}

/**
 * What a busy button shows, at the width it had: the spinner takes the first
 * icon's place, at its size. Without one, the label stays in the layout and
 * for screen readers, out of sight, with the spinner over it.
 */
function busyChildren(children: React.ReactNode) {
  const kids = React.Children.toArray(children)
  const first = kids.findIndex((kid) => typeof kid !== "string" || kid.trim() !== "")
  const icon = kids[first]
  if (isIcon(icon)) {
    kids[first] = <CircleNotch key={icon.key} aria-hidden className={cn("animate-spin", icon.props.className)} />
    return kids
  }
  return (
    <>
      <span className="inline-flex items-center gap-2 opacity-0">{children}</span>
      <span className="absolute inset-0 flex items-center justify-center">
        <CircleNotch aria-hidden className="animate-spin" />
      </span>
    </>
  )
}

/**
 * `busy` says its action is running: a spinner (see `busyChildren`), and
 * clicks do nothing until it ends (it stays focusable, and lit so the spinner reads). Not with `asChild`: a link has no running
 * action, and its children aren't the button's to change.
 */
export function Button({ className, variant, size, asChild, type = "button", busy, disabled, children, onClick, ...props }: ButtonProps) {
  const Comp = asChild ? Slot.Root : "button"
  const working = busy && !asChild
  return (
    <Comp
      type={asChild ? undefined : type}
      disabled={disabled}
      aria-busy={working || undefined}
      aria-disabled={working || undefined}
      // Busy is not `disabled`: a focused button that gets disabled drops
      // focus to the page, and the keyboard loses its place.
      onClick={working ? (e: React.MouseEvent) => e.preventDefault() : onClick}
      className={cn(buttonVariants({ variant, size }), working && "relative", className)}
      {...props}
    >
      {working ? busyChildren(children) : children}
    </Comp>
  )
}
