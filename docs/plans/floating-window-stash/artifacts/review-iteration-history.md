# Review Iteration History: Floating Window Stash for niri (feature specification)

<!--
This file records how the review of the feature specification evolved. Findings live in
[review-findings.md](review-findings.md); the specification lives one directory up. Lightweight
mode caps at 1 iteration for a small plan.

Cross-referencing invariants:
- `Findings raised:` — F# IDs from review-findings.md.
- `Changed in plan:` — sections of ../feature-specification.md updated this round.
- `Changed in tech-notes:` — T# IDs in feature-technical-notes.md added or edited this round.
-->

## R1: Iteration 1 — gate-trip corrections from implementation planning

- **Mode:** lightweight
- **Spec-aware mode:** engaged
- **Specialists engaged:** self-review
- **New input provided:** the approved specification and its companions; the implementation-planning
  round-1 claim ledger (C8, C10, C13) and the gate-trip facilitator's findings M1–M5
  ([implementation-iteration-history.md](implementation-iteration-history.md)); the niri v26.04
  source verifications in [.discovery-notes.md](.discovery-notes.md); the user's choice to pause
  implementation planning for this pass (escalation E1 there).
- **What was checked:**
  - Assumptions. Primary: (A1) niri reports a failed action for a vanished window — **refuted**
    (`src/ipc/server.rs` 381–399; `src/input/mod.rs` ~1986) → F18. (A2) an in-flight entry at
    startup always belongs to a window that moved — **refuted** by T4's own write ordering → F19.
    (A3) the only interruptible operation is a stash — **refuted** (T4 marks unstash in flight too;
    one-phase unstash) → F20. (A4) the state file is always writable — **uncertain**, tmpfs, so
    treated as a failure mode → F21. Secondary: (A5) D13 covers every path out of the floating layout
    — **refuted** for mid-press → F22. All other T1–T3 mechanics were confirmed by both upstream
    specialists and were not re-opened.
  - Overlap. Internal: none; the new rows do not duplicate existing ones. External: no code exists
    yet; nothing to overlap.
  - Contract check. Contracts the spec references: the hotkey ⇄ daemon protocol (OI-1, delegated to
    implementation planning by design and pinned there); the state file (private to the daemon, not
    a contract). No contract is introduced by this round.
  - Mechanics leaking into spec: the new rows and the amended restart flow were written
    behaviorally; no mechanic entered the spec. The mechanics went to T4 and T5.
  - YAGNI: the five corrections each rest on a verified failure mode or on D10's existing
    commitment; none adds a knob, abstraction, or observability. No YAGNI candidate raised.
- **Questions surfaced to user:** none asked. The user redirected mid-pass: record desired state,
  write it before sending commands, and re-apply it at startup instead of reconciling observed
  geometry. Applied to F19, F20, F21 (resolved by user input) and to T4, D10, the restart flow, and
  the Coordinations row.
- **Findings raised:** F18, F19, F20, F21, F22, F23, F24, F25 (F24 and F25 added after the pass closed: F24 from implementation-planning escalation E2, F25 from a user observation about niri's layout reports)
- **Changed in plan:** Outcome; Actors and Triggers; Alternate Flows and States; Edge Cases and Failure Modes; User Interactions; Deferred (YAGNI)
- **Changed in tech-notes:** T1, T2, T3, T4, T5
- **Stability assessment:** structural changes medium (restart model replaced by desired-state
  re-apply at the user's direction; one flow and D10/T4 rewritten; three rows added, one row
  removed); probability of meaningful improvement next round below 80%; recommendation stop.
- **Next-step recommendation:** Stop — the specification has converged on the five inputs. Resume
  `han-planning:plan-implementation` from its round-1 record.
