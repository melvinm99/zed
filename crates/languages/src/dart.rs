use std::{ffi::OsString, path::PathBuf, sync::Arc};

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::Future;
use gpui::AsyncApp;
use language::{LspAdapter, LspAdapterDelegate, LspInstaller, Toolchain};
use lsp::{LanguageServerBinary, LanguageServerName};

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
        _pre_release: bool,
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
                // <flutter-sdk>/bin/flutter -> <flutter-sdk>/bin/cache/dart-sdk/bin/dart
                // ponytail: no exists-check API on LspAdapterDelegate; if this path is wrong
                // the server simply fails to launch and the LSP stays down (no crash).
                let bin_dir = flutter.parent()?;
                bin_dir.join("cache/dart-sdk/bin/dart")
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
