//! The press engine: the one place that decides what the daemon does. Pure apart from the `Ports`
//! it is handed, so every behavior is testable with a recording fake.
//!
//! Two maps carry a window's state. `desired` is the target and is exactly what the state file
//! holds; `observed` is where each tracked window was last seen and is never persisted. Every
//! change, including startup, goes through `transition_to`: build the new desired map, diff it
//! against the current one, write it, send what the diff calls for, then read back where the
//! windows ended up.
//!
//! The daemon only ever moves windows: a stash is a push toward an edge that niri clamps, a
//! restore is the nudge back. niri applies a move before it answers the request, so the read-back
//! is the whole story: there is nothing to wait for, and any later report that differs from it is
//! a change the user made, which forgets the window.

use std::collections::{BTreeMap, BTreeSet};

use crate::core::command::Command;
use crate::core::focus::{FocusDetector, FocusPolicy};
use crate::core::geometry::{self, Geometry};
use crate::core::input::{CompositorEvent, CompositorSnapshot, WindowInfo};
use crate::core::ports::Ports;
use crate::core::WindowId;
use crate::hotkey::protocol::{HotkeyReply, HotkeyRequest, ReplyErrorCode};
use crate::state_file::{DesiredState, LoadOutcome, StateFile, WindowEntry};

/// The target: what each tracked window should be. Identical to the state file's contents.
type Desired = BTreeMap<WindowId, WindowEntry>;

pub(crate) struct StashEngine {
    instance: String,
    desired: Desired,
    /// Where each tracked window was last seen.
    observed: BTreeMap<WindowId, Geometry>,
    focus: FocusDetector,
    focus_policy: FocusPolicy,
}

impl StashEngine {
    /// Create an engine for niri instance `instance` (the value of `$NIRI_SOCKET`) that trusts
    /// focus changes per `focus_policy`.
    pub(crate) fn new(instance: String, focus_policy: FocusPolicy) -> Self {
        Self {
            instance,
            desired: Desired::new(),
            observed: BTreeMap::new(),
            focus: FocusDetector::new(None, focus_policy),
            focus_policy,
        }
    }

    // ----- startup -----------------------------------------------------------------------------

    /// Startup: take what `load` found and the live snapshot, and re-apply the desired state:
    /// entries whose window is gone, tiled, or in the user's hand are dropped, and every floating
    /// one is pushed or nudged back again through the usual path.
    pub(crate) fn start_from_saved_state(
        &mut self,
        loaded: LoadOutcome,
        snapshot: &CompositorSnapshot,
        ports: &mut dyn Ports,
    ) {
        let focused = snapshot.windows.iter().find(|w| w.focused).map(|w| w.id);
        self.focus = FocusDetector::new(focused, self.focus_policy);
        let saved = match loaded {
            LoadOutcome::Usable(saved) => saved,
            LoadOutcome::StartEmpty(reason) => {
                log::info!("starting with no stashed windows: {reason}");
                return;
            }
        };
        let mut target = Desired::new();
        for (id, entry) in saved.windows {
            match floating_geometry(&snapshot.windows, id) {
                Some(geometry) => {
                    log::info!("startup re-apply window={id} goal={:?}", entry.desired);
                    self.observed.insert(id, geometry);
                    target.insert(id, entry);
                }
                None => {
                    log::info!("startup: window {id} is gone, tiled, or being dragged; dropped")
                }
            }
        }
        if let Err(err) = self.transition_to(target, &[], ports) {
            log::error!("state write failed at startup; not re-applying: {err}");
            self.observed.clear();
        }
    }

    // ----- presses -----------------------------------------------------------------------------

