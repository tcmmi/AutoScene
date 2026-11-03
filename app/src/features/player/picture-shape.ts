import type * as React from "react"
import type { VideoStream } from "@/bindings"

/** What a picture without a usable size is shown at. */
const WIDE = 16 / 9
/** Narrower or wider than this is a broken or crafted size, not a picture. */
const MIN_ASPECT = 1 / 4
const MAX_ASPECT = 4

/** `ar` when it is a picture's shape, otherwise `null`. */
export function sane(ar: number): number | null {
  return Number.isFinite(ar) && ar >= MIN_ASPECT && ar <= MAX_ASPECT ? ar : null
}

/**
 * The shape the file's picture is shown at: its width over its height, with
 * the pixels' own shape (a DVD's 720×480 at 32:27 is 16:9) and turned for a
 * file that is stored on its side (a phone clip). A size that is missing,
 * zero or absurd counts as 16:9.
 */
export function aspectOf(video: Pick<VideoStream, "width" | "height" | "sar" | "rotation"> | null | undefined): number {
  if (!video || !(video.width > 0) || !(video.height > 0)) return WIDE
  // A pixel shape with a zero part means "not stated": square.
  const sar = video.sar && video.sar.num > 0 && video.sar.den > 0 ? video.sar.num / video.sar.den : 1
  const ar = (video.width * sar) / video.height
  const turned = Math.abs(Math.round(video.rotation ?? 0)) % 180 === 90
  return sane(turned ? 1 / ar : ar) ?? WIDE
}

/**
 * The picture's box: the largest one at its shape that fits the stage (the
 * nearest size container), small files and preview copies scaled up. Where
 * container units aren't known, the width is dropped and the `max-*` classes
 * keep the picture at its own size inside the stage, as before.
 */
export function pictureStyle(ar: number): React.CSSProperties {
  return { aspectRatio: ar, width: `min(100cqw, calc(100cqh * ${ar}))` }
}
