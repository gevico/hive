# Goal Tracker

<!--
RULES:
- IMMUTABLE SECTION: Do not modify after initialization
- MUTABLE SECTION: Update each round, but document all changes
- Every task must be in one of: Active, Completed, or Deferred
- Deferred items require explicit justification
-->

## IMMUTABLE SECTION
<!-- Do not modify after initialization -->

### Ultimate Goal

Implement the Hive multi-agent orchestration harness as a single Rust CLI binary (`hive`) that enables team-based multi-agent task execution through a three-layer architecture (Orchestrator → Sub-agent → Worker). The v1 delivers an end-to-end execution chain: init → plan (spec + plan.md) → rfc (team review) → approve → exec (isolated worktree execution with acceptance verification) → merge (PR-based integration). Claude Code and Codex CLI are first-class agent backends with a generic CLI fallback for extensibility.

**v1 Scope**: init → plan (spec + plan.md, external tools) → rfc (aggregates spec + plan) → approve → exec (claim → isolate → launch → check → report) → merge (PR mode) → status → audit → doctor

**Deferred to v2**: 7-phase interactive planning flow, full skill install/uninstall/marketplace, pause/resume/checkpoint, auto-conflict-resolution, doctor auto-repair, aggregated per-draft audit reports, merge queue integration

### Acceptance Criteria

- AC-1: `hive init` creates `.hive/` directory structure, config files, .gitignore entries, agent adapter generation, humanize plugin installation
- AC-2: Dual-layer YAML config with deep merge; `hive config --show` with source annotations
- AC-3: 7-state task state machine (pending/assigned/in_progress/review/completed/failed/blocked) with hard-coded transition rules
- AC-4: Task ID and Draft ID use `<user_name>-<ulid>` format; spec file format with YAML frontmatter; plan file at `plans/<draft_id>/<task_id>.md`
- AC-5: Per-task state.json with schema_version; state.md is derived view only
- AC-6: Git worktree lifecycle: create at `.hive/worktrees/<task_id>/`, branch `hive/<task_id>`, cleanup after merge
- AC-7: Concurrency control: per-task flock, orchestrator lock, stale detection, atomic writes (tmp+rename)
- AC-8: `hive exec` orchestrates full chain: dependency graph, scheduling, retry logic (up to 3)
- AC-9: `hive launch` starts agent in worktree; Claude Code, Codex CLI, generic CLI backends
- AC-10: `hive check` with 3 verifier types (command/file/manual); exit codes 0/1/2/3
- AC-11: `hive report` reads result.md, validates frontmatter, updates state.json; exit codes 0/1/2
- AC-12: `hive merge` rebases onto main + PR/MR creation; `--all` dependency-ordered; `--mode direct`
- AC-13: RFC workflow: `hive rfc --draft` aggregates specs+plans into rfcs/, `hive approve --draft` gates exec
- AC-14: Audit system: 3-level logging (minimal/standard/full), append-only per-task audit.md
- AC-15: Skill loading from 3-tier sources (repo > user > system), priority resolution, launch injection
- AC-16: Schema versioning: reject unknown major (>1), warn unknown fields, default to v1
- AC-17: Numeric constraints: retry_limit=3, S/M/L→RLCR 2/5/8, frontmatter≤1024, description≤500
- AC-18: `hive doctor` validates environment, config, state consistency, stale locks, worktree health
- AC-19: Agent tool adapter generation: Claude Code plugin, Codex instructions/hooks, generic fallback, humanize default plugin

---

## MUTABLE SECTION

### Plan Version: 6 (Updated: Round 2 Execution-Path Closure)

#### Plan Evolution Log
| Round | Change | Reason | Impact on AC |
|-------|--------|--------|--------------|
| 0 | Initial plan | - | - |
| 0 | Review correction: reopened task3, task5-task7, and task10-task22 | Code audit found the end-to-end chain, adapter generation, audit/skill wiring, and analyze deliverables were not complete despite completion claims | AC-1, AC-4-AC-19 |
| 1 | Narrowed Round 1 to the execution-path repair: real Layer 1 orchestration plus deterministic verification behavior | `hive exec` can currently complete work without the planned command chain, so the round must stay focused on the highest-risk mainline gap | AC-8, AC-10, with AC-11 treated as a blocking handoff dependency |
| 1-review | Rejected closing Round 1 on the narrowed slice alone; restored full-plan pressure and reopened execution preflight gaps | The implementation advanced the Layer 1 pipeline, but `exec` still mishandles skipped plans/blocked dependencies and still suppresses spec-parse failures during dependency resolution, while task7/task14-task22 remain untouched | AC-1, AC-8, AC-9, AC-12-AC-19 |
| 2 | Re-anchored Round 2 on deterministic `exec` preflight and scheduler convergence | The newest review shows AC-8 is still the highest-risk incomplete path; strict preflight and fixture coverage are required before the broader plan can safely continue | AC-8 primarily, with AC-10 preserved as a blocking execution-contract dependency |
| 2 | Closed the remaining execution-path preflight gaps and locked them with fixture coverage | `exec` now fails fast on malformed approved specs, preserves missing-plan skip semantics, blocks downstream tasks when dependencies block, and has binary tests for the repaired scheduler behavior | AC-8, AC-10, AC-11, AC-16 |