    /// A hotkey request with a fresh snapshot: `Stash` pushes what is not stashed yet, `Unstash`
    /// restores what is, and `Toggle` does the first when there is anything to stash, else the
    /// second. The reply is final: by the time the batch has been answered, every window it
    /// moved is where it is going to be.
    pub(crate) fn on_press(
        &mut self,
        request: HotkeyRequest,
        snapshot: &CompositorSnapshot,
        ports: &mut dyn Ports,
    ) -> HotkeyReply {
        // Anything a previous press expected and never saw is stale by now.
        self.focus.clear_expectations();
        let Some(workspace) = snapshot.focused_workspace() else {
            return HotkeyReply::Ok;
        };
        let (out_w, out_h) = snapshot.focused_output_size();
        // The floating windows on the workspace that have a position, split by whether they
        // are stashed. Members of the press keep their report alongside their geometry.
        let (stashed, unstashed): (Vec<(&WindowInfo, Geometry)>, Vec<_>) = snapshot
            .windows
            .iter()
            .filter(|info| info.floating && info.workspace == Some(workspace.id))
            .filter_map(|info| info.geometry.map(|geometry| (info, geometry)))
            .partition(|(info, _)| self.is_stashed(info.id));

        let wants_stash = matches!(request, HotkeyRequest::Stash | HotkeyRequest::Toggle);
        let wants_restore = matches!(request, HotkeyRequest::Toggle | HotkeyRequest::Unstash);
        let (target, members, extra) = if wants_stash && !unstashed.is_empty() {
            // Stash every unstashed floating window toward its nearest edge (spec D2, D3).
            let mut target = self.desired.clone();
            for &(info, geometry) in &unstashed {
                let edge = geometry::nearest_output_edge(geometry, out_w, out_h);
                target.insert(
                    info.id,
                    WindowEntry {
                        original: geometry,
                        desired: DesiredState::Stashed { edge },
                    },
                );
            }
            let extra = self.focus_commands_after_stash(&unstashed, snapshot);
            (target, unstashed, extra)
        } else if wants_restore && !stashed.is_empty() {
            // Restore every stashed window on the workspace (spec D5), focusing the one the
            // compositor saw focused last; the first on the workspace if it has no record.
            let mut target = self.desired.clone();
            for (info, _) in &stashed {
                target.get_mut(&info.id).expect("stashed").desired = DesiredState::Restored;
            }
            let focus_id = stashed
                .iter()
                .filter(|(info, _)| info.focus_timestamp.is_some())
                .max_by_key(|(info, _)| info.focus_timestamp)
                .map_or(stashed[0].0.id, |(info, _)| info.id);
            self.focus.expect_focus_request(Some(focus_id));
            (target, stashed, vec![Command::FocusWindow(focus_id)])
        } else {
            return HotkeyReply::Ok;
        };
        for (info, geometry) in members {
            self.observed.insert(info.id, geometry);
        }
        match self.transition_to(target, &extra, ports) {
            Ok(()) => HotkeyReply::Ok,
            Err(message) => {
                // Nothing was written, so nothing noted for this press is remembered either.
                self.observed.retain(|id, _| self.desired.contains_key(id));
                self.focus.clear_expectations();
                HotkeyReply::Err {
                    code: ReplyErrorCode::StateUnwritable,
                    message,
                }
            }
        }
    }

    /// Keystrokes must not land in a stashed window (spec D8): after stashing the focused window,
    /// move focus to the tiled layout when there is one to move to; niri ignores the request
    /// otherwise.
    fn focus_commands_after_stash(
        &mut self,
        stashed: &[(&WindowInfo, Geometry)],
        snapshot: &CompositorSnapshot,
    ) -> Vec<Command> {
        let focused_stashed = stashed.iter().any(|(info, _)| info.focused);
        let workspace_id = snapshot.focused_workspace().map(|w| w.id);
        let has_tiled = snapshot
            .windows
            .iter()
            .any(|info| !info.floating && info.workspace == workspace_id);
        if focused_stashed && has_tiled {
            self.focus.expect_focus_request(None);
            vec![Command::FocusTiling]
        } else {
            Vec::new()
        }
    }

    // ----- compositor events -------------------------------------------------------------------

    /// Events from the compositor, in order.
    pub(crate) fn on_compositor_events(
        &mut self,
        events: impl IntoIterator<Item = CompositorEvent>,
        ports: &mut dyn Ports,
    ) {
        for event in events {
            self.on_compositor_event(event, ports);
        }
    }

    fn on_compositor_event(&mut self, event: CompositorEvent, ports: &mut dyn Ports) {
        match event {
            CompositorEvent::GeometryChanged { id, geometry } => {
                self.on_geometry_changed(id, geometry, ports)
            }
            CompositorEvent::FocusChanged(id) => {
                if let Some(landed) = self.focus.observe_focus(id) {
                    self.on_focus_landing(landed, ports);
                }
            }
            CompositorEvent::WindowChanged(info) => {
                if let Some(landed) = self.focus.observe_window_report(info.id, info.focused) {
                    self.on_focus_landing(landed, ports);
                }
                self.on_window_changed(&info, ports);
            }
            CompositorEvent::WindowClosed(id) if self.desired.contains_key(&id) => {
                self.forget_windows(&[id], "closed", ports)
            }
            CompositorEvent::WindowClosed(_) => {}
        }
    }

    /// The user landed on `id` (spec D6, or any focus change under `FocusPolicy::AnyFocus`): if
    /// it is a stashed window, nudge it back alone. Focus is already on it, so no focus request
    /// is needed.
    fn on_focus_landing(&mut self, id: WindowId, ports: &mut dyn Ports) {
        if !self.is_stashed(id) {
            return;
        }
        let target = self.target_restoring(id);
        match self.transition_to(target, &[], ports) {
            Ok(()) => log::info!("focus unstash window={id}"),
            Err(err) => log::error!("state write failed before focus unstash of {id}: {err}"),
        }
    }

