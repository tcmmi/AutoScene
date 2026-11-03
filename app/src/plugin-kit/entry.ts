// The kit's build entry (vite.kit.config.ts): sdk.js is the SDK as the
// global `AutoScene`, and its CSS comes out beside it as theme.css. The SDK
// itself doesn't import the CSS, so its declarations (templates/types) name
// no file they don't have.
import "./theme.css"

export { connect, version } from "./sdk"
