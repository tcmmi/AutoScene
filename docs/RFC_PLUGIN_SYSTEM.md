# RFC & Architectural Specification: AutoScene Plugin Framework (.asp) & Extension SDK

**Status:** Prototype / Request for Comments (RFC)  
**Authors:** @moongetsu & community contributors  
**Target:** Core Maintainers & AI Architectural Agent (Claude / Codex)  
**Tracking Pull Request:** https://github.com/tcmmi/AutoScene/pull/93  

---

## 1. Executive Summary & Author's Vision

### 1.1 Direct Note from the Author (@moongetsu) to the AI Reviewer (Claude & Core Team)

> *"This is just a prototype that I made so you can clearly grasp the vision of what I want to achieve with plugins in AutoScene.*
> 
> *The AI (Claude or whoever reviews this) should make a deep check into it, refine it, and make sure that it can be way, way better. This implementation is here to teach what I meant with the whole plugin system:*
> 
> - *I want to **let developers do anything via a custom-made UI library**.*
> - *I want them to be able to **add new settings, new items in the navbar, and entirely new pages** (for example, a dedicated full-screen page where users can upload videos and run AI interpolations, just as an example).*
> - *I want plugins to be able to **leverage external APIs and services** (such as Discord notifications, GitHub updates, external ML pipelines, etc.).*
> - *I want developers to have the **freedom to create amazing things** inside AutoScene, pushing the application forward without bloating the core app or making it heavy. That is the ultimate goal, something like After Effects (`.aex`) plugins.*
> 
> *This can be an absolute **game changer** for AutoScene: developers can build custom tools and continuously improve the app alongside the core team. It is a heavy task, but doing it right now while the application is still in beta is 100% worth it.*
> 
> *The `.asp` file format must be a custom, safe package for AutoScene. Developers will share plugins via our Discord forum, and our team/automated bots will verify that the code is safe at every update, marking approved plugins as trusted on the download page.*
> 
> *If Claude has more ideas to elevate this further, please do them without hesitating. I'm really looking forward to your work!"*

---

### 1.2 High-Level Architectural Goal
The objective is to achieve an extensibility standard on par with professional creative tools like **Adobe After Effects (`.aex`)**, **Figma Plugins**, or **VS Code Extensions (`.vsix`)**, while preserving AutoScene’s hallmark performance, minimal memory footprint, and tight security boundaries.

---

## 2. Core Pillars & Capability Requirements

### 2.1 The Custom UI Library & Host Extension Hooks
We want plugin authors to possess expressive freedom to build first-class user experiences without hardcoding them into the AutoScene monolith:
- **Declarative Host Contribution Points:**
  - Dynamic button injection into the Top Navigation Bar (`navButtons`).
  - Native Left Navigation Rail entries (`railButtons`).
  - Context menu hooks across timeline clips, exports, and library items (`menus`).
  - Configuration panels in AutoScene Settings via declarative schema definitions (`settings`).
- **Dedicated Custom Full-Screen Pages & Views:**
  - The framework must allow plugins to register first-class route mounts (e.g., `/plugins/:pluginId/*`).
  - Example: A plugin offering specialized video frame interpolation, optical flow deadframe repair, or dataset curation tools can present its own dedicated multi-step UI inside the AutoScene window shell.
- **AutoScene Component Primitives:**
  - Expose an `@autoscene/plugin-sdk` or standard Web Component / React component bridge matching AutoScene's native styling (dark theme, phosphor icons, high-density layouts, custom sliders, switches, and progress rings).

### 2.2 IPC, Asynchronous Pipelines & External Integrations
- **Subprocess & Out-of-Process Execution:**
  - Plugin logic executes decoupled from the main Tauri UI thread (via background child processes, CLI scripts, Python environments, or WebAssembly).
  - Streamed JSON-RPC / IPC protocols across standard input/output for live progress bars, frame counts, and cancellation tokens (`CancelToken`).
- **Third-Party Services & APIs:**
  - Out-of-the-box support for network-enabled extensions (e.g., Discord webhook web-alerts, GitHub release checkers, cloud storage uploaders, custom REST/gRPC backends) governed by a declared permissions manifest.

### 2.3 The `.asp` Container Specification & Cryptographic Security
Distributing executable code carries security risks. A user downloading an archive renamed to `.asp` must never unknowingly run arbitrary malware.

