import { convertFileSrc } from "@tauri-apps/api/core"
import { open, save } from "@tauri-apps/plugin-dialog"
import type { MediaInfo, MediaItem } from "@/bindings"
import { fmt, t } from "./i18n"
import { isTauri } from "./platform"

/** URL the webview can load for a local file (asset protocol). The mock backend already returns URLs. */
export function assetUrl(path: string): string {
  return isTauri ? convertFileSrc(path) : path
}

export function fileStem(name: string): string {
  const i = name.lastIndexOf(".")
  return i > 0 ? name.slice(0, i) : name
}

/** Scene-release words that say nothing about the content. */
const RELEASE_NOISE =
  /^(2160p|1440p|1080p|1080i|720p|576p|480p|4k|uhd|sdr|hdr|hdr10\+?|dv|dovi|bluray|blu-ray|bdrip|brrip|bdremux|remux|web|web-?dl|webrip|hdtv|dvdrip|dvd|x26[45]|h\.?26[45]|hevc|avc|av1|vc-?1|mpeg-?2|aac\d?(\.\d)?|ac3|eac3|e-ac-3|ddp?\d?(\.\d)?|dd\+|dts|dts-hd|dts-x|ma|truehd|atmos|flac|opus|lpcm|10bit|10-bit|8bit|8-bit|proper|repack|internal|extended|unrated|remastered|multi|dual|dual-audio|esubs?|subs?|subbed|dubbed|nf|amzn|dsnp|hmax|atvp|cr)$/i

/**
 * What a file is about, without the release clutter:
 * `Breaking Bad S01E01 1080p BluRay REMUX AVC [...] (ARRØW)` → `Breaking Bad`,
 * `The.Office.S02E03.Office.Olympics.720p.WEB-DL` → `Office Olympics`,
 * `[SubsPlease] Frieren - 03 (1080p) [ABCD]` → `Frieren`.
 * The episode's own title wins over the show's when the name has one.
 */
export function releaseTitle(fileName: string): string {
  const stem = fileStem(fileName)
  const words = stem
    .replace(/\[[^\]]*\]|\([^)]*\)|\{[^}]*\}/g, " ")
    .replace(/[._]+/g, " ")
    .replace(/\s+-\s+/g, " - ")
    .split(/\s+/)
    .filter(Boolean)
  const isEpisode = (w: string) => /^(s\d{1,2}\s?e\d{1,4}(-?e\d{1,4})?|\d{1,2}x\d{2,3}|e\d{1,4}|ep\d{1,4})$/i.test(w)
  const isYear = (w: string) => /^(19|20)\d\d$/.test(w)
  const until = (list: string[]) => {
    // A year ends the title only where the release info starts ("Blade Runner 2049 2017 2160p").
    const i = list.findIndex(
      (w, k) => RELEASE_NOISE.test(w) || (isYear(w) && (k + 1 === list.length || RELEASE_NOISE.test(list[k + 1]))),
    )
    return (i < 0 ? list : list.slice(0, i)).join(" ").replace(/^[\s-]+|[\s-]+$/g, "")
  }
  let at = words.findIndex(isEpisode)
  // Anime style: "Show - 03".
  if (at < 0) at = words.findIndex((w, i) => /^\d{1,4}$/.test(w) && words[i - 1] === "-")
  if (at < 0) return until(words) || stem
  const series = until(words.slice(0, at))
  const episode = until(words.slice(at + 1))
  return episode || series || stem
}

/** "S01E03 · Pilot", or just the clean name for unnumbered media. */
export function mediaTitle(m: Pick<MediaItem, "label" | "fileName">): string {
  const name = releaseTitle(m.fileName)
  return m.label ? `${m.label} · ${name}` : name
}

const CODECS: Record<string, string> = {
  h264: "H.264",
  hevc: "HEVC",
  av1: "AV1",
  vp9: "VP9",
  vp8: "VP8",
  prores: "ProRes",
  dnxhd: "DNxHR",
  mpeg2video: "MPEG-2",
  mpeg4: "MPEG-4",
  vc1: "VC-1",
}

export function codecLabel(codec: string): string {
  return CODECS[codec] ?? codec.toUpperCase()
}

export function containerLabel(info: MediaInfo): string {
  const c = info.container.split(",")[0]
  return { matroska: "MKV", mov: "MP4/MOV", mpegts: "MPEG-TS", avi: "AVI", webm: "WebM" }[c] ?? c.toUpperCase()
}

/** Short format badges an editor cares about. */
export function formatFlags(m: MediaItem): string[] {
  const flags: string[] = []
  const v = m.info?.video
  if (!v) return flags
  if (v.hdr === "pq") flags.push("HDR10")
  if (v.hdr === "hlg") flags.push("HLG")
  if (v.hdr === "dolby_vision") flags.push("Dolby Vision")
  if (v.bitDepth > 8) flags.push(t("media:flags.bitDepth", { n: v.bitDepth }))
  if (m.vfr) flags.push("VFR")
  if (m.interlaced) flags.push(t("media:flags.interlaced"))
  return flags
}

type AudioStream = MediaInfo["audio"][number]

/**
 * The codes AutoScene names: ISO 639-2 (both /B and /T forms) and 639-1.
 * Mirrors `as_media::lang`. The names come from the platform, in the UI's
 * language (`fmt.languageName`); any other code reads as the code.
 */
