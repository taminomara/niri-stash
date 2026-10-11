//! Which focus changes mean "the user went to that window on purpose". Pure.
//!
//! With focus-follows-mouse on, only the switcher is trusted (spec D6): focus leaves every window
//! (`None`) and then lands on a window other than the one it left. With it off, every change to
//! another window is deliberate. In both policies a return to the window focus came from is not a
//! landing, focus changes the daemon itself requested never count, and a window-changed report
//! naming the window that already holds focus is not a transition.

use crate::core::WindowId;

/// Which focus changes count as the user landing on a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    /// Only a landing through the switcher: focus-follows-mouse makes every other change suspect.
    #[default]
    AltTabOnly,
    /// Any change to another window: focus-follows-mouse is off, so each one is deliberate.
    AnyFocus,
}

/// Tracks the last two focus states and pending expectations under a `FocusPolicy`.
#[derive(Debug, Default)]
pub(crate) struct FocusDetector {
    policy: FocusPolicy,
    /// The focus state before `current`.
    previous: Option<WindowId>,
    /// The window that holds focus now, if any.
    current: Option<WindowId>,
    /// Focus changes the daemon requested and has not yet seen land, in request order.
    expected: Vec<Option<WindowId>>,
}

impl FocusDetector {
    /// Start with the window that currently has focus, if any.
    pub(crate) fn new(current: Option<WindowId>, policy: FocusPolicy) -> Self {
        let mut detector = Self {
            policy,
            ..Self::default()
        };
        detector.set_current(current);
        detector
    }

    /// Feed a `WindowFocusChanged { id }` event. Returns `Some(id)` when this event is a landing
    /// on `id` under the policy (the engine then checks whether `id` is stashed).
    pub(crate) fn observe_focus(&mut self, id: Option<WindowId>) -> Option<WindowId> {
        let expected = self.consume_expectation(id);
        let landing = match (self.policy, self.previous, self.current, id) {
            (FocusPolicy::AltTabOnly, Some(a), None, Some(b)) if a != b => Some(b),
            (FocusPolicy::AltTabOnly, ..) => None,
            // Focus came back to where it was, e.g. the overview closed.
            (FocusPolicy::AnyFocus, Some(a), None, Some(b)) if a == b => None,
            (FocusPolicy::AnyFocus, _, current, Some(b)) if current != Some(b) => Some(b),
            (FocusPolicy::AnyFocus, ..) => None,
        };
        self.set_current(id);
        landing.filter(|_| !expected)
    }

    /// Feed a `WindowOpenedOrChanged` report for window `id` carrying `is_focused`. A report for
    /// the window that already holds focus is ignored; one that newly gains focus is treated like
    /// `observe_focus(Some(id))`.
    pub(crate) fn observe_window_report(
        &mut self,
        id: WindowId,
        is_focused: bool,
    ) -> Option<WindowId> {
        if !is_focused || self.current == Some(id) {
            return None;
        }
        self.observe_focus(Some(id))
    }

    /// Register a focus change the daemon is about to request (spec D8), so it never counts as
    /// a landing. `None` means "focus moves to the tiled layout".
    pub(crate) fn expect_focus_request(&mut self, id: Option<WindowId>) {
        self.expected.push(id);
    }

    /// Drop every pending expectation (at the start of the next press; a request niri ignored
    /// must not mask a real landing later).
    pub(crate) fn clear_expectations(&mut self) {
        self.expected.clear();
    }

    /// The window that currently holds focus, if any. A test probe.
    #[cfg(test)]
    pub(crate) fn current(&self) -> Option<WindowId> {
        self.current
    }

    /// Make `id` the current focus.
    fn set_current(&mut self, id: Option<WindowId>) {
        self.previous = self.current;
        self.current = id;
    }

    /// Remove the expectation this focus change fulfils, if any, and say whether one was found.
    fn consume_expectation(&mut self, id: Option<WindowId>) -> bool {
        let Some(index) = self.expected.iter().position(|e| e.is_none() || *e == id) else {
            return false;
        };
        self.expected.remove(index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_the_given_current_focus() {
        let detector = FocusDetector::new(Some(5), FocusPolicy::AltTabOnly);
        assert_eq!(detector.current(), Some(5));
        assert_eq!(
            FocusDetector::new(None, FocusPolicy::AltTabOnly).current(),
            None
        );
    }

    #[test]
    fn none_then_a_different_window_is_an_alt_tab_landing_on_that_window() {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AltTabOnly);
        assert_eq!(detector.observe_focus(None), None);
        assert_eq!(detector.observe_focus(Some(2)), Some(2));
        assert_eq!(detector.current(), Some(2));
    }

    #[test]
    fn none_then_the_same_window_is_not_an_alt_tab() {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AltTabOnly);
        detector.observe_focus(None);
        assert_eq!(detector.observe_focus(Some(1)), None);
        assert_eq!(detector.current(), Some(1));
    }