    /// A geometry report for `id`: where the engine last saw it means nothing happened (the
    /// report for its own move arrives after the read-back); anything else is a change the user
    /// made, which forgets the window (spec D7).
    fn on_geometry_changed(&mut self, id: WindowId, current: Geometry, ports: &mut dyn Ports) {
        if let Some(at) = self.observed.get(&id) {
            if !geometry::within_tolerance(*at, current) {
                self.forget_windows(&[id], "manual-change", ports);
            }
        }
    }

    /// A window-changed report: a tracked window that leaves the floating layout or is picked up
    /// by hand is forgotten (spec D7, D13); otherwise its geometry is handled like a geometry report.
    fn on_window_changed(&mut self, info: &WindowInfo, ports: &mut dyn Ports) {
        if !self.desired.contains_key(&info.id) {
            return;
        }
        match (info.floating, info.geometry) {
            (false, _) => self.forget_windows(&[info.id], "tiled", ports),
            (true, None) if info.workspace.is_none() => {
                self.forget_windows(&[info.id], "dragged", ports)
            }
            (true, None) => {}
            (true, Some(current)) => self.on_geometry_changed(info.id, current, ports),
        }
    }

    // ----- the one path every change takes ---------------------------------------------------

    /// Move from the current desired map to `target`: write it, send what the diff calls for,
    /// then `extra`, and read back where every window that was sent something ended up. On a
    /// failed write nothing changes and the reason is returned (plan D-13).
    ///
    /// Diff rules, per window: newly stashed → push it toward its edge; stashed → restored →
    /// nudge it back, or drop it at once when it already sits there; dropped from the map →
    /// nothing to send; unchanged → nothing. A window with no observed geometry is skipped.
    fn transition_to(
        &mut self,
        mut target: Desired,
        extra: &[Command],
        ports: &mut dyn Ports,
    ) -> Result<(), String> {
        let mut commands: Vec<Command> = Vec::new();
        let mut sent: Vec<WindowId> = Vec::new();
        let mut already_there: Vec<WindowId> = Vec::new();
        let ids: BTreeSet<WindowId> = self.desired.keys().chain(target.keys()).copied().collect();
        for id in ids {
            let before = self.desired.get(&id).map(|e| e.desired);
            let Some(after) = target.get(&id) else {
                continue;
            };
            let Some(current) = self.observed.get(&id).copied() else {
                continue;
            };
            match (before, after.desired) {
                (Some(DesiredState::Stashed { .. }), DesiredState::Stashed { .. })
                | (Some(DesiredState::Restored), DesiredState::Restored) => {}
                (_, DesiredState::Stashed { edge }) => {
                    commands.push(Command::Nudge {
                        id,
                        by: geometry::push_toward(edge),
                    });
                    sent.push(id);
                }
                (_, DesiredState::Restored) => {
                    match geometry::nudge_between(current, after.original) {
                        Some(nudge) => {
                            commands.push(Command::Nudge { id, by: nudge });
                            sent.push(id);
                        }
                        None => already_there.push(id),
                    }
                }
            }
        }
        for id in &already_there {
            target.remove(id);
        }
        ports
            .save(&StateFile {
                instance: self.instance.clone(),
                windows: target.clone(),
            })
            .map_err(|e| e.to_string())?;
        self.desired = target;
        self.observed.retain(|id, _| self.desired.contains_key(id));
        commands.extend_from_slice(extra);
        if !commands.is_empty() {
            if let Err(err) = ports.execute(&commands) {
                log::warn!("compositor request failed: {err}");
            }
        }
        if !sent.is_empty() {
            self.read_back(&sent, ports);
        }
        Ok(())
    }

    /// After a batch, read where the windows it moved now are: record each
    /// position, drop every restore as done, and forget any window that is no longer floating.
    fn read_back(&mut self, sent: &[WindowId], ports: &mut dyn Ports) {
        let live = match ports.list_windows() {
            Ok(live) => live,
            Err(err) => {
                log::warn!("could not read the windows back after a command: {err}");
                self.forget_windows(sent, "unconfirmed", ports);
                return;
            }
        };
        let mut gone: Vec<WindowId> = Vec::new();
        let mut restored: Vec<WindowId> = Vec::new();
        for id in sent {
            match floating_geometry(&live, *id) {
                None => gone.push(*id),
                Some(at) => {
                    log::debug!("window={id} now at {at:?}");
                    self.observed.insert(*id, at);
                    if !self.is_stashed(*id) {
                        restored.push(*id);
                    }
                }
            }
        }
        for id in &restored {
            log::info!("restored window={id}");
        }
        if !gone.is_empty() {
            self.forget_windows(&gone, "gone-after-command", ports);
        }
        if !restored.is_empty() {
            let target = self.target_without(&restored);
            if let Err(err) = self.transition_to(target, &[], ports) {
                log::error!("state write failed after dropping restored windows: {err}");
            }
        }
    }

