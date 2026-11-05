// The media namespace: what a video file is called on screen (its audio tracks, format flags) and the OS dialogs
// that pick one (docs/DESIGN.md, Copy). English source, typed. Comments are notes for translators.
// Codec and container names (ProRes, DNxHR, HEVC, Dolby Digital, MKV…) are product names and stay as they are;
// a language's own name comes from the platform's names for languages, not from here.
export default {
  /** How an audio track is named, as "English · Dolby Digital · Stereo". */
  audio: {
    mono: "Mono",
    stereo: "Stereo",
    /** A layout without a common name: `n` is the number of channels. */
    channels: "{{n}} ch",
    /** A track that names no language: `n` is its place in the file, from 1. */
    track: "Track {{n}}",
    commentary: "Commentary",
    /** The choice that leaves the track to each file. */
    defaultTrack: "Each file's default track",
  },
  /** Short badges on a file: the picture's format. */
  flags: {
    /** `n` is the bits of each colour: "10-bit". */
    bitDepth: "{{n}}-bit",
    interlaced: "Interlaced",
  },
  /** The system's file dialogs. */
  dialog: {
    /** The name of a group of file types in a dialog's list. */
    videoFilter: "Video",
    pluginFilter: "AutoScene plugin",
    /** The window title of the dialog that picks a plugin's package. */
    installPlugin: "Install a plugin",
  },
} as const