    #[test]
    fn a_direct_window_to_window_change_is_not_an_alt_tab() {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AltTabOnly);
        assert_eq!(detector.observe_focus(Some(2)), None);
        assert_eq!(detector.current(), Some(2));
    }

    #[test]
    fn none_with_no_window_before_it_then_a_window_is_not_an_alt_tab() {
        let mut detector = FocusDetector::new(None, FocusPolicy::AltTabOnly);
        assert_eq!(detector.observe_focus(None), None);
        assert_eq!(detector.observe_focus(Some(2)), None);
        assert_eq!(detector.current(), Some(2));
    }

    #[test]
    fn an_expected_landing_on_a_specific_window_is_consumed_and_the_expectation_is_gone() {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AltTabOnly);
        detector.observe_focus(None);
        detector.expect_focus_request(Some(2));
        // The expected landing updates state but does not count.
        assert_eq!(detector.observe_focus(Some(2)), None);
        assert_eq!(detector.current(), Some(2));
        // The expectation was consumed: the same shape again counts.
        detector.observe_focus(None);
        assert_eq!(detector.observe_focus(Some(3)), Some(3));
        // An expectation for another window does not swallow this landing.
        detector.expect_focus_request(Some(9));
        detector.observe_focus(None);
        assert_eq!(detector.observe_focus(Some(4)), Some(4));
    }

    #[test]
    fn an_expectation_of_any_consumes_the_next_focus_change_of_any_kind() {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AltTabOnly);
        detector.observe_focus(None);
        detector.expect_focus_request(None);
        // Focus lands on a window the daemon did not name: consumed, does not count.
        assert_eq!(detector.observe_focus(Some(2)), None);
        assert_eq!(detector.current(), Some(2));
        // Consumed once only: the next alt-tab shape counts again.
        detector.observe_focus(None);
        assert_eq!(detector.observe_focus(Some(3)), Some(3));
    }

    #[test]
    fn clear_expectations_drops_pending_expectations() {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AltTabOnly);
        detector.expect_focus_request(Some(2));
        detector.expect_focus_request(None);
        detector.clear_expectations();
        detector.observe_focus(None);
        assert_eq!(detector.observe_focus(Some(2)), Some(2));
    }

    #[test]
    fn a_window_changed_report_for_the_focused_window_changes_nothing_and_a_newly_focused_one_acts_like_a_focus_change(
    ) {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AltTabOnly);
        detector.observe_focus(None);
        // An unfocused report is not a transition.
        assert_eq!(detector.observe_window_report(2, false), None);
        assert_eq!(detector.current(), None);
        // Re-report of the already-focused window (a drag): nothing changes.
        let mut dragging = FocusDetector::new(Some(1), FocusPolicy::AltTabOnly);
        dragging.observe_focus(None);
        dragging.observe_focus(Some(1));
        assert_eq!(dragging.observe_window_report(1, true), None);
        assert_eq!(dragging.current(), Some(1));
        // A different window reported focused completes the alt-tab shape.
        assert_eq!(detector.observe_window_report(2, true), Some(2));
        assert_eq!(detector.current(), Some(2));
    }

    /// Feed a captured niri event stream through the adapter's translation and collect the
    /// alt-tab landings the detector reports.
    #[test]
    fn under_any_focus_a_direct_change_to_another_window_is_a_landing() {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AnyFocus);
        assert_eq!(detector.observe_focus(Some(2)), Some(2));
        assert_eq!(detector.observe_focus(Some(2)), None, "no change");
        assert_eq!(detector.observe_focus(None), None);
        assert_eq!(detector.observe_focus(Some(3)), Some(3));
        assert_eq!(
            FocusDetector::new(None, FocusPolicy::AnyFocus).observe_focus(Some(4)),
            Some(4),
            "the first window to take focus counts"
        );
    }

    #[test]
    fn under_any_focus_a_return_to_the_window_focus_left_and_an_expected_change_do_not_count() {
        let mut detector = FocusDetector::new(Some(1), FocusPolicy::AnyFocus);
        detector.observe_focus(None);
        assert_eq!(
            detector.observe_focus(Some(1)),
            None,
            "overview closed, focus came back"
        );
        detector.expect_focus_request(Some(2));
        assert_eq!(
            detector.observe_focus(Some(2)),
            None,
            "the daemon asked for this"
        );
        assert_eq!(
            detector.observe_window_report(2, true),
            None,
            "already focused"
        );
    }

    fn replay(log: &str) -> Vec<u64> {
        use crate::core::input::CompositorEvent;
        use crate::niri::translate::events_from_niri;
        let mut detector = FocusDetector::new(None, FocusPolicy::AltTabOnly);
        let mut landings = Vec::new();
        for line in log.lines().filter(|l| !l.trim().is_empty()) {
            let event: niri_ipc::Event = serde_json::from_str(line).unwrap();
            for input in events_from_niri(&event) {
                let landed = match input {
                    CompositorEvent::FocusChanged(id) => detector.observe_focus(id),
                    CompositorEvent::WindowChanged(w) => {
                        detector.observe_window_report(w.id, w.focused)
                    }
                    _ => None,
                };
                landings.extend(landed);
            }
        }
        landings
    }

    #[test]
    fn replay_base_log_yields_no_alt_tab() {
        let landings = replay(include_str!(
            "../../tests/fixtures/niri-event-log-2026-10-11.jsonl"
        ));
        assert_eq!(landings, Vec::<u64>::new());
    }

    #[test]
    fn replay_log_2_yields_exactly_the_switcher_landing_on_73() {
        let landings = replay(include_str!(
            "../../tests/fixtures/niri-event-log-2026-10-11-2.jsonl"
        ));
        assert_eq!(landings, vec![73]);
    }

    #[test]
    fn replay_logs_1_and_3_yield_none_and_logs_4_and_5_yield_52() {
        let log_1 = include_str!("../../tests/fixtures/niri-event-log-2026-10-11-1.jsonl");
        let log_3 = include_str!("../../tests/fixtures/niri-event-log-2026-10-11-3.jsonl");
        let log_4 = include_str!("../../tests/fixtures/niri-event-log-2026-10-11-4.jsonl");
        let log_5 = include_str!("../../tests/fixtures/niri-event-log-2026-10-11-5.jsonl");
        assert_eq!(replay(log_1), Vec::<u64>::new(), "log 1");
        assert_eq!(replay(log_3), Vec::<u64>::new(), "log 3");
        assert_eq!(replay(log_4), vec![52], "log 4");
        assert_eq!(replay(log_5), vec![52], "log 5");
    }
}
