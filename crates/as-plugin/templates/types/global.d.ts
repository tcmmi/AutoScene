// Written by hand, beside the declarations `npm run build:kit` makes: the
// global `AutoScene` that /_as/sdk.js defines in a plugin page.
import type { AutoSceneSdk } from "./sdk"

declare global {
  const AutoScene: AutoSceneSdk
}

export {}
