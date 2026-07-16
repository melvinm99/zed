use anyhow::{Context as _, Result};
use futures::channel::oneshot;
use fuzzy::{StringMatch, StringMatchCandidate};
use gpui::{
    App, AppContext, Context, DismissEvent, Entity, EventEmitter, Focusable, IntoElement, Render,
    Subscription, Task, Window,
};
use picker::{Picker, PickerDelegate};
use std::sync::Arc;
use ui::{ListItem, ListItemSpacing, prelude::*};
use workspace::ModalView;

/// A device reported by `flutter devices --machine`. Other fields in that JSON
/// (targetPlatform, sdk, etc.) are ignored.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct FlutterDevice {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// Runs `flutter devices --machine` and parses the resulting JSON array.
///
/// `env` should be the worktree's shell environment (in particular its shell
/// `PATH`) so that a bare `flutter` can be resolved even when this process's
/// own environment doesn't have it — e.g. a macOS `.app` launched from Finder
/// doesn't inherit the user's shell `PATH`. Pass `None` to fall back to the
/// process's own environment.
///
/// Callers should treat any error (command not found, non-zero exit, bad JSON)
/// as "skip device selection" rather than a fatal condition — launching a
/// Flutter session without a `deviceId` is a valid fallback that `flutter run`
/// / the adapter handles on its own.
pub(crate) async fn list_flutter_devices(
    env: Option<collections::HashMap<String, String>>,
) -> Result<Vec<FlutterDevice>> {
    let mut command = util::command::new_command("flutter");
    command.args(["devices", "--machine"]);
    if let Some(env) = env {
        command.envs(env);
    }
    let output = command
        .output()
        .await
        .context("failed to run `flutter devices --machine`")?;
    anyhow::ensure!(
        output.status.success(),
        "`flutter devices --machine` exited with a non-zero status"
    );
    parse_flutter_devices(&output.stdout)
}

fn parse_flutter_devices(bytes: &[u8]) -> Result<Vec<FlutterDevice>> {
    Ok(serde_json::from_slice(bytes)?)
}

pub(crate) struct FlutterDeviceModalDelegate {
    selected_index: usize,
    matches: Vec<StringMatch>,
    candidates: Arc<[FlutterDevice]>,
    tx: Option<oneshot::Sender<Option<String>>>,
}

pub(crate) struct FlutterDeviceModal {
    _subscription: Subscription,
    picker: Entity<Picker<FlutterDeviceModalDelegate>>,
}

impl FlutterDeviceModal {
    pub(crate) fn new(
        devices: Arc<[FlutterDevice]>,
        tx: oneshot::Sender<Option<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let picker = cx.new(|cx| {
            Picker::uniform_list(
                FlutterDeviceModalDelegate {
                    selected_index: 0,
                    matches: Vec::default(),
                    candidates: devices,
                    tx: Some(tx),
                },
                window,
                cx,
            )
        });
        Self {
            _subscription: cx.subscribe(&picker, |_, _, _, cx| {
                cx.emit(DismissEvent);
            }),
            picker,
        }
    }
}

impl Render for FlutterDeviceModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("FlutterDeviceModal")
            .track_focus(&self.focus_handle(cx))
            .child(self.picker.clone())
    }
}

impl EventEmitter<DismissEvent> for FlutterDeviceModal {}

impl Focusable for FlutterDeviceModal {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.picker.read(cx).focus_handle(cx)
    }
}

impl ModalView for FlutterDeviceModal {}

impl PickerDelegate for FlutterDeviceModalDelegate {
    type ListItem = ListItem;

    fn name() -> &'static str {
        "flutter device modal"
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        Arc::from("Select the Flutter device to launch on")
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let Some(devices) = this
                .read_with(cx, |this, _| this.delegate.candidates.clone())
                .ok()
            else {
                return;
            };

            let matches = fuzzy::match_strings(
                &devices
                    .iter()
                    .enumerate()
                    .map(|(id, candidate)| {
                        StringMatchCandidate::new(
                            id,
                            format!("{} {}", candidate.name, candidate.id).as_str(),
                        )
                    })
                    .collect::<Vec<_>>(),
                &query,
                true,
                true,
                100,
                &Default::default(),
                cx.background_executor().clone(),
            )
            .await;

            this.update(cx, |this, _| {
                let delegate = &mut this.delegate;

                delegate.matches = matches;

                if delegate.matches.is_empty() {
                    delegate.selected_index = 0;
                } else {
                    delegate.selected_index =
                        delegate.selected_index.min(delegate.matches.len() - 1);
                }
            })
            .ok();
        })
    }

    fn confirm(&mut self, _secondary: bool, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let device = self
            .matches
            .get(self.selected_index())
            .and_then(|current_match| {
                let ix = current_match.candidate_id;
                self.candidates.get(ix)
            });

        cx.emit(DismissEvent);

        if let Some(tx) = self.tx.take() {
            tx.send(device.map(|device| device.id.clone())).ok();
        }
    }

    fn dismissed(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.selected_index = 0;

        if let Some(tx) = self.tx.take() {
            tx.send(None).ok();
        }

        cx.emit(DismissEvent);
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let hit = self.matches.get(ix)?;
        let device = self.candidates.get(hit.candidate_id)?;

        Some(
            ListItem::new(format!("flutter-device-entry-{ix}"))
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(
                    v_flex()
                        .items_start()
                        .child(Label::new(device.name.clone()))
                        .child(
                            Label::new(device.id.clone())
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flutter_devices_machine_json() {
        let json = br#"[
            {"id":"macos","name":"macOS","targetPlatform":"darwin-x64","emulator":false},
            {"id":"chrome","name":"Chrome"}
        ]"#;

        let devices = parse_flutter_devices(json).unwrap();

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "macos");
        assert_eq!(devices[0].name, "macOS");
        assert_eq!(devices[1].id, "chrome");
        assert_eq!(devices[1].name, "Chrome");
    }

    #[test]
    fn parses_empty_device_list() {
        let devices = parse_flutter_devices(b"[]").unwrap();
        assert!(devices.is_empty());
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_flutter_devices(b"not json").is_err());
    }
}
