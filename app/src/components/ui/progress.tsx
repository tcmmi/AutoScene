import { useState } from "react"
import type * as React from "react"
import { cn } from "@/lib/utils"
import { follow, type Motion, start } from "./progress-motion"

/**
 * The motion of a bar given `value` on this render: React's "adjust state when a prop
 * changed" pattern. `follow` returns the same object for the same (normalized) value,
 * so a value that doesn't change, or a bad one (NaN, ±Infinity), never re-renders.
 */
function useFollow(value: number | null | undefined): Motion {
  const [m, setM] = useState(() => start(value, performance.now()))
  // When a value comes is what the glide is timed from, so the clock is read here on purpose:
  // a render that repeats with the same value gets the same motion back, whatever the time.
  // oxlint-disable-next-line react/purity
  const next = follow(m, value, performance.now())
  if (next !== m) setM(next)
  return next
}

/**
 * Progress bar with a soft glow. `value` in 0..1; omit (or null) for a sweep: how far
 * isn't known. A stream of values glides at the pace they come, a lower one (a new
 * stage) is shown at once, never sliding back, and the fill and the sweep crossfade.
 */
export function Progress({ value, className }: { value?: number | null; className?: string }) {
  const m = useFollow(value)
  const sweeping = m.value == null
  const shown = m.value != null && m.value > 0
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={m.value != null ? Math.round(m.value * 100) : undefined}
      className={cn("relative h-1.5 w-full rounded-full bg-white/10", className)}
    >
      {/* The clip. Its glow is drawn outside it; `isolate` keeps composited children clipped (WebKit). */}
      <div
        className={cn(
          "absolute inset-0 isolate overflow-hidden rounded-full",
          (shown || sweeping) && "[filter:drop-shadow(0_0_6px_var(--accent-line))]",
        )}
      >
        {/* The fill slides in the rounded clip, so its leading cap stays round at every value. */}
        <div
          className={cn(
            "absolute inset-0 rounded-full bg-accent transition-[transform,opacity]",
            shown && "will-change-transform",
          )}
          style={{
            transform: `translateX(${(m.pos - 1) * 100}%)`,
            // At 0 it is hidden too: its edge would show as a hairline at the clip's rounded start.
            opacity: shown ? 1 : 0,
            transitionDuration: `${m.ms}ms, var(--dur-med)`,
            transitionTimingFunction: `${m.ease === "out" ? "var(--ease-out)" : "linear"}, linear`,
          }}
        />
        {/* The sweep: the wrapper fades, the bar inside moves, paused while hidden. It mounts
            the first time the bar sweeps and stays, so it never restarts. */}
        <div
          className="absolute inset-0 transition-opacity duration-[var(--dur-med)]"
          style={{ opacity: sweeping ? 1 : 0 }}
        >
          {m.swept && (
            // Under reduced motion it stands still across the whole track, a faint tint that fades
            // (the reduced-motion rule in the base styles), so it never reads as a third done.
            <div
              className="absolute inset-y-0 w-1/3 animate-indeterminate rounded-full bg-accent motion-reduce:inset-x-0 motion-reduce:w-auto motion-reduce:bg-accent/40"
              style={{ animationPlayState: sweeping ? "running" : "paused" }}
            />
          )}
        </div>
      </div>
    </div>
  )
}

/**
 * The same as a ring, filling clockwise from the top; a `size-*` class sizes
 * it. `value` in 0..1; omit (or null) and an arc spins. `grid` is the width it
 * is drawn on, the line 2 of it: a bigger grid draws a finer line.
 *
 * A ring steps with a stream of values and eases only a change that stands
 * alone: its arc repaints on the main thread, and on a small ring a step can't
 * be seen.
 */
export function ProgressRing({
  value,
  grid = 16,
  className,
  trackClassName = "text-white/15",
  ...props
}: { value?: number | null; grid?: number; trackClassName?: string } & React.ComponentProps<"svg">) {
  const m = useFollow(value)
  const sweeping = m.value == null
  const arc = m.value ?? m.pos
  const c = grid / 2
  return (
    <svg
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={m.value != null ? Math.round(m.value * 100) : undefined}
      viewBox={`0 0 ${grid} ${grid}`}
      className={cn("size-4 shrink-0 overflow-visible", className)}
      {...props}
    >
      <circle cx={c} cy={c} r={c - 2} fill="none" stroke="currentColor" strokeWidth={2} className={trackClassName} />
      <circle
        cx={c}
        cy={c}
        r={c - 2}
        pathLength={100}
        fill="none"
        stroke="var(--accent)"
        strokeWidth={2}
        strokeLinecap="round"
        strokeDasharray={`${Math.max(3, arc * 100)} 100`}
        className="origin-center -rotate-90 drop-shadow-[0_0_6px_var(--accent)] transition-[stroke-dasharray,opacity]"
        style={{
          opacity: sweeping ? 0 : 1,
          transitionDuration: `${m.ease === "out" ? "var(--dur-slow)" : "0ms"}, var(--dur-med)`,
          transitionTimingFunction: "var(--ease-out), linear",
        }}
      />
      {/* The spinner mounts the first time the ring spins, is paused under a value and crossfades with the arc. */}
      <g className="transition-opacity duration-[var(--dur-med)]" style={{ opacity: sweeping ? 1 : 0 }}>
        {m.swept && (
          <circle
            cx={c}
            cy={c}
            r={c - 2}
            pathLength={100}
            fill="none"
            stroke="var(--accent)"
            strokeWidth={2}
            strokeLinecap="round"
            strokeDasharray="25 75"
            // Under reduced motion the spinner is a whole circle that fades instead of turning, at the bar's
            // faint tint and unlit, so it never reads as a full ring.
            className="origin-center animate-spin drop-shadow-[0_0_6px_var(--accent)] [animation-duration:1.4s] motion-reduce:drop-shadow-none motion-reduce:[stroke-dasharray:100_0] motion-reduce:[stroke:var(--accent-line)]"
            style={{ animationPlayState: sweeping ? "running" : "paused" }}
          />
        )}
      </g>
    </svg>
  )
}
