# Flutter Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add fully built-in Flutter development to the Zed fork — Dart editing, Flutter debugging, hot reload, and hot restart — and produce a runnable macOS app.

**Architecture:** Three layers, all in-core: (1) a built-in Dart language (tree-sitter grammar + `dart language-server` LSP), (2) a built-in Flutter DAP adapter that spawns `flutter debug_adapter`, and (3) two new core custom DAP requests (`hotReload`/`hotRestart`) mirroring the existing `RestartCommand`, surfaced as debugger actions/buttons. A device picker feeds the target device into the adapter's launch args. Local debug sessions only (no collab/proto changes).

**Tech Stack:** Rust, `gpui`, Zed's `dap`/`project`/`debugger_ui`/`languages`/`grammars` crates, `dap-types` (external, unmodified), tree-sitter-dart, the user's installed Flutter/Dart SDK.

## Global Constraints

- **No `dap-types` fork.** `dap_types::requests::Request` is unsealed (`const COMMAND`, `type Arguments`, `type Response`); implement it on local types via orphan rules. (dap-types `src/requests.rs:8`)
- **No proto/collab changes.** `Session::request<T>` requires only `T: LocalDapCommand + PartialEq + Eq + Hash`. Local sessions only. (`session.rs:1750`)
- **Do not download the Flutter/Dart SDK.** Discover `flutter`/`dart` via `delegate.which(...)`; error with an install hint if absent.
- **Custom requests are exactly** `hotReload` and `hotRestart`, args `{ "reason": "manual" }`. (`flutter_adapter.dart:184-205`)
- **Never commit on the user's behalf** (repo owner rule). The commit steps below are written for the user/executor to run manually; if the executor is a subagent, it should stage and commit per repo policy only if the owner has enabled it — otherwise stop at staging.
- **Build with** `cargo build -p <crate>`; run targeted tests with `cargo test -p <crate> <name> -- --nocolor`.

---

## File map

**Create:**
- `crates/grammars/src/dart/config.toml` + `highlights.scm`, `brackets.scm`, `indents.scm`, `injections.scm`, `outline.scm` — Dart language config + tree-sitter queries.
- `crates/languages/src/dart.rs` — `DartLspAdapter` (`LspInstaller` + `LspAdapter`).
- `crates/dap_adapters/src/flutter.rs` — `FlutterDebugAdapter` (`DebugAdapter`).

**Modify:**
- `Cargo.toml` (workspace) + `crates/grammars/Cargo.toml` — add `tree-sitter-dart` dep.
- `crates/grammars/src/grammars.rs:16` — register the `dart` native grammar.
- `crates/languages/src/lib.rs` — declare `mod dart;`, build the adapter, add the `LanguageInfo` entry.
- `crates/dap_adapters/src/dap_adapters.rs:1` and `:29` — `mod flutter;` + `registry.add_adapter(...)`.
- `crates/project/src/debugger/dap_command.rs` — add `HotReloadRequest`/`HotRestartRequest` request types + `HotReloadCommand`/`HotRestartCommand` local commands + a serialization test.
- `crates/project/src/debugger/session.rs` — `Session::hot_reload` / `Session::hot_restart`.
- `crates/debugger_ui/src/session/running.rs` — `hot_reload` / `hot_restart` methods delegating to the session.
- `crates/debugger_ui/src/debugger_ui.rs:29` (actions) and `:160` (wiring) — `HotReload`/`HotRestart` actions.
- `crates/debugger_ui/src/debugger_panel.rs` — two toolbar buttons, gated to the Flutter adapter.

---

## Phase 1 — Dart editing (language + LSP)

### Task 1: Register the Dart tree-sitter grammar + language queries

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`), `crates/grammars/Cargo.toml`
- Modify: `crates/grammars/src/grammars.rs:16-43`
- Create: `crates/grammars/src/dart/config.toml`, `crates/grammars/src/dart/highlights.scm`, `.../brackets.scm`, `.../indents.scm`, `.../injections.scm`, `.../outline.scm`

**Interfaces:**
- Produces: a registered grammar named `"dart"` and a loadable language config named `dart` (consumed by Task 2's `LanguageInfo`).

- [ ] **Step 1: Confirm the grammar crate name/version.** Search crates.io for the maintained tree-sitter Dart grammar (`tree-sitter-dart`). Confirm the exact crate name, latest version, and that it exposes a `LANGUAGE` const (newer tree-sitter API) or a `language()` fn (older). Note which, you'll match the call style in Step 3.

Run: `cargo search tree-sitter-dart`
Expected: a crate line like `tree-sitter-dart = "x.y.z"`.

- [ ] **Step 2: Add the dependency.** In workspace `Cargo.toml` under `[workspace.dependencies]`, add a line modeled on the existing `tree-sitter-go` entry:

```toml
tree-sitter-dart = "VERSION_FROM_STEP_1"
```

In `crates/grammars/Cargo.toml`, under `[dependencies]`, add next to the other tree-sitter grammars:

```toml
tree-sitter-dart.workspace = true
```

- [ ] **Step 3: Register the native grammar.** In `crates/grammars/src/grammars.rs`, add to the `vec![...]` in `native_grammars()` (alongside the `("go", ...)` line, keep alphabetical-ish placement near "css"/"dart"):

```rust
        ("dart", tree_sitter_dart::LANGUAGE.into()),
