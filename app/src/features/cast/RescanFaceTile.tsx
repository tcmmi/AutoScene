import type * as React from "react"
import { useState } from "react"
import type { FaceTrackItem } from "@/bindings"
import { assetUrl } from "@/lib/media"
import { usePortrait } from "@/lib/queries"
import { cn } from "@/lib/utils"

/**
 * A face as Rescan saw it: the soft thumbnail first, then its exact frame
 * at the source's own quality comes into focus over it (opacity and scale
 * only). `children` lie on the picture.
 */
export function RescanFaceTile({
  pid,
  face,
  className,
  children,
}: {
  pid: string
  face: FaceTrackItem
  className?: string
  children?: React.ReactNode
}) {
  const { data: portrait } = usePortrait(pid, face)
  const [sharp, setSharp] = useState(false)
  return (
    <div className={cn("group relative aspect-[3/4] overflow-hidden rounded-xl bg-inset picture-edge", className)}>
      <img
        src={assetUrl(face.thumb)}
        alt=""
        loading="lazy"
        draggable={false}
        className="absolute inset-0 size-full scale-105 object-cover opacity-90"
      />
      {portrait && (
        <img
          src={assetUrl(portrait)}
          alt=""
          draggable={false}
          onLoad={() => setSharp(true)}
          className={cn(
            "absolute inset-0 size-full object-cover transition-[opacity,transform] duration-[600ms] ease-out",
            sharp ? "scale-100 opacity-100" : "scale-[1.04] opacity-0",
          )}
        />
      )}
      {children}
    </div>
  )
}