The `.asp` file format implements a multi-layer verification pipeline:
1. **Container Isolation & Safe Extraction:**
   - Archive extraction with Zip-Slip path traversal rejection (`..`, absolute paths).
   - Automatic execution permission restoration (`chmod 0o755`) for declared scripts/binaries on Unix platforms.
2. **Ed25519 Cryptographic Signatures & SHA-256 Manifest:**
   - Every `.asp` contains a `META-INF/MANIFEST.json` containing SHA-256 hashes of every payload file.
   - A companion `META-INF/SIGNATURE.sig` contains an Ed25519 signature over the manifest, the developer's public key, and identity metadata.
3. **Trust Tiers:**
   - 🟢 **Verified Official:** Signed with AutoScene's built-in master key.
   - 🔵 **Community Signed:** Valid cryptographic signature from a registered developer key (fingerprint: `SHA256:...`).
   - 🟠 **Unsigned / Self-Signed:** Triggers a security confirmation dialog where users must explicitly acknowledge the lack of verification.
   - 🔴 **Tampered / Corrupted:** File checksum mismatch or invalid signature. Installation is completely blocked.
4. **Pre-Install Security Modal:**
   - Inspects package contents without unpacking to disk.
   - Displays requested capabilities (`filesystem:write-export`, `network:webhook`, `subprocess`, etc.) and author identity.

---

## 3. Discord Community Distribution & Automated CI/CD Review Pipeline

The long-term distribution roadmap envisions a community-driven marketplace:
1. **Developer Publication:** Developers export `.asp` packages via GitHub Actions (`docs/templates/package-asp.yml`) and share them in a dedicated Discord Forum or repository.
2. **Automated Security Bot / Scanner:**
   - A CI/Discord verification bot automatically unpacks the `.asp`, runs static analysis (SAST) for malicious calls, verifies cryptographic signatures, and generates an automated audit report.
3. **Approval & Trusted Status:**
   - Once approved by maintainers, the plugin is cataloged in the official directory as trusted.
   - Subsequent updates trigger incremental diff audits.

---

## 4. Current Prototype Implementation Artifacts

| Component | Path | Description |
| :--- | :--- | :--- |
| **Packaging & Crypto Engine** | `crates/as-export/src/extension.rs` | Implements `package_asp_signed`, `inspect_asp`, `install_asp`, and Ed25519 validation. |
| **Re-exports & Aliases** | `crates/as-export/src/lib.rs` | Exposes plugin types, manifest structs, and signature verification helpers. |
| **Path Configuration** | `crates/as-core/src/paths.rs` | Resolves `plugins/` directory with backward-compatible fallback to `extensions/`. |
| **Tauri IPC Layer** | `app/src-tauri/src/commands/system.rs` | Commands for listing, testing, inspecting, packaging, and installing `.asp` packages. |
| **Frontend UI Page** | `app/src/features/extensions/ExtensionsPage.tsx` | Route `/plugins` with filter pills, status badges, test trigger, and uninstallation. |
| **Security Modal** | `app/src/features/extensions/PluginInstallModal.tsx` | Pre-installation inspection modal reviewing signature validity and declared permissions. |
| **Reference Plugin** | `examples/plugins/amverge-deadframes/` | Sample optical flow deadframe compaction plugin with `plugin.json`. |
| **CI/CD Template** | `docs/templates/package-asp.yml` | GitHub Actions workflow for building and signing `.asp` releases. |

---

## 5. Architectural Inquiries & Future Work for Claude / Maintainers

1. **Sandboxed Webview / iframe Hosting for Custom Pages:**
   - Should dedicated plugin pages run in an isolated Webview / iframe context with a scoped RPC bridge to prevent unauthorized DOM manipulation of the host shell?
2. **Plugin SDK Package:**
   - Should we extract TypeScript definitions and React UI primitives into an npm package (e.g. `@autoscene/sdk`) so authors can develop plugins with full autocomplete and type safety?
3. **Dynamic Route Registration:**
   - What is the most resilient pattern in TanStack Router to register runtime plugin routes (`/plugins/:id/*`) without rebuilding the host client?
4. **Wasm / Wasi Micro-runtimes:**
   - Should computationally lightweight plugins (metadata filters, timecode translators, format converters) be supported via WebAssembly for instantaneous startup and strict sandboxing?

---
*Contributions, architectural feedback, and PR reviews are welcome. Please treat this document as an active RFC.*