```

(If Step 1 found a `language()` fn instead of `LANGUAGE`, use `tree_sitter_dart::language()` — no `.into()`.)

- [ ] **Step 4: Create the language config.** `crates/grammars/src/dart/config.toml`, modeled on `crates/grammars/src/go/config.toml`:

```toml
name = "Dart"
grammar = "dart"
path_suffixes = ["dart"]
line_comments = ["// "]
autoclose_before = ";:.,=}])>"
brackets = [
  { start = "{", end = "}", close = true, newline = true },
  { start = "[", end = "]", close = true, newline = true },
  { start = "(", end = ")", close = true, newline = true },
  { start = "'", end = "'", close = true, newline = false, not_in = ["string", "comment"] },
  { start = "\"", end = "\"", close = true, newline = false, not_in = ["string", "comment"] },
]
```

- [ ] **Step 5: Create the query files.** Fetch the tree-sitter-dart repo's `queries/` folder (same repo as the crate) and copy its `highlights.scm`, `brackets.scm` (or `tags`/`indents`) into the matching Zed filenames. At minimum create `highlights.scm`; create `brackets.scm`, `indents.scm`, `injections.scm`, `outline.scm` (an empty file is acceptable for any query type the grammar lacks — Zed concatenates whatever exists). Verify node names in the queries match the grammar version from Step 1 (a query referencing a node the grammar doesn't have fails to load).

- [ ] **Step 6: Build the grammars crate.**

Run: `cargo build -p grammars --features load-grammars`
Expected: compiles; no "unknown grammar" or query-parse panic. If a query panics at load, fix the offending `.scm` node names.

- [ ] **Step 7: Commit.**

```bash
git add Cargo.toml Cargo.lock crates/grammars/Cargo.toml crates/grammars/src/grammars.rs crates/grammars/src/dart/
git commit -m "languages: add Dart tree-sitter grammar and queries"
```

---

### Task 2: Dart LSP adapter + language registration

**Files:**
- Create: `crates/languages/src/dart.rs`
- Modify: `crates/languages/src/lib.rs` (`mod` decl ~top; adapter build ~line 116; `LanguageInfo` entry in the `built_in_languages` array)
- Test: manual (open a `.dart` file; LSP attaches if SDK present)

**Interfaces:**
- Consumes: grammar/config `dart` from Task 1.
- Produces: `dart::DartLspAdapter` registered so `.dart` files get diagnostics/completion from `dart language-server`.

- [ ] **Step 1: Read the reference.** Open `crates/languages/src/go.rs` lines 43-209 to see the exact `LspInstaller` + `LspAdapter` split, and `LanguageServerBinary`/`LanguageServerName` usage. Match imports and the `#[async_trait(?Send)]` attributes exactly.

- [ ] **Step 2: Write `crates/languages/src/dart.rs`.** The Dart analysis server ships with the SDK, so discovery is "find `dart` (or `flutter`'s bundled dart) on PATH"; there is nothing to download.

```rust
use std::{ffi::OsString, path::PathBuf, sync::Arc};

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::Future;
use gpui::AsyncApp;
use language::{
    LanguageServerBinary, LanguageServerName, LspAdapter, LspAdapterDelegate, LspInstaller,
    Toolchain,
};

#[derive(Copy, Clone)]
pub struct DartLspAdapter;

impl DartLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("dart");
}

// `dart language-server --protocol=lsp` launches the analysis server in LSP mode.
// The same server handles Flutter projects (detected via pubspec.yaml).
fn server_binary_arguments() -> Vec<OsString> {
    vec!["language-server".into(), "--protocol=lsp".into()]
}

impl LspInstaller for DartLspAdapter {
    type BinaryVersion = ();

    async fn fetch_latest_server_version(
        &self,
        _delegate: &Arc<dyn LspAdapterDelegate>,
        _: bool,
        _cx: &mut AsyncApp,
    ) -> Result<()> {
        // SDK-provided; no remote version to fetch.
        Ok(())
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        // Prefer `dart` on PATH; fall back to the dart bundled inside a Flutter SDK.
        let path = match delegate.which("dart".as_ref()).await {
            Some(path) => path,
            None => {
                let flutter = delegate.which("flutter".as_ref()).await?;
                // <flutter>/bin/flutter -> <flutter>/bin/cache/dart-sdk/bin/dart
                let bin_dir = flutter.parent()?;
                let dart = bin_dir.join("cache/dart-sdk/bin/dart");
                if delegate.try_exists(&dart).await.unwrap_or(false) {
                    dart
                } else {
                    return None;
                }
            }
        };
        Some(LanguageServerBinary {
            path,
            arguments: server_binary_arguments(),
            env: None,
        })
    }

    fn fetch_server_binary(
        &self,
        _version: (),
        _container_dir: PathBuf,
        _delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Result<LanguageServerBinary>> + use<> {
        async {
            bail!(
                "Dart language server is provided by the Flutter/Dart SDK. \
                 Install Flutter and ensure `dart` (or `flutter`) is on your PATH."
            )
        }
    }

    async fn cached_server_binary(
        &self,
        _container_dir: PathBuf,
        _: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        None
    }
}

#[async_trait(?Send)]
impl LspAdapter for DartLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}
```

