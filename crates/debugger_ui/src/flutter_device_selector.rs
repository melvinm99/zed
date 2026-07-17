use gpui::{
    Anchor, App, Context, Entity, Global, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Task, WeakEntity, Window, div,
};
use project::{Worktree, WorktreeId};
use ui::{Button, ButtonCommon, ContextMenu, LabelSize, PopoverMenu, Tooltip};
use util::rel_path::RelPath;
use workspace::{HideStatusItem, StatusItemView, Workspace, item::ItemHandle};

use crate::flutter_device_modal::{FlutterDevice, list_flutter_devices};

/// The Flutter device a debug launch with no explicit `deviceId` should use.
/// Set by [`FlutterDeviceSelector`], read by `resolve_scenario` so that a
/// selection made in the status bar skips the launch-time device picker.
pub struct SelectedFlutterDevice(pub String);
impl Global for SelectedFlutterDevice {}

/// Status-bar item showing the currently selected Flutter device. Hidden
/// unless the open project has a `pubspec.yaml` at a worktree root and
/// `flutter devices` reports at least one device.
pub struct FlutterDeviceSelector {
    workspace: WeakEntity<Workspace>,
    is_flutter_project: bool,
    devices: Vec<FlutterDevice>,
    flutter_worktree_id: Option<WorktreeId>,
    _observe_selected: Subscription,
    _refresh_task: Task<Option<()>>,
}

impl FlutterDeviceSelector {
    pub fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let this = Self {
            workspace: workspace.weak_handle(),
            is_flutter_project: false,
            devices: Vec::new(),
            flutter_worktree_id: None,
            _observe_selected: cx.observe_global::<SelectedFlutterDevice>(|_, cx| cx.notify()),
            _refresh_task: Task::ready(None),
        };
        // `new` runs inside a Workspace update (via observe_new /
        // initialize_workspace), so reading the Workspace entity synchronously
        // here would double-lease it and panic. Defer the initial refresh so it
        // runs once that update has completed.
        cx.defer_in(window, |this, window, cx| this.refresh(window, cx));
        this
    }

    /// Returns the first worktree with a `pubspec.yaml` at its root, if any.
    fn flutter_worktree(workspace: &Entity<Workspace>, cx: &App) -> Option<Entity<Worktree>> {
        let pubspec = RelPath::from_unix_str("pubspec.yaml").unwrap();
        workspace
            .read(cx)
            .project()
            .read(cx)
            .worktrees(cx)
            .find(|worktree| worktree.read(cx).entry_for_path(pubspec).is_some())
    }

    /// Re-detects whether the project is a Flutter project and, if so,
    /// (re-)lists devices in the background. A no-op if the detected Flutter
    /// worktree (or lack thereof) is unchanged since the last call, so this
    /// can be called on every active-pane-item change without spawning a new
    /// `flutter devices --machine` subprocess on every tab switch.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let worktree = Self::flutter_worktree(&workspace, cx);
        let worktree_id = worktree.as_ref().map(|worktree| worktree.read(cx).id());
        if worktree_id == self.flutter_worktree_id {
            return;
        }
        self.flutter_worktree_id = worktree_id;

        let Some(worktree) = worktree else {
            self.is_flutter_project = false;
            self.devices.clear();
            cx.notify();
            return;
        };
        self.is_flutter_project = true;

        let environment = workspace.read(cx).project().read(cx).environment().clone();
        self._refresh_task = cx.spawn_in(window, async move |this, cx| {
            let env = environment
                .update(cx, |environment, cx| {
                    environment.worktree_environment(worktree, cx)
                })
                .await;
            let devices = list_flutter_devices(env, &cx.background_executor())
                .await
                .unwrap_or_default();

            this.update(cx, |this, cx| {
                if !devices.is_empty() && cx.try_global::<SelectedFlutterDevice>().is_none() {
                    cx.set_global(SelectedFlutterDevice(devices[0].id.clone()));
                }
                this.devices = devices;
                cx.notify();
            })
            .ok();
            Some(())
        });
    }
}

/// Label to show on the status-bar button: the selected device's name if
/// known, its raw id if it's not (yet) in `devices`, otherwise a placeholder.
fn device_label(devices: &[FlutterDevice], selected_id: Option<&str>) -> SharedString {
    let Some(selected_id) = selected_id else {
        return SharedString::new_static("Select Device");
    };
    devices
        .iter()
        .find(|device| device.id == selected_id)
        .map(|device| SharedString::from(device.name.clone()))
        .unwrap_or_else(|| SharedString::from(selected_id.to_string()))
}

impl Render for FlutterDeviceSelector {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_flutter_project || self.devices.is_empty() {
            return div().hidden();
        }

        let selected_id = cx.try_global::<SelectedFlutterDevice>().map(|d| d.0.clone());
        let label = device_label(&self.devices, selected_id.as_deref());
        let devices = self.devices.clone();

        div().child(
            PopoverMenu::new("flutter-device-selector")
                .trigger(
                    Button::new("flutter-device-selector-trigger", label)
                        .label_size(LabelSize::Small)
                        .tab_index(0isize)
                        .tooltip(Tooltip::text("Select Flutter Device")),
                )
                .anchor(Anchor::BottomRight)
                .menu(move |window, cx| {
                    let devices = devices.clone();
                    Some(ContextMenu::build(window, cx, move |mut menu, _window, _cx| {
                        for device in devices.iter() {
                            let id = device.id.clone();
                            menu = menu.entry(device.name.clone(), None, move |_window, cx| {
                                cx.set_global(SelectedFlutterDevice(id.clone()));
                            });
                        }
                        menu
                    }))
                }),
        )
    }
}

impl StatusItemView for FlutterDeviceSelector {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `add_right_item` calls this synchronously while the Workspace is
        // still being updated (in initialize_workspace), and pane changes can
        // also fire mid-update. `refresh` reads the Workspace entity, which
        // would double-lease it, so defer. The worktree-change guard in
        // `refresh` keeps repeated deferred calls cheap (no-op when unchanged).
        cx.defer_in(window, |this, window, cx| this.refresh(window, cx));
    }

    fn hide_setting(&self, _: &App) -> Option<HideStatusItem> {
        // Visibility is driven entirely by Flutter-project detection and
        // device availability, not a user-facing setting.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, name: &str) -> FlutterDevice {
        FlutterDevice {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn device_label_prefers_name_over_id() {
        let devices = vec![device("macos", "macOS"), device("chrome", "Chrome")];
        assert_eq!(device_label(&devices, Some("macos")).as_ref(), "macOS");
    }

    #[test]
    fn device_label_falls_back_to_id_when_not_listed() {
        let devices = vec![device("macos", "macOS")];
        assert_eq!(device_label(&devices, Some("missing")).as_ref(), "missing");
    }

    #[test]
    fn device_label_default_when_none_selected() {
        let devices = vec![device("macos", "macOS")];
        assert_eq!(device_label(&devices, None).as_ref(), "Select Device");
    }
}
