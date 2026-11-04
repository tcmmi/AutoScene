// The UI's catalogs type `t()`: an unknown key, a missing `{{variable}}` or a
// misspelled one is a type error (English is the source, src/locales/en).
import "i18next"
import type { en } from "@/locales"

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "common"
    resources: typeof en
    strictKeyChecks: true
    strictInterpolationChecks: true
  }
}