**Verify while writing:** confirm the exact `LspInstaller`/`LspAdapter` trait method set and the `LanguageServerBinary` field names against `crates/languages/src/go.rs` (Step 1). Confirm `LspAdapterDelegate` exposes `which(&OsStr)` and an existence check (`try_exists`); if the method is named differently, match the real name from the delegate trait. If `LspAdapter` has additional required methods with no default, copy their signatures from `GoLspAdapter` and return sensible defaults.

- [ ] **Step 3: Declare the module + register the language.** In `crates/languages/src/lib.rs`:

Add near the other `mod` declarations:
```rust
mod dart;
```

Near the Go adapter construction (~line 116), add:
```rust
    let dart_lsp_adapter = Arc::new(dart::DartLspAdapter);
```

Add an entry to the `built_in_languages` array (next to the `"go"` entry):
```rust
        LanguageInfo {
            name: "dart",
            adapters: vec![dart_lsp_adapter],
            ..Default::default()
        },
```

- [ ] **Step 4: Build.**

Run: `cargo build -p languages`
Expected: compiles. Fix any trait-method mismatches surfaced by the compiler against the real `LspInstaller`/`LspAdapter` definitions.

- [ ] **Step 5: Manual smoke test (needs Flutter SDK on PATH).** `cargo run`, open any `.dart` file in a Flutter/Dart project. Expect syntax highlighting (grammar) and, if `dart` is on PATH, LSP diagnostics/completion. If no SDK, highlighting still works and the LSP quietly stays down.

- [ ] **Step 6: Commit.**

```bash
git add crates/languages/src/dart.rs crates/languages/src/lib.rs
git commit -m "languages: add built-in Dart language server adapter"
```

---

## Phase 2 — Flutter debugging (DAP adapter)

### Task 3: Built-in Flutter debug adapter

**Files:**
- Create: `crates/dap_adapters/src/flutter.rs`
- Modify: `crates/dap_adapters/src/dap_adapters.rs:1` (mod decl), `:29` (registration)
- Test: `cargo build -p dap_adapters`; manual launch

**Interfaces:**
- Produces: a `DebugAdapter` named `"Flutter"` selectable in the debug UI; spawns `flutter debug_adapter`; maps scenario config → launch args.

- [ ] **Step 1: Read the reference.** Open `crates/dap_adapters/src/go.rs` fully (struct, `impl DebugAdapter`: `name`, `adapter_language_name`, `dap_schema`, `config_from_zed_format`, `get_binary`) and note the exact types: `DebugAdapterName`, `DebugScenario`, `DebugTaskDefinition`, `DebugAdapterBinary`, `StartDebuggingRequestArguments`, `DebugRequest`, `DapDelegate`. Match imports.

- [ ] **Step 2: Write `crates/dap_adapters/src/flutter.rs`.** Fill the four required `DebugAdapter` methods. The launch config carries `program`, `cwd`, `toolArgs` (device via `["-d", deviceId]`), and optional `noDebug`.