    // ----- helpers -----------------------------------------------------------------------------

    fn is_stashed(&self, id: WindowId) -> bool {
        self.desired
            .get(&id)
            .is_some_and(|e| matches!(e.desired, DesiredState::Stashed { .. }))
    }

    /// `desired` with `id` set to be restored to its original.
    fn target_restoring(&self, id: WindowId) -> Desired {
        let mut target = self.desired.clone();
        if let Some(entry) = target.get_mut(&id) {
            entry.desired = DesiredState::Restored;
        }
        target
    }

    /// `desired` without `ids`.
    fn target_without(&self, ids: &[WindowId]) -> Desired {
        let mut target = self.desired.clone();
        for id in ids {
            target.remove(id);
        }
        target
    }

    /// Stop tracking `ids` for `reason`, logging each original so it can be restored by hand
    /// from the journal. The windows stay where they are.
    fn forget_windows(&mut self, ids: &[WindowId], reason: &str, ports: &mut dyn Ports) {
        for id in ids {
            if let Some(entry) = self.desired.get(id) {
                log::info!(
                    "forget window={id} reason={reason} original={:?}",
                    entry.original
                );
            }
        }
        let target = self.target_without(ids);
        if let Err(err) = self.transition_to(target, &[], ports) {
            log::error!("state write failed after forgetting {ids:?}: {err}");
        }
    }
}