const NAMED_LANGUAGES = new Set([
  ...["eng", "en", "jpn", "ja", "deu", "ger", "de", "fra", "fre", "fr", "spa", "es", "ita", "it", "por", "pt"],
  ...["rus", "ru", "kor", "ko", "zho", "chi", "zh", "hin", "hi", "ara", "ar", "tur", "tr", "pol", "pl"],
  ...["nld", "dut", "nl", "swe", "sv", "nor", "no", "dan", "da", "fin", "fi", "ces", "cze", "cs", "hun", "hu"],
  ...["ell", "gre", "el", "heb", "he", "tha", "th", "vie", "vi", "ind", "id", "ukr", "uk", "ron", "rum", "ro"],
  ...["tam", "ta", "tel", "te"],
])

/** The /B forms older platform data may not know: asked for by their /T twin. */
const TERMINOLOGIC: Record<string, string> = { ger: "deu", fre: "fra", chi: "zho", dut: "nld", gre: "ell", rum: "ron", cze: "ces" }

export function languageName(code: string | null | undefined): string | null {
  if (!code) return null
  const c = code.toLowerCase().split(/[-_]/)[0]
  return (NAMED_LANGUAGES.has(c) ? fmt.languageName(TERMINOLOGIC[c] ?? c) : undefined) ?? code.toUpperCase()
}

// i18n-ignore: codec names are product names, the same in every language
const AUDIO_CODECS: Record<string, string> = {
  ac3: "Dolby Digital",
  eac3: "Dolby Digital Plus",
  truehd: "Dolby TrueHD",
  dts: "DTS",
  aac: "AAC",
  flac: "FLAC",
  alac: "ALAC",
  opus: "Opus",
  vorbis: "Vorbis",
  mp3: "MP3",
  mp2: "MP2",
}

function audioCodecLabel(a: AudioStream): string {
  // DTS profiles are what people recognise ("DTS-HD MA", "DTS-HD HRA", "DTS:X").
  if (a.codec === "dts" && a.profile) return a.profile
  if (a.codec.startsWith("pcm_")) return "PCM"
  return AUDIO_CODECS[a.codec] ?? a.codec.toUpperCase()
}

function channelLabel(n: number): string {
  return n === 1
    ? t("media:audio.mono")
    : n === 2
      ? t("media:audio.stereo")
      : n === 6
        ? "5.1"
        : n === 8
          ? "7.1"
          : n === 3
            ? "2.1"
            : t("media:audio.channels", { n })
}

/** "English · DTS-HD MA · 5.1" — language first, so the right track is obvious at a glance. */
export function audioLabel(a: AudioStream): string {
  const parts = [
    languageName(a.language) ?? t("media:audio.track", { n: a.ordinal + 1 }),
    audioCodecLabel(a),
    channelLabel(a.channels),
  ]
  if (a.commentary) parts.push(t("media:audio.commentary"))
  return parts.join(" · ")
}

const squash = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "")

/** Track picker option; the file's own title is shown underneath when it says something the label doesn't. */
export function audioOption(a: AudioStream): { value: string; label: string; description?: string } {
  const label = audioLabel(a)
  const title = a.title?.trim()
  const redundant = !title || squash(label).includes(squash(title))
  return { value: String(a.ordinal), label, description: redundant ? undefined : title }
}

/** The track a media plays and exports with (the backend fills `audioOrdinal` at indexing). */
export function currentAudio(m: Pick<MediaItem, "audioOrdinal" | "info">): number | undefined {
  const audio = m.info?.audio ?? []
  return m.audioOrdinal ?? audio.find((a) => a.default)?.ordinal ?? audio[0]?.ordinal
}

/** The codes of the audio languages offered as the automatic choice (ISO 639-2), and "" for each file's own track. */
const PREFERRED_CODES = ["eng", "jpn", "deu", "fra", "spa", "ita", "por", "kor", "zho", "hin", "rus", ""] as const

/**
 * Audio languages offered as the automatic choice, by ISO 639-2 code. Each
 * `label` is looked up when it is read, in the UI's language.
 */
export const PREFERRED_AUDIO_LANGUAGES: { value: string; label: string }[] = PREFERRED_CODES.map((value) => ({
  value,
  get label() {
    return value ? (languageName(value) ?? value.toUpperCase()) : t("media:audio.defaultTrack")
  },
}))

const VIDEO_EXTENSIONS = ["mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "ts", "m2ts", "mts", "mpg", "mpeg", "vob", "flv", "ogv", "3gp"]

export async function pickVideoFiles(): Promise<string[]> {
  const picked = await open({ multiple: true, filters: [{ name: t("media:dialog.videoFilter"), extensions: VIDEO_EXTENSIONS }] })
  return picked ?? []
}

/** One video file. */
export async function pickVideoFile(title?: string): Promise<string | null> {
  const picked = await open({ multiple: false, title, filters: [{ name: t("media:dialog.videoFilter"), extensions: VIDEO_EXTENSIONS }] })
  return typeof picked === "string" ? picked : null
}

/** A plugin package (`.asp`). */
export async function pickPluginFile(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    title: t("media:dialog.installPlugin"),
    filters: [{ name: t("media:dialog.pluginFilter"), extensions: ["asp"] }],
  })
  return typeof picked === "string" ? picked : null
}

export async function pickFolder(title?: string): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false, title })
  return typeof picked === "string" ? picked : null
}

/** Where to save a file of `filter`'s kind, starting at `defaultPath`; `null` when cancelled. A file there is replaced (the dialog asks first). */
export async function pickSaveFile(title: string, defaultPath: string, filter: { name: string; extensions: string[] }): Promise<string | null> {
  return (await save({ title, defaultPath, filters: [filter] })) ?? null
}

/** Last path component, for titles suggested from a dropped folder. */
export function baseName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? path
}
