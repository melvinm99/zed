# Flutter support in Zed — Design

**Date:** 2026-07-16
**Status:** Approved (design), pending implementation plan
**Scope:** Full-featured Flutter development in the Zed fork: Dart editing, Flutter debugging, hot reload, hot restart. Delivered fully built-in (no external extension dependency), on the `melvinm99/zed` fork, producing a macOS `.app`.

---

## 1. Goal & constraints

Add Flutter development support to Zed with three capabilities the user explicitly requested:

1. **Debugging** — breakpoints, stepping, variables, via the Debug Adapter Protocol (DAP).
2. **Hot restart** — full app restart preserving no state.
3. **Hot reload** — inject changed sources and rebuild the widget tree, preserving state.

Constraints and decisions taken during brainstorming:

- **Fully built-in / self-contained.** All layers live in the Zed fork's Rust tree. No dependency on an external Zed extension for any layer. (User decision.)
- **SDK is not managed by Zed.** The Flutter SDK is multi-GB; Zed will *discover* the user's installed `flutter`/`dart` (on `PATH`, with a settings override) rather than download or install it.
- **Local sessions only** for v1. No collab/remote debugging of Flutter — this is what lets Component 3 avoid touching the proto/collab layer.
- **Never auto-commit** — per repo owner instruction, code and docs are written to disk but committing is left to the user.

---

## 2. Verified technical facts (basis for this design)

These were verified against source, not assumed:

| Fact | Source |
|------|--------|
| Zed's debugger is a pure standard-DAP client; requests are statically bound to Rust types implementing `dap_types::requests::Request`. There is **no** generic "send arbitrary custom request" API. | `crates/dap/src/client.rs:100`, `crates/project/src/debugger/dap_command.rs:19` |
| `dap_types::requests::Request` is an **unsealed** 3-item trait: `const COMMAND: &'static str`, `type Arguments`, `type Response` (serde-bounded). Implementable on a local type via orphan rules. | dap-types `src/requests.rs:8` (pinned rev `1b461b31…`) |
| `Session::request<R>` requires `R: LocalDapCommand + PartialEq + Eq + Hash` only — **no** collab `DapCommand`/proto impl needed for local sessions. | `crates/project/src/debugger/session.rs:1750` |
| `RestartCommand { raw: Value }` proves arbitrary JSON args flow cleanly through the typed path — the pattern to mirror. | `dap_command.rs:748` |
| Flutter custom requests are exactly **`hotReload`** and **`hotRestart`**, each with `{ "reason": "manual" }`. `isFullRestart = command == 'hotRestart'`. | flutter `flutter_adapter.dart:184-205`, `:698-722` |
| The Flutter adapter conditionally advertises `supportsRestartRequest`, so Zed's existing Restart button already triggers a restart; hot **reload** has no standard-DAP equivalent. | `flutter_adapter.dart:420` |
| Debug adapters are registered built-in by implementing `DebugAdapter` (`crates/dap/src/adapters.rs:349`) + one `registry.add_adapter(...)` line. | `crates/dap_adapters/src/dap_adapters.rs:29` |
| Debug-session actions are declared in one `actions!` block and wired in the toolbar builder. | `crates/debugger_ui/src/debugger_ui.rs:29-86`, `:160-245` |
| `script/bundle-mac` exists and produces the release `.app`. | `script/bundle-mac` |

**Open items to confirm at implementation time (not guesses baked into code):**
- Exact `flutter debug_adapter` subcommand spelling (hyphen vs underscore) — verify with `flutter --help`.
- tree-sitter-dart grammar source repo + query compatibility with Zed's grammar version.
- The response body shape of `hotReload`/`hotRestart` — use a permissive `serde_json::Value` response type to avoid deserialization failures on empty bodies.

---

## 3. Architecture

Three layers, all built into the fork, sharing one SDK-discovery concern.