/// Where window `id` is, if it is listed, floating, and has a position.
fn floating_geometry(windows: &[WindowInfo], id: WindowId) -> Option<Geometry> {
    windows
        .iter()
        .find(|w| w.id == id)
        .filter(|w| w.floating)
        .and_then(|w| w.geometry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry::Edge;
    use crate::core::input::FocusTimestamp;
    use crate::core::input::WorkspaceInfo;
    use crate::core::ports::{Compositor, StateStore};
    use std::collections::HashMap;
    use std::io;

    /// Records every effect the engine asks for. `windows` is what niri lists after a batch.
    #[derive(Default)]
    struct Fake {
        saves: Vec<StateFile>,
        actions: Vec<Command>,
        /// The order of effects: "save", "act" and "list" entries.
        order: Vec<&'static str>,
        fail_save: bool,
        fail_list: bool,
        windows: Vec<WindowInfo>,
    }

    impl StateStore for Fake {
        fn save(&mut self, state: &StateFile) -> io::Result<()> {
            if self.fail_save {
                return Err(io::Error::other("rename state.json: Read-only file system"));
            }
            self.order.push("save");
            self.saves.push(state.clone());
            Ok(())
        }
    }

    impl Compositor for Fake {
        fn execute(&mut self, commands: &[Command]) -> io::Result<()> {
            self.order.push("act");
            self.actions.extend(commands.iter().copied());
            Ok(())
        }
        fn list_windows(&mut self) -> io::Result<Vec<WindowInfo>> {
            if self.fail_list {
                return Err(io::Error::other("connection refused"));
            }
            self.order.push("list");
            Ok(self.windows.clone())
        }
    }

    const INSTANCE: &str = "/run/user/1000/niri.wayland-1.1572.sock";

    fn workspace(id: u64, focused: bool) -> WorkspaceInfo {
        WorkspaceInfo {
            id,
            focused,
            output: Some("eDP-1".to_string()),
        }
    }

    fn window(
        id: u64,
        ws: Option<u64>,
        floating: bool,
        geo: Option<Geometry>,
        focused: bool,
    ) -> WindowInfo {
        WindowInfo {
            id,
            workspace: ws,
            floating,
            focused,
            geometry: geo,
            focus_timestamp: None,
        }
    }

    /// `w` as niri reports it after it held focus at `secs`.
    fn focused_at(mut w: WindowInfo, secs: u64) -> WindowInfo {
        w.focus_timestamp = Some(FocusTimestamp { secs, nanos: 0 });
        w
    }

    fn snapshot(windows: Vec<WindowInfo>) -> CompositorSnapshot {
        CompositorSnapshot {
            windows,
            workspaces: vec![workspace(1, true)],
            outputs: HashMap::from([("eDP-1".to_string(), (1828.0, 1142.0))]),
        }
    }

    fn layouts(changes: Vec<(u64, Geometry)>) -> Vec<CompositorEvent> {
        changes
            .into_iter()
            .map(|(id, geo)| CompositorEvent::GeometryChanged { id, geometry: geo })
            .collect()
    }

    fn changed(w: WindowInfo) -> Vec<CompositorEvent> {
        vec![CompositorEvent::WindowChanged(w)]
    }

    fn geo(x: f64, y: f64, w: f64, h: f64) -> Geometry {
        Geometry { x, y, w, h }
    }

    const ORIGINAL: Geometry = Geometry {
        x: 100.0,
        y: 400.0,
        w: 600.0,
        h: 300.0,
    }; // centre (400, 550): left is the nearest edge
    const SLIVER: Geometry = Geometry {
        x: -525.0,
        y: 400.0,
        w: 600.0,
        h: 300.0,
    }; // where niri clamps a 600-wide window pushed left

    #[test]
    fn a_press_with_no_floating_windows_on_the_focused_workspace_replies_ok_and_does_nothing() {
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let mut io = Fake::default();
        let snap = snapshot(vec![window(52, Some(1), false, None, true)]);
        let reply = engine.on_press(HotkeyRequest::Toggle, &snap, &mut io);
        assert_eq!(reply, HotkeyReply::Ok);
        assert!(io.actions.is_empty());
        assert!(io.saves.is_empty());
    }

    #[test]
    fn a_toggle_with_an_unstashed_window_saves_desired_state_stashes_it_and_reads_it_back() {
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let mut io = Fake {
            windows: vec![window(72, Some(1), true, Some(SLIVER), false)],
            ..Fake::default()
        };
        let snap = snapshot(vec![
            window(72, Some(1), true, Some(ORIGINAL), false),
            window(52, Some(1), false, None, true),
        ]);
        let reply = engine.on_press(HotkeyRequest::Toggle, &snap, &mut io);
        assert_eq!(reply, HotkeyReply::Ok, "the reply is final");
        assert_eq!(io.order, vec!["save", "act", "list"]);
        let saved = &io.saves[0].windows[&72];
        assert_eq!(
            *saved,
            WindowEntry {
                original: ORIGINAL,
                desired: DesiredState::Stashed { edge: Edge::Left }
            }
        );
        let expected = vec![Command::Nudge {
            id: 72,
            by: geometry::push_toward(Edge::Left),
        }];
        assert_eq!(io.actions, expected);
        assert_eq!(engine.observed[&72], SLIVER, "read back from niri");
    }

    #[test]
    fn a_press_whose_state_write_fails_moves_nothing_and_reports_state_unwritable() {
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let mut io = Fake {
            fail_save: true,
            windows: vec![window(72, Some(1), true, Some(SLIVER), false)],
            ..Fake::default()
        };
        let snap = snapshot(vec![window(72, Some(1), true, Some(ORIGINAL), false)]);
        let reply = engine.on_press(HotkeyRequest::Toggle, &snap, &mut io);
        assert!(
            matches!(&reply, HotkeyReply::Err { code: ReplyErrorCode::StateUnwritable, message } if message.contains("Read-only")),
            "{reply:?}"
        );
        assert!(io.actions.is_empty());
        io.fail_save = false;
        // Nothing about the window is remembered: a hand move of it is not a change to react to.
        engine.on_compositor_events(layouts(vec![(72, SLIVER)]), &mut io);
        assert!(
            io.saves.is_empty(),
            "no state write for an untracked window"
        );
        // The window is not left half-tracked: a later successful press treats it as unstashed.
        assert_eq!(
            engine.on_press(HotkeyRequest::Toggle, &snap, &mut io),
            HotkeyReply::Ok
        );
        assert_eq!(io.saves[0].windows.len(), 1);
    }

    /// Stash window 72 (left edge) and clear the recorders.
    fn stashed_72() -> (StashEngine, Fake) {
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let mut io = Fake {
            windows: vec![
                window(72, Some(1), true, Some(SLIVER), false),
                window(52, Some(1), false, None, true),
            ],
            ..Fake::default()
        };
        let snap = snapshot(vec![
            window(72, Some(1), true, Some(ORIGINAL), false),
            window(52, Some(1), false, None, true),
        ]);
        assert_eq!(
            engine.on_press(HotkeyRequest::Toggle, &snap, &mut io),
            HotkeyReply::Ok
        );
        io.order.clear();
        io.actions.clear();
        io.saves.clear();
        (engine, io)
    }

    #[test]
    fn the_report_of_the_daemons_own_move_is_not_a_hand_change() {
        let (mut engine, mut io) = stashed_72();
        engine.on_compositor_events(layouts(vec![(72, SLIVER)]), &mut io);
        assert!(engine.is_stashed(72), "still stashed");
        assert!(io.actions.is_empty() && io.saves.is_empty());
    }

    #[test]
    fn a_toggle_with_everything_stashed_restores_each_window_focuses_it_and_drops_it() {
        let (mut engine, mut io) = stashed_72();
        let windows = vec![
            window(72, Some(1), true, Some(SLIVER), false),
            window(52, Some(1), false, None, true),
        ];
        io.windows = vec![window(72, Some(1), true, Some(ORIGINAL), true)];
        assert_eq!(
            engine.on_press(HotkeyRequest::Toggle, &snapshot(windows), &mut io),
            HotkeyReply::Ok
        );
        assert_eq!(
            io.order,
            vec!["save", "act", "list", "save"],
            "the restore is written before the move and the drop after it"
        );
        assert_eq!(io.saves[0].windows[&72].desired, DesiredState::Restored);
        let expected = vec![
            Command::Nudge {
                id: 72,
                by: geometry::nudge_between(SLIVER, ORIGINAL).unwrap(),
            },
            Command::FocusWindow(72),
        ];
        assert_eq!(io.actions, expected);
        assert!(engine.desired.is_empty());
        assert!(io.saves[1].windows.is_empty());
    }

    #[test]
    fn a_restore_focuses_the_stashed_window_niri_saw_focused_last_or_the_first_without_timestamps()
    {
        let b = geo(1500.0, 100.0, 200.0, 200.0);
        let b_sliver = geo(1753.0, 100.0, 200.0, 200.0);
        let stash_both = |io: &mut Fake| {
            let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
            io.windows = vec![
                window(72, Some(1), true, Some(SLIVER), false),
                window(73, Some(1), true, Some(b_sliver), false),
            ];
            let snap = snapshot(vec![
                window(72, Some(1), true, Some(ORIGINAL), false),
                window(73, Some(1), true, Some(b), false),
                window(52, Some(1), false, None, true),
            ]);
            assert_eq!(
                engine.on_press(HotkeyRequest::Toggle, &snap, io),
                HotkeyReply::Ok
            );
            io.actions.clear();
            engine
        };
        // 73 was focused more recently than 72, by niri's own debounced record.
        let mut io = Fake::default();
        let mut engine = stash_both(&mut io);
        let snap = snapshot(vec![
            focused_at(window(72, Some(1), true, Some(SLIVER), false), 10),
            focused_at(window(73, Some(1), true, Some(b_sliver), false), 20),
            window(52, Some(1), false, None, true),
        ]);
        engine.on_press(HotkeyRequest::Unstash, &snap, &mut io);
        assert!(
            io.actions.contains(&Command::FocusWindow(73)),
            "{:?}",
            io.actions
        );
        // Without timestamps the first stashed window on the workspace is focused.
        let mut io = Fake::default();
        let mut engine = stash_both(&mut io);
        let snap = snapshot(vec![
            window(72, Some(1), true, Some(SLIVER), false),
            window(73, Some(1), true, Some(b_sliver), false),
        ]);
        engine.on_press(HotkeyRequest::Unstash, &snap, &mut io);
        assert!(
            io.actions.contains(&Command::FocusWindow(72)),
            "{:?}",
            io.actions
        );
    }

    #[test]
    fn a_stash_request_stashes_what_is_left_and_never_restores() {
        // One window already stashed, one not: only the unstashed one is sent anywhere.
        let (mut engine, mut io) = stashed_72();
        let b = geo(1500.0, 100.0, 200.0, 200.0);
        let b_sliver = geo(1753.0, 100.0, 200.0, 200.0);
        io.windows = vec![
            window(72, Some(1), true, Some(SLIVER), false),
            window(73, Some(1), true, Some(b_sliver), false),
        ];
        let snap = snapshot(vec![
            window(72, Some(1), true, Some(SLIVER), false),
            window(73, Some(1), true, Some(b), false),
            window(52, Some(1), false, None, true),
        ]);
        assert_eq!(
            engine.on_press(HotkeyRequest::Stash, &snap, &mut io),
            HotkeyReply::Ok
        );
        assert_eq!(
            io.actions,
            vec![Command::Nudge {
                id: 73,
                by: geometry::push_toward(Edge::Top)
            }]
        );
        assert!(engine.is_stashed(72) && engine.is_stashed(73));
        // Everything stashed: a stash request does nothing, unlike a toggle.
        io.actions.clear();
        io.saves.clear();
        let snap = snapshot(vec![
            window(72, Some(1), true, Some(SLIVER), false),
            window(73, Some(1), true, Some(b_sliver), false),
        ]);
        assert_eq!(
            engine.on_press(HotkeyRequest::Stash, &snap, &mut io),
            HotkeyReply::Ok
        );
        assert!(io.actions.is_empty() && io.saves.is_empty());
        assert!(engine.is_stashed(72) && engine.is_stashed(73));
    }

    #[test]
    fn stashing_the_focused_window_moves_focus_to_tiling_only_when_a_tiled_window_exists() {
        // With a tiled window on the workspace: focus-tiling is requested after the stash.
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let mut io = Fake::default();
        let snap = snapshot(vec![
            window(72, Some(1), true, Some(ORIGINAL), true),
            window(52, Some(1), false, None, false),
        ]);
        engine.on_press(HotkeyRequest::Toggle, &snap, &mut io);
        assert!(
            matches!(io.actions.last(), Some(Command::FocusTiling)),
            "{:?}",
            io.actions.last()
        );
        // Without one: nothing is requested, because niri would ignore it.
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let mut io = Fake::default();
        let snap = snapshot(vec![window(72, Some(1), true, Some(ORIGINAL), true)]);
        engine.on_press(HotkeyRequest::Toggle, &snap, &mut io);
        assert!(!io.actions.iter().any(|a| matches!(a, Command::FocusTiling)));
    }

    #[test]
    fn alt_tab_into_a_stashed_window_restores_only_that_window() {
        let (mut engine, mut io) = stashed_72();
        io.windows = vec![window(72, Some(1), true, Some(ORIGINAL), true)];
        // Focus was on 52 before; the switcher takes it away, then lands on the sliver.
        engine.on_compositor_events([CompositorEvent::FocusChanged(Some(52))], &mut io);
        engine.on_compositor_events([CompositorEvent::FocusChanged(None)], &mut io);
        assert!(io.actions.is_empty(), "nothing happens while focus is away");
        engine.on_compositor_events([CompositorEvent::FocusChanged(Some(72))], &mut io);
        assert_eq!(io.order, vec!["save", "act", "list", "save"]);
        assert_eq!(io.saves[0].windows[&72].desired, DesiredState::Restored);
        let expected = vec![Command::Nudge {
            id: 72,
            by: geometry::nudge_between(SLIVER, ORIGINAL).unwrap(),
        }];
        assert_eq!(
            io.actions, expected,
            "no focus request: focus is already there"
        );
        assert!(engine.desired.is_empty());
        // Returning to the same window later does nothing.
        io.actions.clear();
        engine.on_compositor_events([CompositorEvent::FocusChanged(None)], &mut io);
        engine.on_compositor_events([CompositorEvent::FocusChanged(Some(72))], &mut io);
        assert!(io.actions.is_empty());
    }

    #[test]
    fn under_any_focus_a_direct_focus_change_onto_a_stashed_window_restores_it() {
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AnyFocus);
        let mut io = Fake {
            windows: vec![window(72, Some(1), true, Some(SLIVER), false)],
            ..Fake::default()
        };
        let snap = snapshot(vec![
            window(72, Some(1), true, Some(ORIGINAL), false),
            window(52, Some(1), false, None, true),
        ]);
        assert_eq!(
            engine.on_press(HotkeyRequest::Toggle, &snap, &mut io),
            HotkeyReply::Ok
        );
        io.actions.clear();
        io.windows = vec![window(72, Some(1), true, Some(ORIGINAL), true)];
        // Focus goes straight from the tiled window to the stashed one (keyboard, no switcher).
        engine.on_compositor_events([CompositorEvent::FocusChanged(Some(72))], &mut io);
        assert_eq!(
            io.actions,
            vec![Command::Nudge {
                id: 72,
                by: geometry::nudge_between(SLIVER, ORIGINAL).unwrap()
            }]
        );
        assert!(engine.desired.is_empty());
    }

    #[test]
    fn a_window_missing_from_the_read_back_is_dropped_and_the_press_still_succeeds() {
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let b = geo(1500.0, 100.0, 200.0, 200.0);
        let windows = vec![
            window(72, Some(1), true, Some(ORIGINAL), false),
            window(73, Some(1), true, Some(b), false),
        ];
        // niri no longer lists 73 by the time the batch is answered.
        let mut io = Fake {
            windows: vec![window(72, Some(1), true, Some(SLIVER), false)],
            ..Fake::default()
        };
        assert_eq!(
            engine.on_press(HotkeyRequest::Toggle, &snapshot(windows), &mut io),
            HotkeyReply::Ok
        );
        assert!(engine.is_stashed(72));
        assert!(!engine.desired.contains_key(&73));
        let last = io.saves.last().unwrap();
        assert!(
            last.windows.contains_key(&72) && !last.windows.contains_key(&73),
            "the drop is written"
        );
    }

    #[test]
    fn a_closed_stashed_window_is_dropped_and_the_drop_is_written() {
        let (mut engine, mut io) = stashed_72();
        engine.on_compositor_events([CompositorEvent::WindowClosed(72)], &mut io);
        assert!(engine.desired.is_empty());
        assert!(io.saves.last().unwrap().windows.is_empty());
        assert!(io.actions.is_empty());
    }

    #[test]
    fn when_the_windows_cannot_be_read_back_the_moved_windows_are_forgotten() {
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let mut io = Fake {
            fail_list: true,
            ..Fake::default()
        };
        let snap = snapshot(vec![window(72, Some(1), true, Some(ORIGINAL), false)]);
        assert_eq!(
            engine.on_press(HotkeyRequest::Toggle, &snap, &mut io),
            HotkeyReply::Ok
        );
        assert!(engine.desired.is_empty(), "nothing is tracked blind");
        assert!(io.saves.last().unwrap().windows.is_empty());
    }

    #[test]
    fn startup_reapplies_saved_desired_state_against_the_live_windows() {
        let orig73 = geo(1200.0, 100.0, 400.0, 200.0);
        let orig75 = geo(300.0, 300.0, 300.0, 300.0);
        let mut saved = StateFile {
            instance: INSTANCE.to_string(),
            windows: BTreeMap::new(),
        };
        saved.windows.insert(
            72,
            WindowEntry {
                original: ORIGINAL,
                desired: DesiredState::Stashed { edge: Edge::Left },
            },
        );
        saved.windows.insert(
            73,
            WindowEntry {
                original: orig73,
                desired: DesiredState::Restored,
            },
        );
        saved.windows.insert(
            74,
            WindowEntry {
                original: ORIGINAL,
                desired: DesiredState::Stashed { edge: Edge::Left },
            },
        );
        saved.windows.insert(
            75,
            WindowEntry {
                original: orig75,
                desired: DesiredState::Stashed { edge: Edge::Top },
            },
        );
        let sliver73 = geo(1753.0, 100.0, 400.0, 200.0);
        let live = vec![
            window(52, Some(1), false, None, true),
            window(72, Some(1), true, Some(SLIVER), false),
            window(73, Some(1), true, Some(sliver73), false),
            window(75, Some(1), false, None, false),
        ];
        let mut io = Fake {
            windows: live.clone(),
            ..Fake::default()
        };
        let mut engine = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        engine.start_from_saved_state(LoadOutcome::Usable(saved), &snapshot(live), &mut io);
        assert_eq!(
            engine.focus.current(),
            Some(52),
            "focus seeded from the snapshot"
        );
        assert!(engine.is_stashed(72), "re-applied and kept");
        assert!(
            !engine.desired.contains_key(&73),
            "a restore is done once sent"
        );
        assert!(
            !engine.desired.contains_key(&74),
            "a vanished window is dropped"
        );
        assert!(
            !engine.desired.contains_key(&75),
            "a tiled window is not tracked"
        );
        let expected = vec![
            Command::Nudge {
                id: 72,
                by: geometry::push_toward(Edge::Left),
            },
            Command::Nudge {
                id: 73,
                by: geometry::nudge_between(sliver73, orig73).unwrap(),
            },
        ];
        assert_eq!(io.actions, expected);
        assert_eq!(
            io.order.first(),
            Some(&"save"),
            "the file is rewritten before any command"
        );
        // Nothing to re-apply from a fresh load.
        let mut fresh = StashEngine::new(INSTANCE.to_string(), FocusPolicy::AltTabOnly);
        let mut io2 = Fake::default();
        fresh.start_from_saved_state(
            LoadOutcome::StartEmpty("no state file".into()),
            &snapshot(vec![]),
            &mut io2,
        );
        assert!(io2.actions.is_empty() && io2.saves.is_empty());
    }

    #[test]
    fn a_restore_of_a_window_already_at_its_original_sends_nothing_and_drops_it() {
        let (mut engine, mut io) = stashed_72();
        // Something (a hand that the engine never saw) already put it back exactly.
        let windows = vec![window(72, Some(1), true, Some(ORIGINAL), false)];
        assert_eq!(
            engine.on_press(HotkeyRequest::Unstash, &snapshot(windows), &mut io),
            HotkeyReply::Ok
        );
        assert!(
            io.actions
                .iter()
                .all(|a| matches!(a, Command::FocusWindow(_))),
            "{:?}",
            io.actions
        );
        assert!(!engine.desired.contains_key(&72));
    }

    #[test]
    fn any_hand_change_to_a_stashed_window_forgets_it_at_once() {
        let (mut engine, mut io) = stashed_72();
        let moved = geo(500.0, 300.0, SLIVER.w, SLIVER.h);
        engine.on_compositor_events(layouts(vec![(72, moved)]), &mut io);
        assert!(!engine.desired.contains_key(&72), "forgotten");
        assert!(io.actions.is_empty(), "nothing is sent back");
        assert!(
            io.saves.last().unwrap().windows.is_empty(),
            "the drop was written"
        );
    }

    #[test]
    fn a_stashed_window_that_is_picked_up_or_tiled_is_forgotten() {
        let (mut engine, mut io) = stashed_72();
        engine.on_compositor_events(changed(window(72, None, true, None, true)), &mut io);
        assert!(!engine.desired.contains_key(&72), "picked up: forgotten");

        let (mut engine, mut io) = stashed_72();
        engine.on_compositor_events(changed(window(72, Some(1), false, None, true)), &mut io);
        assert!(!engine.desired.contains_key(&72), "tiled: forgotten");
        assert!(io.actions.is_empty());
    }
}
