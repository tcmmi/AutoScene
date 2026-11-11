/** The catalog's words for what each model is for, by the manifest's id (`models/manifest.toml`). */
export const PURPOSE = {
  transnetv2: "about.models.purpose.transnetv2",
  "scrfd-10g": "about.models.purpose.scrfd",
  "arcface-glintr100": "about.models.purpose.arcface",
  "scrfd-2.5g": "about.models.purpose.yunet",
  "sface-2021dec": "about.models.purpose.sface",
  "person-ppyoloe": "about.models.purpose.personDetector",
  "person-reid-0277": "about.models.purpose.personLook",
  "anime-head-v2m": "about.models.purpose.animeHead",
  "anime-person-v1m": "about.models.purpose.animePerson",
  "anime-ccip-b36": "about.models.purpose.animeIdentity",
  "anime-berkeley-dino-l": "about.models.purpose.animeIdentityBest",
} as const