#### Active Tasks
| Task | Target AC | Status | Tag | Owner | Notes |
|------|-----------|--------|-----|-------|-------|
| task3: Frontmatter + schema validation | AC-16,AC-17 | pending | [queued] | claude | Keep queued unless the Round 2 preflight repair uncovers a minimal parser hardening change that must be done in-path |
| task5: Task ID + spec parser | AC-4 | in_progress | [queued] | claude | The minimal spec-parser tightening needed for execution preflight is now in place; broader ID/spec metadata work remains outside this slice |
| task6: state.json + state.md + status | AC-5 | pending | [queued] | claude | Runtime bootstrap remains important but is not part of the execution-path repair objective |
| task7: hive init command | AC-1 | pending | [queued] | claude | Adapter generation and detected-tool humanize installation stay queued this round |
| task10: Layer 1 commands | AC-7,AC-9 | in_progress | [queued] | claude | Launch/isolate contract drift is no longer the active blocker; remaining context/skill and retry surface work stays queued |
| task11: hive check (3 verifiers) | AC-10 | in_progress | [mainline] | claude | `check` is now covered in the repaired `exec` path by binary fixture tests; remaining work is broader plan completion rather than execution-path correctness |
| task12: hive report | AC-11 | in_progress | [queued] | claude | `report` stays sufficient for the execution path; broader audit/event coverage remains for the audit slice |
| task13: hive exec orchestration | AC-8 | in_progress | [mainline] | claude | Strict approved-task preflight, missing-plan skip preservation, blocked-dependency fan-out, and retry/block convergence are now implemented and fixture-covered |
| task14: hive merge + PR | AC-12 | pending | [queued] | claude | Dependency ordering and conflict/platform behavior remain queued this round |
| task15: hive rfc + approve | AC-13 | pending | [queued] | claude | RFC/approve end-to-end repair remains queued this round |
| task16: Audit system | AC-14 | pending | [queued] | claude | Audit wiring is not part of the current mainline objective |
| task17: Skill discovery + loading | AC-15 | pending | [queued] | claude | Launch-time skill injection remains queued unless a minimal fix becomes unavoidable |
| task18: Numeric constraints | AC-17 | pending | [queued] | claude | Numeric-constraint cleanup is not needed for the round-1 execution repair |
| task19: Diagnostic commands | AC-18 | pending | [queued] | claude | Doctor completeness remains queued |
| task20: Agent tool adapters | AC-19 | pending | [queued] | claude | Full adapter outputs and hooks remain queued |
| task21: Architecture review | AC-5,AC-7 | pending | [queued] | codex | Review findings exist in RLCR feedback, but producing a checked-in artifact is out of this round's mainline |
| task22: Integration test strategy | All | pending | [queued] | codex | Full fixture strategy remains queued; this round only requires targeted execution-path coverage |

### Blocking Side Issues
| Issue | Discovered Round | Blocking AC | Resolution Path |
|-------|-----------------|-------------|-----------------|
| None currently open for the execution-path slice | 2 | AC-8, AC-10, AC-11, AC-16 | Round 2 closed the preflight/skip/convergence gaps; remaining work sits in other AC slices |

### Queued Side Issues
| Issue | Discovered Round | Why Not Blocking | Revisit Trigger |
|-------|-----------------|------------------|-----------------|
| RFC/approve flow depends on pre-existing `state.json`, but there is no bootstrap/import path from spec+plan | 0 | Important but not needed to repair the current execution-path contract in isolation | Revisit when the round returns to AC-5/AC-13 work |
| Adapter generation for detected tools is incomplete (missing hooks, guard, and humanize installation) | 0 | Large surface area that would take over the round without helping the immediate execution-path fix | Revisit when AC-1/AC-19 return to the mainline |
| Merge/RFC collaboration path is incomplete (`merge --all` order, conflict blocking, platform-none RFC commit path) | 0 | Independent collaboration work that does not block the current `exec` repair | Revisit when AC-12/AC-13 become mainline again |
| `hive doctor` stale-lock warning path ignores the documented 5-minute age threshold and relies on Linux-only `/proc` liveness checks | 0 | Important contract bug, but secondary to the larger doctor/audit coverage gaps | Revisit when task19 implements the full doctor contract |
| The analyze deliverables still are not checked in as durable repo artifacts (no committed architecture review / integration strategy document) | 1-review | The missing artifacts do not block the current binary from running, but they do block plan completion and verification confidence | Revisit when task21/task22 are brought back into execution instead of being summarized around |

### Completed and Verified
| AC | Task | Completed Round | Verified Round | Evidence |
|----|------|-----------------|----------------|----------|
| AC-1 | task1: Cargo workspace + CLI skeleton | 0 | 0-review | `cargo build` and `hive --help` succeed |
| AC-2 | task2: Config parser + deep merge | 0 | 0-review | 7 unit tests pass |
| AC-3 | task4: 7-state machine | 0 | 0-review | 16 unit tests pass |
| AC-6 | task8: Git worktree operations | 0 | 0-review | Worktree create/remove/list helpers exist and build cleanly |
| AC-7 | task9: Locking primitives | 0 | 0-review | Lock/orchestrator unit tests pass, but orchestration still needs to honor them end-to-end |

### Explicitly Deferred
| Task | Original AC | Deferred Since | Justification | When to Reconsider |
|------|-------------|----------------|---------------|-------------------|