```rust
use std::{collections::HashMap, ffi::OsStr, path::PathBuf, sync::Arc};

use anyhow::{Result, bail};
use async_trait::async_trait;
use dap::{
    DebugRequest, StartDebuggingRequestArguments,
    adapters::{
        DapDelegate, DebugAdapter, DebugAdapterBinary, DebugAdapterName, DebugTaskDefinition,
    },
};
use gpui::AsyncApp;
use serde_json::{Value, json};
use task::{DebugScenario, ZedDebugConfig};

#[derive(Default, Debug)]
pub(crate) struct FlutterDebugAdapter;

impl FlutterDebugAdapter {
    const ADAPTER_NAME: &'static str = "Flutter";
}

#[async_trait(?Send)]
impl DebugAdapter for FlutterDebugAdapter {
    fn name(&self) -> DebugAdapterName {
        DebugAdapterName(Self::ADAPTER_NAME.into())
    }

    fn adapter_language_name(&self) -> Option<language::LanguageName> {
        Some(gpui::SharedString::new_static("Dart").into())
    }

    fn dap_schema(&self) -> Value {
        // Minimal launch/attach schema. `program`, `cwd`, `deviceId`, `toolArgs`, `noDebug`.
        json!({
            "properties": {
                "request": { "type": "string", "enum": ["launch", "attach"] },
                "program": { "type": "string", "description": "Dart entrypoint, e.g. lib/main.dart" },
                "cwd": { "type": "string" },
                "deviceId": { "type": "string", "description": "Flutter device id, e.g. macos, chrome" },
                "toolArgs": { "type": "array", "items": { "type": "string" } },
                "noDebug": { "type": "boolean" }
            }
        })
    }

    async fn config_from_zed_format(&self, zed_scenario: ZedDebugConfig) -> Result<DebugScenario> {
        // Model exactly on go.rs config_from_zed_format (go.rs:397-437):
        // build a serde_json config from zed_scenario.request, then return DebugScenario.
        let mut config = json!({});
        match zed_scenario.request {
            DebugRequest::Launch(launch) => {
                config["request"] = "launch".into();
                config["program"] = launch.program.into();
                if let Some(cwd) = launch.cwd {
                    config["cwd"] = cwd.to_string_lossy().into_owned().into();
                }
            }
            DebugRequest::Attach(_) => {
                config["request"] = "attach".into();
            }
        }
        if let Some(stop_on_entry) = zed_scenario.stop_on_entry {
            config["stopOnEntry"] = stop_on_entry.into();
        }
        Ok(DebugScenario {
            adapter: zed_scenario.adapter,
            label: zed_scenario.label,
            build: None,
            config,
            tcp_connection: None,
        })
    }

    async fn get_binary(
        &self,
        delegate: &Arc<dyn DapDelegate>,
        task_definition: &DebugTaskDefinition,
        user_installed_path: Option<PathBuf>,
        user_args: Option<Vec<String>>,
        _user_env: Option<HashMap<String, String>>,
        _cx: &mut AsyncApp,
    ) -> Result<DebugAdapterBinary> {
        let flutter = match user_installed_path {
            Some(p) => p,
            None => delegate
                .which(OsStr::new("flutter"))
                .await
                .ok_or_else(|| anyhow::anyhow!(
                    "`flutter` not found on PATH. Install the Flutter SDK."
                ))?,
        };

        let mut config = task_definition.config.clone();

        // Merge device id into toolArgs as `-d <id>` if provided and not already present.
        if let Some(device) = config.get("deviceId").and_then(Value::as_str) {
            let device = device.to_string();
            let tool_args = config
                .get_mut("toolArgs")
                .and_then(Value::as_array_mut);
            match tool_args {
                Some(args) => {
                    args.insert(0, "-d".into());
                    args.insert(1, device.into());
                }
                None => {
                    config["toolArgs"] = json!(["-d", device]);
                }
            }
        }

        let cwd = config
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .or_else(|| delegate.worktree_root_path().map(PathBuf::from));

        let mut arguments = vec!["debug_adapter".to_string()];
        if let Some(extra) = user_args {
            arguments.extend(extra);
        }

        Ok(DebugAdapterBinary {
            command: Some(flutter.to_string_lossy().into_owned()),
            arguments,
            cwd,
            envs: HashMap::default(),
            connection: None,
            request_args: StartDebuggingRequestArguments {
                configuration: config,
                request: self.request_kind(&task_definition.config).await?,
            },
        })
    }
}
```

**Verify while writing:** (a) the exact `flutter debug_adapter` subcommand spelling via `flutter --help` — if it's `debug-adapter`, change the string in `arguments`. (b) `DebugAdapterBinary`/`DebugScenario`/`ZedDebugConfig`/`DebugRequest` field names and import paths against `go.rs` — adjust `configuration` type (it may be `String` JSON-encoded rather than `Value`; go.rs shows the real type). (c) `delegate.worktree_root_path()` and `delegate.which(...)` signatures against the `DapDelegate` trait. (d) whether `command` is `Option<String>` (go.rs return confirms it is).

- [ ] **Step 3: Register the adapter.** In `crates/dap_adapters/src/dap_adapters.rs`, add the module decl at the top:
```rust
mod flutter;
```
And inside `init` (line ~29), add after the Go line:
```rust
        registry.add_adapter(Arc::from(FlutterDebugAdapter::default()));
```
Add `use flutter::FlutterDebugAdapter;` if the file uses explicit `use` for the others (match the existing style — go is referenced as `GoDebugAdapter`, so add the matching import).

- [ ] **Step 4: Build.**

Run: `cargo build -p dap_adapters`
Expected: compiles. Fix field/type mismatches against the compiler and `go.rs`.

