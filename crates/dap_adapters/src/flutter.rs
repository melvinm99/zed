use std::{ffi::OsStr, path::PathBuf, sync::Arc};

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use collections::HashMap;
use dap::{
    DebugRequest, StartDebuggingRequestArguments,
    adapters::{
        DapDelegate, DebugAdapter, DebugAdapterBinary, DebugAdapterName, DebugTaskDefinition,
    },
};
use gpui::{AsyncApp, SharedString};
use language::LanguageName;
use serde_json::{Value, json};
use task::{DebugScenario, ZedDebugConfig};

#[derive(Default, Debug)]
pub(crate) struct FlutterDebugAdapter;

impl FlutterDebugAdapter {
    const ADAPTER_NAME: &'static str = dap::adapters::FLUTTER_ADAPTER_NAME;
    // ponytail: verified against `flutter --help` on a real Flutter SDK install before
    // shipping; the DAP entrypoint subcommand may be spelled `debug-adapter` on some
    // versions. Update this single constant if so.
    const DEBUG_ADAPTER_SUBCOMMAND: &'static str = "debug_adapter";
}

#[async_trait(?Send)]
impl DebugAdapter for FlutterDebugAdapter {
    fn name(&self) -> DebugAdapterName {
        DebugAdapterName(Self::ADAPTER_NAME.into())
    }

    fn adapter_language_name(&self) -> Option<LanguageName> {
        Some(SharedString::new_static("Dart").into())
    }

    fn dap_schema(&self) -> Value {
        json!({
            "properties": {
                "request": { "type": "string", "enum": ["launch", "attach"] },
                "program": {
                    "type": "string",
                    "description": "Dart entrypoint, e.g. lib/main.dart",
                    "default": "lib/main.dart"
                },
                "cwd": {
                    "type": "string",
                    "description": "Workspace relative or absolute path to the Flutter project root.",
                    "default": "${ZED_WORKTREE_ROOT}"
                },
                "deviceId": {
                    "type": "string",
                    "description": "Flutter device id to run on, e.g. macos, chrome, emulator-5554."
                },
                "toolArgs": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Extra arguments forwarded to `flutter debug_adapter`.",
                    "default": []
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Extra arguments forwarded to the running app.",
                    "default": []
                },
                "env": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Environment variables to set when launching the app."
                },
                "noDebug": {
                    "type": "boolean",
                    "description": "Run without attaching a debugger.",
                    "default": false
                }
            }
        })
    }

    async fn config_from_zed_format(&self, zed_scenario: ZedDebugConfig) -> Result<DebugScenario> {
        let mut config = json!({});
        match zed_scenario.request {
            DebugRequest::Launch(launch) => {
                config["request"] = "launch".into();
                config["env"] = launch.env_json();
                config["program"] = launch.program.into();
                config["args"] = launch.args.into();
                if let Some(cwd) = launch.cwd {
                    config["cwd"] = cwd.to_string_lossy().into_owned().into();
                }
            }
            DebugRequest::Attach(_) => {
                // Flutter's `attach` targets a running app via its VM service URI rather
                // than a PID, so there's nothing from `AttachRequest` to map here yet.
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
        user_env: Option<HashMap<String, String>>,
        _cx: &mut AsyncApp,
    ) -> Result<DebugAdapterBinary> {
        let flutter = match user_installed_path {
            Some(path) => path,
            None => delegate
                .which(OsStr::new("flutter"))
                .await
                .context("`flutter` not found on PATH. Install the Flutter SDK first.")?,
        };

        let mut configuration = task_definition.config.clone();
        apply_device_id(&mut configuration);

        let cwd = Some(
            task_definition
                .config
                .get("cwd")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| delegate.worktree_root_path().to_path_buf()),
        );

        let mut arguments = vec![Self::DEBUG_ADAPTER_SUBCOMMAND.to_string()];
        if let Some(extra) = user_args {
            arguments.extend(extra);
        }

        Ok(DebugAdapterBinary {
            command: Some(flutter.to_string_lossy().into_owned()),
            arguments,
            cwd,
            envs: user_env.unwrap_or_default(),
            // ponytail: `flutter debug_adapter` speaks stdio only; no TCP transport to wire up.
            connection: None,
            request_args: StartDebuggingRequestArguments {
                request: self.request_kind(&task_definition.config).await?,
                configuration,
            },
        })
    }
}

/// Merges a `deviceId` config field into `toolArgs` as `-d <id>`, preserving any
/// existing `toolArgs` array (or discarding a malformed non-array value).
fn apply_device_id(config: &mut Value) {
    let Some(device) = config.get("deviceId").and_then(Value::as_str) else {
        return;
    };
    if device.starts_with('-') {
        log::warn!("Ignoring deviceId {device:?}: looks like a flag, not a device id");
        return;
    }
    let device = device.to_string();
    match config.get_mut("toolArgs").and_then(Value::as_array_mut) {
        Some(args) => {
            let has_device_flag = args
                .iter()
                .any(|arg| matches!(arg.as_str(), Some("-d") | Some("--device-id")));
            if !has_device_flag {
                args.insert(0, "-d".into());
                args.insert(1, device.into());
            }
        }
        None => {
            config["toolArgs"] = json!(["-d", device]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_device_id_no_existing_tool_args() {
        let mut config = json!({ "deviceId": "macos" });
        apply_device_id(&mut config);
        assert_eq!(config["toolArgs"], json!(["-d", "macos"]));
    }

    #[test]
    fn apply_device_id_merges_into_existing_tool_args() {
        let mut config = json!({ "deviceId": "macos", "toolArgs": ["--verbose"] });
        apply_device_id(&mut config);
        assert_eq!(config["toolArgs"], json!(["-d", "macos", "--verbose"]));
    }

    #[test]
    fn apply_device_id_overwrites_non_array_tool_args() {
        let mut config = json!({ "deviceId": "macos", "toolArgs": "not-an-array" });
        apply_device_id(&mut config);
        assert_eq!(config["toolArgs"], json!(["-d", "macos"]));
    }

    #[test]
    fn apply_device_id_rejects_leading_dash() {
        let mut config = json!({ "deviceId": "--foo" });
        apply_device_id(&mut config);
        assert_eq!(config.get("toolArgs"), None);
    }

    #[test]
    fn apply_device_id_does_not_duplicate_existing_device_flag() {
        let mut config = json!({ "deviceId": "chrome", "toolArgs": ["-d", "chrome"] });
        apply_device_id(&mut config);
        assert_eq!(config["toolArgs"], json!(["-d", "chrome"]));
    }
}
