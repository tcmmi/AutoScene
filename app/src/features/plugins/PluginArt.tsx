import { Artwork } from "@/components/ui/artwork"
import { pluginPicture } from "./plugin-items"

/**
 * A plugin's art, 2:1: its banner with its name on the dark bottom; else its
 * icon blurred into a backdrop, the sharp icon on glass and the name; else its
 * name set large as the picture, as a TV app shows a title without artwork.
 * Until its banner is in hand, the icon or the name shows. A catalog entry
 * has no banner: its icon or its name. Only checked `data:` pictures draw.
 */
export function PluginArt({
  name,
  icon: iconSrc,
  banner,
  className,
}: {
  name: string
  icon: string | null
  banner?: string | null
  className?: string
}) {
  const picture = pluginPicture(banner)
  const icon = pluginPicture(iconSrc)
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
            <div className="display truncate text-2xl text-white drop-shadow-[0_2px_12px_rgb(0_0_0/0.6)]">{name}</div>
          </div>
        </>
      ) : (
        <div className="absolute inset-x-0 bottom-0 p-5">
          <div className="display line-clamp-3 text-3xl break-words text-fg-1">{name}</div>
        </div>
      )}
    </Artwork>
  )
}