```
┌─ Editing ───────────────┐  ┌─ Debugging ─────────────┐  ┌─ Live-edit ─────────────┐
│ Dart language (built-in)│  │ Flutter DAP adapter     │  │ Hot Reload / Hot        │
│ • tree-sitter-dart      │  │ (built-in)              │  │ Restart (core)          │
│ • dart language-server  │  │ • `flutter debug_adapter`│ │ • hotReload/hotRestart  │
│   (from Flutter SDK)    │  │ • device via toolArgs    │  │   custom DAP requests   │
└─────────────────────────┘  └──────────────────────────┘  └─────────────────────────┘
           │                            │                              │
           └──── discover Flutter/Dart SDK on PATH (or settings) ──────┘
```

---

## 4. Components

### Component 1 — Dart language (`crates/languages/src/dart.rs`)

- New `LspAdapter` launching the Dart analysis server: `dart language-server --protocol=lsp`. The same server serves Flutter projects (it detects Flutter via `pubspec.yaml`).
- `dart` binary resolved from the discovered SDK (`<flutter>/bin/cache/dart-sdk/bin/dart` when only `flutter` is on `PATH`, else `dart` directly).
- Language config, `tree-sitter-dart` grammar, and highlight/indent/bracket queries under `crates/languages/src/dart/`, registered exactly as `go`/`python` are.
- Registered in `crates/languages/src/lib.rs`.

**Note:** This is the heaviest layer and the one with the least intrinsic reason to be in-core. It is in-core by user choice. If upstream-rebase pain appears, this layer is the clean descope into a standalone extension **without touching Components 3–4**.

### Component 2 — Flutter DAP adapter (`crates/dap_adapters/src/flutter.rs`)

- A struct implementing the `DebugAdapter` trait (`crates/dap/src/adapters.rs:349`), added via one `registry.add_adapter(...)` line in `crates/dap_adapters/src/dap_adapters.rs:29` and a module declaration at the top of that file.
- **Binary discovery:** locate `flutter` (PATH → settings override). Spawn the DAP server subprocess: `flutter debug_adapter` (exact spelling verified at build time).
- **Launch config construction** from the debug scenario:
  - `program` — entrypoint (default `lib/main.dart`), `cwd` — project root.
  - `toolArgs` — `["-d", "<deviceId>"]` for device targeting; extendable with user `toolArgs`.
  - `noDebug: true` when the scenario is a non-debug run.

### Component 3 — Hot Reload / Hot Restart (core, mirrors `RestartCommand`)

All self-contained in the `project` crate + `debugger_ui`. **No dap-types fork, no proto changes.**

- **`crates/project/src/debugger/dap_command.rs`:**
  - Two local request marker types implementing `dap::requests::Request`:
    ```rust
    struct HotReloadRequest;
    impl dap::requests::Request for HotReloadRequest {
        const COMMAND: &'static str = "hotReload";
        type Arguments = HotReloadArguments;   // { reason: String }
        type Response = serde_json::Value;     // permissive; empty body -> Null
    }
    struct HotRestartRequest;  // COMMAND = "hotRestart"; same Arguments/Response
    ```
  - `HotReloadCommand` / `HotRestartCommand` structs (derive `Debug, PartialEq, Eq, Hash`) implementing `LocalDapCommand`, with `to_dap()` producing `{ reason: "manual" }`.
- **`crates/project/src/debugger/session.rs`:** `Session::hot_reload()` / `hot_restart()` calling the existing private `request(...)` path (as `restart_session` does at `:2181`).
- **`crates/debugger_ui/src/debugger_ui.rs`:** two new actions `debugger::HotReload` / `debugger::HotRestart` in the `actions!` block (`:29`), wired to handlers in the toolbar builder (`:160`).
- **`crates/debugger_ui/src/debugger_panel.rs`:** two toolbar buttons.
- **Gating:** buttons are shown/enabled only when (a) the running adapter is the Flutter adapter, and (b) the app is live — tracked by receiving the adapter's `flutter.appStarted` custom event. Sending before the app is live returns an adapter error.
- The existing generic **Restart** button is unchanged (standard `restart` request / terminate+relaunch fallback), giving the user three escalating levels: **Hot Reload → Hot Restart → cold Restart**.

