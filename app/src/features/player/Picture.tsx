import type * as React from "react"
import { useEffect, useState } from "react"
import type { VideoStream } from "@/bindings"
import { aspectOf, pictureStyle, sane } from "./picture-shape"

/**
 * The episode's picture, as large as the stage allows. Its shape starts as
 * the file's and becomes that of what actually plays once it has loaded: a
 * preview copy is upright and has square pixels.
 */
export function PlayerPicture({
  videoRef,
  src,
  video,
}: {
  videoRef: React.RefObject<HTMLVideoElement | null>
  src: string
  /** The file's video stream, as probed. */
  video: VideoStream | null | undefined
}) {
  const [playing, setPlaying] = useState<number | null>(null)
  useEffect(() => {
    const v = videoRef.current
    if (!v) return
    const read = () => setPlaying(v.videoHeight > 0 ? sane(v.videoWidth / v.videoHeight) : null)
    // It may have loaded before this effect ran.
    if (v.readyState >= HTMLMediaElement.HAVE_METADATA) read()
    v.addEventListener("loadedmetadata", read)
    return () => v.removeEventListener("loadedmetadata", read)
  }, [videoRef, src])
  const ar = playing ?? aspectOf(video)
  return (
    <video
      ref={videoRef}
      src={src}
      preload="auto"
      // The asset protocol answers with CORS headers; keeps frames readable for canvas grabs.
      crossOrigin="anonymous"
      playsInline
      disablePictureInPicture
      style={pictureStyle(ar)}
      className="relative max-h-full max-w-full rounded-2xl bg-black shadow-[0_50px_120px_-40px_rgb(0_0_0/1),0_0_0_1px_rgb(255_255_255/0.07)]"
    />
  )
}