- [ ] **Step 5: Manual smoke test (needs Flutter SDK + a device).** `cargo run`; open a Flutter project; create a debug scenario selecting adapter `Flutter` with config `{ "request": "launch", "program": "lib/main.dart", "deviceId": "macos" }`; launch. Expect the app to build and run under the debugger; set a breakpoint and confirm it hits.

- [ ] **Step 6: Commit.**

```bash
git add crates/dap_adapters/src/flutter.rs crates/dap_adapters/src/dap_adapters.rs
git commit -m "dap_adapters: add built-in Flutter debug adapter"
```

---

## Phase 3 — Hot reload & hot restart (core)

### Task 4: `hotReload`/`hotRestart` DAP request types + commands (+ test)

**Files:**
- Modify: `crates/project/src/debugger/dap_command.rs`
- Test: same file (`#[cfg(test)]` module — mirror existing dap_command tests)

**Interfaces:**
- Consumes: `dap::requests::Request` (external, unsealed), `LocalDapCommand` trait (`dap_command.rs:19`).
- Produces: `HotReloadCommand` and `HotRestartCommand` (pub(crate)) usable via `Session::request`. Both serialize to `{ "reason": "manual" }` with commands `"hotReload"`/`"hotRestart"`.

- [ ] **Step 1: Write the failing test.** Add to the `#[cfg(test)] mod tests` in `crates/project/src/debugger/dap_command.rs` (create the module if absent, matching the crate's test style):

```rust
#[test]
fn hot_reload_command_serializes_to_flutter_contract() {
    use dap::requests::Request as _;
    assert_eq!(HotReloadRequest::COMMAND, "hotReload");
    assert_eq!(HotRestartRequest::COMMAND, "hotRestart");

    let reload = HotReloadCommand::default();
    let args = reload.to_dap();
    assert_eq!(serde_json::to_value(&args).unwrap(), serde_json::json!({ "reason": "manual" }));

    let restart = HotRestartCommand::default();
    let args = restart.to_dap();
    assert_eq!(serde_json::to_value(&args).unwrap(), serde_json::json!({ "reason": "manual" }));
}
```

- [ ] **Step 2: Run test to verify it fails.**

Run: `cargo test -p project hot_reload_command_serializes_to_flutter_contract`
Expected: FAIL — `HotReloadRequest`/`HotReloadCommand` not defined.

- [ ] **Step 3: Implement the request types + commands.** Add to `crates/project/src/debugger/dap_command.rs` (near `RestartCommand`, ~line 748):

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct HotReloadArguments {
    pub reason: String,
}

/// Flutter custom DAP request: injects changed sources and rebuilds the widget tree.
pub(crate) enum HotReloadRequest {}
impl dap::requests::Request for HotReloadRequest {
    const COMMAND: &'static str = "hotReload";
    type Arguments = HotReloadArguments;
    type Response = serde_json::Value; // permissive: empty body -> Null
}

/// Flutter custom DAP request: full restart (state not preserved).
pub(crate) enum HotRestartRequest {}
impl dap::requests::Request for HotRestartRequest {
    const COMMAND: &'static str = "hotRestart";
    type Arguments = HotReloadArguments;
    type Response = serde_json::Value;
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct HotReloadCommand;

impl Default for HotReloadCommand {
    fn default() -> Self {
        Self
    }
}

impl LocalDapCommand for HotReloadCommand {
    type Response = <HotReloadRequest as dap::requests::Request>::Response;
    type DapRequest = HotReloadRequest;

    fn to_dap(&self) -> <Self::DapRequest as dap::requests::Request>::Arguments {
        HotReloadArguments { reason: "manual".to_string() }
    }

    fn response_from_dap(
        &self,
        message: <Self::DapRequest as dap::requests::Request>::Response,
    ) -> Result<Self::Response> {
        Ok(message)
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct HotRestartCommand;

impl Default for HotRestartCommand {
    fn default() -> Self {
        Self
    }
}

impl LocalDapCommand for HotRestartCommand {
    type Response = <HotRestartRequest as dap::requests::Request>::Response;
    type DapRequest = HotRestartRequest;

    fn to_dap(&self) -> <Self::DapRequest as dap::requests::Request>::Arguments {
        HotReloadArguments { reason: "manual".to_string() }
    }

    fn response_from_dap(
        &self,
        message: <Self::DapRequest as dap::requests::Request>::Response,
    ) -> Result<Self::Response> {
        Ok(message)
    }
}
```

**Verify while writing:** `HotReloadArguments` must satisfy the `Request::Arguments` bounds `Debug + Clone + Serialize + DeserializeOwned + Send + Sync` (it does). `LocalDapCommand` requires only `Response`, `DapRequest`, `to_dap`, `response_from_dap` (default `is_supported` returns `true`, default `CACHEABLE=false` — both fine). Do NOT implement the collab `DapCommand` trait — local only.

- [ ] **Step 4: Run the test to verify it passes.**

Run: `cargo test -p project hot_reload_command_serializes_to_flutter_contract`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/project/src/debugger/dap_command.rs
git commit -m "project/debugger: add hotReload/hotRestart DAP commands"
```

---

### Task 5: `Session::hot_reload` / `hot_restart`

**Files:**
- Modify: `crates/project/src/debugger/session.rs` (near `restart`, ~line 2181; add `use` for the new commands from the crate-local module — they're already in scope as `dap_command::HotReloadCommand`)

**Interfaces:**
- Consumes: `HotReloadCommand`/`HotRestartCommand` (Task 4); private `Session::request` (`session.rs:1750`).
- Produces: `Session::hot_reload(&mut self, cx)` / `hot_restart(&mut self, cx)`.

- [ ] **Step 1: Read the reference.** Re-read `Session::restart` (`session.rs:2181-2212`) to copy the `self.request(Command, process_result, cx)` shape and the `as_running().is_none()` guard.

- [ ] **Step 2: Implement the methods.** Add to `impl Session`:

```rust
    pub fn hot_reload(&mut self, cx: &mut Context<Self>) {
        if self.as_running().is_none() {
            return;
        }
        self.request(
            dap_command::HotReloadCommand,
            |_this, _result, _cx| None,
            cx,
        )
        .detach();
    }

    pub fn hot_restart(&mut self, cx: &mut Context<Self>) {
        if self.as_running().is_none() {
            return;
        }
        self.request(
            dap_command::HotRestartCommand,
            |_this, _result, _cx| None,
            cx,
        )
        .detach();
    }
```

**Verify while writing:** confirm `RestartCommand` is referenced as `dap_command::RestartCommand` or via a `use` at the top of `session.rs` (line 7 imports a list of commands) — add `HotReloadCommand, HotRestartCommand` to that same `use` list and drop the `dap_command::` prefix to match local style. Confirm the `process_result` closure arity/return type matches the `request` signature (`FnOnce(&mut Self, Result<T::Response>, &mut Context<Self>) -> Option<T::Response>`).

- [ ] **Step 3: Build.**

Run: `cargo build -p project`
Expected: compiles.

- [ ] **Step 4: Commit.**

```bash
git add crates/project/src/debugger/session.rs
git commit -m "project/debugger: Session::hot_reload and hot_restart"
```

---

### Task 6: Debugger actions, running-state methods, and toolbar buttons

**Files:**
- Modify: `crates/debugger_ui/src/session/running.rs` (add methods ~near `restart_session` line 1835)
- Modify: `crates/debugger_ui/src/debugger_ui.rs` (actions block ~29; wiring ~160-245)
- Modify: `crates/debugger_ui/src/debugger_panel.rs` (toolbar buttons ~822)

**Interfaces:**
- Consumes: `Session::hot_reload`/`hot_restart` (Task 5).
- Produces: `debugger::HotReload` / `debugger::HotRestart` actions and two toolbar buttons, shown only for the Flutter adapter.

- [ ] **Step 1: Add running-state methods.** In `crates/debugger_ui/src/session/running.rs`, next to `restart_session` (1835):

```rust
    pub fn hot_reload(&self, cx: &mut Context<Self>) {
        self.session().update(cx, |state, cx| {
            state.hot_reload(cx);
        });
    }

    pub fn hot_restart(&self, cx: &mut Context<Self>) {
        self.session().update(cx, |state, cx| {
            state.hot_restart(cx);
        });
    }
```

**Verify:** confirm `self.session()` returns the `Entity<Session>` and `.update(cx, ...)` matches how `restart_session` accesses it (copy exactly).

- [ ] **Step 2: Add the actions.** In `crates/debugger_ui/src/debugger_ui.rs`, inside the `actions!(debugger, [ ... ])` block (line 29), add:

```rust
        /// Performs a Flutter hot reload (injects changed sources, rebuilds widgets).
        HotReload,
        /// Performs a Flutter hot restart (full restart, state not preserved).
        HotRestart,
```

- [ ] **Step 3: Wire the actions.** In the toolbar-builder chain (near the `Restart` wiring at 216-223), add two `.on_action` closures modeled exactly on `Restart`:

```rust
                .on_action({
                    let active_item = active_item.clone();
                    move |_: &HotReload, _, cx| {
                        active_item
                            .update(cx, |item, cx| item.hot_reload(cx))
                            .ok();
                    }
                })
                .on_action({
                    let active_item = active_item.clone();
                    move |_: &HotRestart, _, cx| {
                        active_item
                            .update(cx, |item, cx| item.hot_restart(cx))
                            .ok();
                    }
                })
```

**Verify:** `active_item.update(cx, |item, cx| item.hot_reload(cx))` — confirm `item` here is the running-state type that owns the Step 1 methods (the `Restart` handler calls `item.restart_session(cx)`, so the same receiver has `hot_reload`).

- [ ] **Step 4: Add the toolbar buttons, gated to the Flutter adapter.** In `crates/debugger_ui/src/debugger_panel.rs`, near the `debug-restart` button (822-840), add two buttons wrapped so they only render for the Flutter adapter. Determine the adapter name from the running state (find how the panel already reads the session's adapter name; if none is handy, add a small accessor on the running state returning the adapter `DebugAdapterName`). Pattern:

```rust
                                    .when(is_flutter_adapter, |this| {
                                        this
                                            .child(
                                                IconButton::new("flutter-hot-reload", IconName::Bolt)
                                                    .icon_size(IconSize::Small)
                                                    .on_click(window.listener_for(
                                                        running_state,
                                                        |this, _, _window, cx| {
                                                            this.hot_reload(cx);
                                                        },
                                                    ))
                                                    .tooltip({
                                                        let focus_handle = focus_handle.clone();
                                                        move |_window, cx| {
                                                            Tooltip::for_action_in(
                                                                "Hot Reload",
                                                                &HotReload,
                                                                &focus_handle,
                                                                cx,
                                                            )
                                                        }
                                                    }),
                                            )
                                            .child(
                                                IconButton::new("flutter-hot-restart", IconName::RotateCw)
                                                    .icon_size(IconSize::Small)
                                                    .on_click(window.listener_for(
                                                        running_state,
                                                        |this, _, _window, cx| {
                                                            this.hot_restart(cx);
                                                        },
                                                    ))
                                                    .tooltip({
                                                        let focus_handle = focus_handle.clone();
                                                        move |_window, cx| {
                                                            Tooltip::for_action_in(
                                                                "Hot Restart",
                                                                &HotRestart,
                                                                &focus_handle,
                                                                cx,
                                                            )
                                                        }
                                                    }),
                                            )
                                    })
```

**Verify:** (a) `is_flutter_adapter` — compute it from the running session's adapter name == `"Flutter"` (match the constant used in Task 3). (b) `IconName::Bolt` / `RotateCw` exist — if not, pick real variants from the `IconName` enum (e.g. `IconName::ArrowCircle`, `IconName::Replace`); the compiler lists valid ones. (c) `window.listener_for(running_state, ...)` closure arity matches the `debug-restart` button exactly. (d) `HotReload`/`HotRestart` are imported into this file (add to the `crate::{...}` use).

- [ ] **Step 5: Build the whole debugger UI.**

Run: `cargo build -p debugger_ui`
Expected: compiles. Resolve any `IconName`/receiver-type mismatches per the compiler.

- [ ] **Step 6: Manual test (needs Flutter SDK + device).** `cargo run`; launch a Flutter app under the `Flutter` adapter; once running, the Hot Reload / Hot Restart buttons appear in the debug toolbar. Edit a widget's text, click Hot Reload → UI updates, counter state preserved. Click Hot Restart → app restarts, state reset. Confirm the buttons are absent for a non-Flutter (e.g. Go) session.

- [ ] **Step 7: Commit.**

```bash
git add crates/debugger_ui/src/session/running.rs crates/debugger_ui/src/debugger_ui.rs crates/debugger_ui/src/debugger_panel.rs
git commit -m "debugger_ui: add Flutter Hot Reload/Hot Restart actions and buttons"
```

**Optional refinement (defer unless needed):** gate the buttons additionally on the adapter's `flutter.appStarted` custom event rather than just "session running." This requires observing custom DAP events in the session; skip for v1 — sending before the app is live simply returns an adapter error that Zed logs.

---

## Phase 4 — Device picker

### Task 7: Interactive Flutter device picker

**Files:**
- Create/Modify: a picker in `crates/debugger_ui/` (new small module, e.g. `crates/debugger_ui/src/flutter_device_picker.rs`) OR fold device resolution into the launch flow.
- Test: manual

**Interfaces:**
- Consumes: nothing new; runs `flutter devices --machine`.
- Produces: a selected `deviceId` string injected into the scenario config's `deviceId` before `get_binary` runs (Task 3 already maps `deviceId` → `-d <id>`).

- [ ] **Step 1: Decide the trigger point — and confirm a clean async hook exists.** The picker should appear when a Flutter scenario is launched with no `deviceId` set. Locate where a debug scenario is resolved into a task before adapter `get_binary` (search `config_from_zed_format` / scenario resolution in `crates/project/src/debugger`). The picker runs there, or in `debugger_ui` before dispatching the launch. **If there is no clean async UI hook at that point, drop this task** — the `deviceId` config field from Task 3 already delivers device targeting on its own, so the picker is a convenience, not a required capability. Decide this before building any UI.

- [ ] **Step 2: Implement device enumeration.** Run `flutter devices --machine` (JSON array of `{ "id", "name", "targetPlatform", ... }`). Parse with serde:

```rust
#[derive(serde::Deserialize)]
struct FlutterDevice {
    id: String,
    name: String,
}

async fn list_flutter_devices(flutter: &std::path::Path) -> anyhow::Result<Vec<FlutterDevice>> {
    let output = util::command::new_smol_command(flutter)
        .args(["devices", "--machine"])
        .output()
        .await?;
    anyhow::ensure!(output.status.success(), "`flutter devices` failed");
    Ok(serde_json::from_slice(&output.stdout)?)
}
```

**Verify:** confirm the command helper name (`util::command::new_smol_command` vs `new_command`) against how `go.rs get_binary` spawns processes.

- [ ] **Step 3: Present the picker.** Reuse Zed's `Picker` component (search the codebase for an existing quick-pick usage, e.g. how the command palette or a modal picker is constructed) to show `name` (subtitle `id`). If exactly one device, auto-select and skip the UI. On selection, set `config["deviceId"]`.

- [ ] **Step 4: Build.**

Run: `cargo build -p debugger_ui`
Expected: compiles.

- [ ] **Step 5: Manual test.** With `macos` + `chrome` (and/or a simulator) available, launch a Flutter scenario without `deviceId` → picker lists devices → select one → app launches on it. With a single device, no picker shows.

- [ ] **Step 6: Commit.**

```bash
git add -A crates/debugger_ui/src crates/project/src
git commit -m "debugger_ui: Flutter device picker"
```

---

## Phase 5 — Build the macOS app

### Task 8: Produce a runnable macOS build

**Files:** none (procedure).

- [ ] **Step 1: Prerequisites.** Install Xcode + Command Line Tools (`xcode-select --install`), and rustup (the repo pins the toolchain via `rust-toolchain.toml`, auto-selected on first build).

- [ ] **Step 2: Dev build (fast loop).**

Run: `cargo run`
Expected: Zed launches from source with Dart language + Flutter adapter + hot reload/restart present. Use this loop throughout implementation.

- [ ] **Step 3: Build + install the app bundle locally.** `script/bundle-mac` (read: it defaults to a `--release` build, auto-installs Zed's `cargo-bundle` fork if missing, and — because no `MACOS_CERTIFICATE`/`APPLE_NOTARIZATION_*` env vars are set on a fork — **ad-hoc self-signs** the bundle at `bundle-mac:222` (`--sign -`) rather than failing). Its flags: `-d` debug build (sets `CARGO_BUNDLE_SKIP_BUILD=true`, so run `cargo build` first), `-i` install into `/Applications`, `-o` open/launch, `-h` help.

For a release build installed to `/Applications` and launched:

Run: `script/bundle-mac -i -o`
Expected: builds release, ad-hoc signs, moves `Zed.app` to `/Applications`, launches it (`bundle-mac:227-233`).

For a faster debug bundle (after a prior `cargo build`):

Run: `cargo build && script/bundle-mac -d -i -o`

With **no** `-i`/`-o`, the script instead produces a DMG (`Zed-<arch>.dmg`).

- [ ] **Step 4: If Gatekeeper still blocks it.** Ad-hoc signing usually lets a locally-built, locally-installed app launch. If macOS still quarantines it (e.g. run from the DMG), right-click → Open once, or clear the attribute:

Run: `xattr -dr com.apple.quarantine "/Applications/Zed.app"`
Expected: the app opens. (Proper distribution signing/notarization needs the `MACOS_CERTIFICATE` + `APPLE_NOTARIZATION_*` env vars and an Apple Developer account — out of scope for a fork.)

- [ ] **Step 5: End-to-end acceptance (needs Flutter SDK).** In the bundled app: open a Flutter project, edit `.dart` with LSP/highlighting, set a breakpoint, launch under the `Flutter` adapter via the device picker, hit the breakpoint, Hot Reload (state preserved), Hot Restart (state reset).

---

## Self-review

- **Spec coverage:** Editing → Tasks 1-2. Debugging → Task 3. Hot reload/restart → Tasks 4-6. Device config field → Task 3 (`deviceId`). Device picker → Task 7. macOS build → Task 8. All spec sections mapped.
- **Type consistency:** `HotReloadCommand`/`HotRestartCommand`/`HotReloadRequest`/`HotRestartRequest`/`HotReloadArguments` used identically in Tasks 4→5→6. `hot_reload`/`hot_restart` method names consistent Session (Task 5) → running-state (Task 6 Step 1) → action handlers (Task 6 Step 3). Adapter name `"Flutter"` consistent Task 3 ↔ Task 6 gating.
- **Grounding caveats (not placeholders):** each task's "Verify while writing" lists the exact struct/field/trait names to confirm against the cited reference file, because struct field sets (e.g. `DebugScenario`, `DebugAdapterBinary`, `LanguageServerBinary`) and `IconName` variants must match the fork's current source rather than be invented.