### Component 4 — Device targeting

- **Config field (baseline, zero-UI):** a `deviceId` field in the `debug.json` scenario config, mapped by Component 2 into `toolArgs: ["-d", <id>]`.
- **Picker UI:** a Zed quick-pick built on the existing `Picker` component, populated by parsing `flutter devices --machine` (JSON). Shown before launch when the scenario has no `deviceId`; the selection is injected into `toolArgs`. If exactly one device is available, auto-select it.

---

## 5. Data flow (launch → reload)

1. User runs the Flutter debug scenario. If no `deviceId`, the device picker (Component 4) resolves one.
2. Component 2 spawns `flutter debug_adapter`; Zed sends the DAP `launch` request with device `toolArgs`.
3. `flutter run` starts the app; the adapter emits `flutter.appStarted` → Component 3 enables the Reload/Restart buttons.
4. **Hot Reload** button → `Session::hot_reload()` → DAP custom request `hotReload { reason: "manual" }` → adapter injects sources & rebuilds the widget tree (state preserved).
5. **Hot Restart** button → `hotRestart { reason: "manual" }` → full restart (state lost).
6. Breakpoints/stepping/variables flow through the standard DAP path unchanged.

---

## 6. Building the macOS app

- **Prerequisites:** Xcode + Command Line Tools; Rust toolchain (pinned by `rust-toolchain.toml`, installed via `rustup`); the Flutter SDK on `PATH` for actually running Flutter apps.
- **Dev / iterate:** `cargo run` (debug) or `cargo run --release` — the fast loop while building this feature.
- **Release bundle:** `script/bundle-mac` → produces `Zed.app`.
- **Gatekeeper caveat:** a fork build is **unsigned and un-notarized**, so macOS quarantines it. Launch via right-click → Open once, or clear the attribute: `xattr -dr com.apple.quarantine /path/to/Zed.app`. Proper distribution signing requires an Apple Developer certificate and is out of scope.

---

## 7. Scope boundaries

**In v1:** Dart editing (LSP + syntax), Flutter debugging, hot reload, hot restart, device targeting (config field + picker), macOS build instructions.

**Out of v1:**
- Flutter DevTools launch and widget inspector integration.
- Dedicated task templates for `flutter test` / `pub get` / `build`, and a first-class non-debug run command (the adapter supports `noDebug`, but no dedicated UI is built).
- Remote/collab debugging of Flutter (local sessions only — the reason Component 3 needs no proto changes).
- Managing/downloading the Flutter SDK.

---

## 8. Testing & verification

- **Unit (required):** a `dap_command` test asserting `HotReloadCommand` / `HotRestartCommand` serialize to `COMMAND == "hotReload"` / `"hotRestart"` with arguments `{ "reason": "manual" }` — mirrors existing `dap_command` tests.
- **Manual (macOS):** create a Flutter counter app, set a breakpoint, launch under the Flutter adapter, hit the breakpoint; edit a widget and Hot Reload (state preserved); Hot Restart (state reset); verify device picker lists `macos`/`chrome`.

---

## 9. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| `flutter debug_adapter` subcommand spelling / flags differ across SDK versions | Verify at implementation time via `flutter --help`; keep the subcommand a single constant. |
| tree-sitter-dart grammar incompatible with Zed's grammar version | Pin a known-good grammar rev; fall back to a minimal config-only language if grammar wiring stalls (editing still works via LSP). |
| `hotReload`/`hotRestart` response body shape varies | Use `serde_json::Value` as the `Response` type. |
| Upstream-rebase drift on Components 1–2 | Language layer is the pre-identified descope-to-extension escape hatch. |
| Buttons fire before app is live → adapter error | Gate on `flutter.appStarted` event. |
