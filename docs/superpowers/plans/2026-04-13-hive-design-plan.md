# Hive (蜂巢) Multi-Agent Orchestration Harness — v1 Implementation Plan

## Goal Description

Implement the Hive multi-agent orchestration harness as a single Rust CLI binary (`hive`) that enables team-based multi-agent task execution through a three-layer architecture (Orchestrator → Sub-agent → Worker). The v1 delivers an end-to-end execution chain: init → plan (spec + plan.md) → rfc (team review) → approve → exec (isolated worktree execution with acceptance verification) → merge (PR-based integration). Claude Code and Codex CLI are first-class agent backends with a generic CLI fallback for extensibility.

**v1 Scope**: init → plan (spec + plan.md, external tools) → rfc (aggregates spec + plan) → approve → exec (claim → isolate → launch → check → report) → merge (PR mode) → status → audit → doctor

**Deferred to v2**: 7-phase interactive planning flow, full skill install/uninstall/marketplace, pause/resume/checkpoint, auto-conflict-resolution, doctor auto-repair, aggregated per-draft audit reports, merge queue integration

## Acceptance Criteria

Following TDD philosophy, each criterion includes positive and negative tests for deterministic verification.

- AC-1: `hive init` creates `.hive/` directory structure (specs/, plans/, rfcs/, reports/, tasks/, skills/, worktrees/) with config.yml template, config.local.yml (auto-populated from git config), appends correct entries to project .gitignore; detects agent CLIs and generates adapter structures; installs default plugins (humanize) for each detected agent tool
  - Positive Tests (expected to PASS):
    - Running `hive init` in a git repository creates all required directories and config files
    - Running `hive init` in an already-initialized repo does not overwrite existing config files, only creates missing directories/files
    - `config.local.yml` user.name and user.email are auto-populated from `git config`
    - .gitignore entries for config.local.yml, state.md, specs/, plans/, tasks/, worktrees/ are appended without duplicates (specs and plans are local working files; their content is aggregated into committed RFC documents)
    - When `claude` CLI is detected, `.claude-plugin/plugin.json` and skill files are generated
    - When `codex` CLI is detected, `.codex/instructions.md` and `.codex/hooks.json` are generated
    - humanize plugin is installed for each detected agent tool (Claude Code: registered as plugin; Codex: merged into instructions; generic: installed to .hive/skills/)
    - humanize plugin installation is idempotent: already installed → skipped silently
  - Negative Tests (expected to FAIL):
    - Running `hive init` in a non-git directory exits with error code and clear message
    - Running `hive init` without write permission to current directory exits with error
    - When neither `claude` nor `codex` CLI is detected, adapter falls back to generic skill files with warning

- AC-2: Dual-layer YAML configuration with deep merge; `hive config --show` displays merged values with source annotations (global/local override)
  - Positive Tests:
    - config.yml + config.local.yml deep merge produces correct effective config for all fields
    - `hive config --show` outputs each field with source annotation (global or local override)
    - config.local.yml fields override config.yml on a per-field basis (deep merge, not shallow replace)
  - Negative Tests:
    - Malformed YAML in config.yml is rejected with parse error and file location
    - Malformed YAML in config.local.yml is rejected with parse error and file location
    - Accessing config before `hive init` exits with "not initialized" error

- AC-3: Task state machine implements 7 states (pending, assigned, in_progress, review, completed, failed, blocked) with hard-coded transition rules; invalid transitions are rejected with specific error messages
  - Positive Tests:
    - pending → assigned succeeds when all depends_on tasks are completed
    - assigned → in_progress succeeds when worktree is created and agent is launched
    - in_progress → review succeeds when worker writes result.md with status: completed
    - in_progress → failed succeeds when worker writes result.md with status: failed, or on timeout
    - review → completed succeeds when `hive check` verifies all acceptance criteria pass
    - review → failed succeeds when `hive check` finds acceptance criteria failures
    - failed → pending (retry action) succeeds when retry count < retry_limit (3)
    - failed → blocked succeeds when retry count >= retry_limit (3)
    - blocked → pending succeeds (manual intervention resolution)
  - Negative Tests:
    - pending → review transition is rejected ("invalid state transition: pending → review")
    - pending → completed transition is rejected
    - assigned → completed transition is rejected (must go through in_progress → review)
    - pending → assigned is rejected when depends_on tasks are not all completed
    - failed → pending (retry) is rejected when retry count >= retry_limit (3)

- AC-4: Task ID and Draft ID both use `<user_name>-<ulid>` format; identity remains stable across content modifications; spec content hash (sha256[:8]) stored as metadata in state.json
  - AC-4.1: Spec file format (`.hive/specs/<id>.md`) uses YAML frontmatter with required fields (id, draft_id, depends_on, complexity, approval_status) and Markdown body (goal, acceptance criteria, context files)
    - Positive: spec.md with all required fields and valid complexity value (S/M/L) is parsed successfully
    - Negative: spec.md missing required field `id` is rejected with "missing required field: id"
  - AC-4.2: Plan file location follows `plans/<draft_id>/<task_id>.md` convention; draft_id groups all tasks belonging to the same draft
    - Positive: plan file at correct path is found by `hive exec` and `hive launch`
    - Negative: plan file at wrong path (e.g., flat in plans/) is not recognized
  - Positive Tests:
    - Task ID and Draft ID generation both produce unique `<user_name>-<ulid>` identifiers
    - user_name is extracted from `git config user.email` (part before @), overridable via config.local.yml
    - Task ID and Draft ID do not change when content is edited
    - Git branch `hive/<task_id>` is correctly derived from the task ID
  - Negative Tests:
    - Creating a task without git user.email configured and no config.local.yml user.name exits with error
    - Spec with invalid complexity value (not S/M/L) is rejected

- AC-5: Per-task runtime state stored in `.hive/tasks/<id>/state.json` with `schema_version` field; `state.md` is a derived Markdown view regenerated by CLI commands, never used as authoritative input
  - Positive Tests:
    - state.json contains current state, retry count, timestamps, spec content hash, approval_status, and schema_version
    - `hive status` regenerates state.md from all task state.json files and displays task table
    - On inconsistency between state.md and state.json files, state.json is treated as authoritative
  - Negative Tests:
    - Direct modification of state.md has no effect on actual task state (state.json remains authoritative)
    - state.json with missing schema_version is loaded with deprecation warning, defaulting to version 1

- AC-6: Git worktree lifecycle: `hive isolate` creates worktree at `.hive/worktrees/<task_id>/` with branch `hive/<task_id>`, records base_commit; `hive cleanup` removes worktree after merge
  - Positive Tests:
    - `hive isolate --task <id>` creates git worktree at correct path with correct branch name
    - base_commit (main HEAD at creation time) is recorded in state.json
    - `hive cleanup --task <id>` removes worktree directory and deletes local branch
    - Multiple concurrent worktrees for different tasks coexist without interference
  - Negative Tests:
    - `hive isolate` for a task not in assigned state is rejected
    - `hive isolate` when worktree already exists for this task is rejected with "worktree already exists"
    - `hive cleanup` for a task not in completed state is rejected

- AC-7: Concurrency control via per-task file locking (`.hive/tasks/<id>/lock`) using flock(2); orchestrator-level lock (`.hive/orchestrator.lock`) prevents double-exec; all state transitions are atomic (read-lock-validate-write-unlock with tmp+rename)
  - Positive Tests:
    - `hive claim` acquires per-task lock before state transition; concurrent claim of same task by second process fails deterministically
    - State file writes are atomic (write to tmp file + rename)
    - Stale lock detection: lock file with non-existent PID and age > 5 minutes is forcibly acquired with warning
    - `.hive/orchestrator.lock` prevents two simultaneous `hive exec` instances
  - Negative Tests:
    - Two concurrent `hive claim` for same task: exactly one succeeds, other receives deterministic error
    - Second `hive exec` instance exits with "orchestrator already running" error
    - Attempting to acquire lock on non-existent task directory fails with clear error

- AC-8: `hive exec` orchestrates the full execution chain: scans approved tasks with plans, resolves dependency graph, schedules (claim → isolate → launch → check → report) respecting dependency order, handles failures with retry logic (up to retry_limit 3)
  - Positive Tests:
    - `hive exec` processes tasks in dependency order; independent tasks can be scheduled in any order
    - `hive exec` validates `.hive/plans/<draft_id>/<task_id>.md` exists for each approved task before scheduling
    - Failed tasks are retried up to retry_limit (3) before being marked blocked
    - Completion of a dependency unblocks downstream tasks for scheduling
    - `hive exec` exits successfully when all tasks are completed or blocked
    - Progress output during execution shows current task status
  - Negative Tests:
    - `hive exec` with no approved tasks exits with "no approved tasks to execute"
    - `hive exec` skips approved tasks missing plan.md with warning "plan not found for task <id>, skipping"
    - Circular dependency in task graph is detected and reported as error before execution begins
    - `hive exec` while another instance is running exits with orchestrator lock error

- AC-9: `hive launch` starts the configured agent tool in the task's worktree with task context; v1 supports Claude Code and Codex CLI as first-class backends with generic CLI fallback
  - Positive Tests:
    - With `launch.tool: claude`, launches Claude Code with plugin reference and agent prompt pointing to task plan
    - With `launch.tool: codex`, launches Codex CLI with `--approval-mode full-auto` and task plan as prompt, instructions merged from loaded skills
    - With `launch.tool: custom`, launches custom_command with `{task_id}` and `{worktree_path}` substituted
    - Agent receives task spec, plan, and loaded skill content as context
    - Agent process exit is detected and result.md presence is checked
  - Negative Tests:
    - `hive launch` with unconfigured or unavailable agent tool exits with "agent tool not found" error
    - `hive launch` for a task not in assigned state is rejected
    - `hive launch` when agent binary is not in PATH exits with clear diagnostic (applies to both `claude` and `codex` tools)

- AC-10: `hive check --task <id>` validates acceptance criteria from spec.md using 3 verifier types; returns deterministic exit codes (0=all pass, 1=at least one fail, 2=spec not found, 3=task not in review state)
  - AC-10.1: Command-type verifier: executes shell command in worktree, checks exit code
    - Positive: command returning exit 0 marks acceptance criterion as passed
    - Negative: command returning non-zero marks criterion as failed with stderr captured
  - AC-10.2: File-type verifier: checks file existence and optional content pattern match in worktree
    - Positive: file exists and matches pattern → pass
    - Negative: file missing or pattern mismatch → fail with details
  - AC-10.3: Manual-type verifier: prompts user for interactive confirmation
    - Positive: user confirms → pass
    - Negative: user rejects → fail with rejection reason recorded
  - Positive Tests:
    - All 3 verifier types work correctly and results are recorded
  - Negative Tests:
    - `hive check` for non-existent task exits with code 2
    - `hive check` for task not in review state exits with code 3

- AC-11: `hive report --task <id>` reads result.md from worktree branch, validates frontmatter schema, updates state.json, regenerates state.md view; exit codes: 0=success, 1=result.md missing/malformed, 2=task not in expected state
  - Positive Tests:
    - Valid result.md with all required frontmatter fields is parsed and state.json updated
    - state.md is regenerated to reflect new task state
    - Audit entry is appended for the state transition
  - Negative Tests:
    - Missing result.md exits with code 1
    - result.md with malformed frontmatter exits with code 1 with parse error details
    - `hive report` for task not in in_progress state exits with code 2

- AC-12: `hive merge --task <id>` rebases task branch onto main then creates PR/MR via configured platform tool; `hive merge --all` processes completed tasks in dependency order; `--mode direct` merges directly without PR/MR
  - AC-12.1: Rebase + PR/MR creation as single operation
    - Positive: task branch is rebased onto current main, PR/MR is created via configured platform (github/gitlab/none)
    - Negative: merge conflict during rebase marks task as blocked (v1 does not auto-resolve conflicts)
  - AC-12.2: Dependency-ordered merge
    - Positive: `hive merge --all` processes tasks in dependency order; later tasks rebase onto updated main
    - Negative: attempting to merge a task whose dependencies are not yet merged is rejected
  - Positive Tests:
    - `--mode direct` merges directly to main without creating PR/MR
    - With `platform: none`, only rebases and outputs branch name for manual review
  - Negative Tests:
    - `hive merge` with configured platform tool unavailable exits with diagnostic error
    - `hive merge` for task not in completed state is rejected

- AC-13: RFC and approval workflow: spec + plan.md are created together in planning phase (both committed), `hive rfc` aggregates them for team review, `hive approve` sets approved status after consensus, then `hive exec` can proceed
  - AC-13.1: RFC document generation
    - `hive rfc --draft <draft_id>` collects all specs (`.hive/specs/*.md`) and corresponding plans (`.hive/plans/<draft_id>/<task_id>.md`) for the draft, generates `.hive/rfcs/<draft_id>.md` aggregating both, commits to RFC branch, optionally creates PR/MR
    - Positive: `.hive/rfcs/<draft_id>.md` contains draft overview, dependency graph (text), complexity summary, full content of all related specs AND plans embedded inline
    - Positive: with `rfc.platform: github`, creates PR via `gh` CLI with RFC title and `rfc` label, body referencing the RFC file
    - Positive: with `rfc.platform: none`, only commits RFC file to branch without creating PR/MR
    - Positive: approval_status transitions from draft to rfc in state.json for all specs under this draft
    - Negative: `hive rfc --draft <id>` with no specs associated to the draft exits with "no specs found for draft"
    - Negative: `hive rfc --draft <id>` when a spec has no corresponding plan.md exits with "plan not found for task <id>"
    - Negative: with `rfc.platform: github` but `gh` CLI unavailable, exits with diagnostic error suggesting `platform: none` fallback
    - Negative: `hive rfc` for a draft already in rfc or approved state is rejected
  - AC-13.2: Approval workflow (`hive approve --draft <draft_id>`)
    - `hive approve` is an explicit human action performed after team review consensus (PR merged, offline discussion, etc.)
    - Positive: `hive approve --draft <draft_id>` transitions all specs' approval_status from rfc (or draft) to approved
    - Positive: with `rfc.platform: github`, checks if RFC PR is merged; if not merged, prints advisory warning but does not block approval
    - Positive: with `rfc.platform: none`, approves without any platform check
    - Positive: `hive approve --draft <draft_id>` on already-approved draft is idempotent (no error, no state change)
    - Negative: `hive approve` for a non-existent draft exits with "draft not found"
    - Negative: `hive exec` skips tasks that do not have approval_status: approved
  - Positive Tests:
    - RFC file is a self-contained Markdown document readable without any platform tooling
    - End-to-end flow: plan (spec + plan.md) → rfc → team review → approve → exec works correctly
  - Negative Tests:
    - `hive exec` with approved tasks missing plan.md exits with "plan not found for task <id>"

- AC-14: Audit system implements 3-level logging (minimal, standard, full) with append-only per-task `.hive/tasks/<id>/audit.md`; CLI-exclusive write access
  - Positive Tests:
    - minimal level: records state changes, final result, merge events
    - standard level: minimal + RLCR round summaries, convergence process, retry reasons
    - full level: standard + agent decision rationale summaries, diff traceability
    - Each audit entry includes timestamp and event type
    - Audit level is configurable in config.yml (`audit_level` field)
  - Negative Tests:
    - Worker agent writing directly to audit.md is detected by `hive doctor` as anomaly
    - Audit entries are append-only; modification of existing entries is flagged by doctor

- AC-15: Skill loading discovers and loads skills from `.hive/skills/`, `~/.config/hive/skills/`, and system plugins; priority: repo > user > system; loaded skills are injected into agent launch context
  - Positive Tests:
    - Skills listed in config.yml `skills.default` are auto-loaded for all tasks
    - Skills listed in spec `skills` field are loaded for that task
    - Skills listed in spec `exclude_skills` field are excluded even if in default list
    - Repo-level skill with same name as system skill overrides it
    - SKILL.md frontmatter is validated (name and description required)
  - Negative Tests:
    - Skill with missing SKILL.md is skipped with warning
    - Skill name with invalid characters (not alphanumeric or hyphen) is rejected
    - Skill with description > 500 characters is rejected
    - Skill with frontmatter > 1024 characters is rejected

- AC-16: All frontmatter files include `schema_version` field; unknown major version (>1) is rejected; unknown fields are warned and ignored; missing schema_version defaults to version 1 with deprecation warning
  - Positive Tests:
    - File with `schema_version: 1` and all required fields is accepted
    - File with `schema_version: 1` and extra unknown fields is accepted with warning logged
    - File without schema_version field is accepted with deprecation warning, treated as version 1
  - Negative Tests:
    - File with `schema_version: 2` (unsupported) is rejected with "unsupported schema version: 2"
    - File with missing required fields is rejected with specific field-level error message
    - File with invalid field types (e.g., depends_on as string instead of list) is rejected

- AC-17: Numeric constraints from design spec are enforced as hard requirements
  - AC-17.1: retry_limit defaults to 3; failed tasks exceeding limit automatically transition to blocked
    - Positive: task fails 3 times → automatically transitions to blocked on exceeding limit
    - Negative: retry attempt after reaching limit is rejected with "retry limit exceeded"
  - AC-17.2: Complexity S/M/L maps to RLCR max rounds 2/5/8 respectively
    - Positive: S-complexity task has rlcr_max_rounds=2 enforced in spec metadata
    - Negative: manual rlcr_max_rounds value exceeding complexity mapping is rejected
  - AC-17.3: Skill frontmatter total must be ≤1024 characters; description field must be ≤500 characters
    - Positive: skill with frontmatter at exactly 1024 chars is accepted
    - Negative: skill with frontmatter at 1025 chars is rejected with size error

- AC-18: `hive doctor` validates environment (git, agent tools, gh CLI), config validity, state consistency (state.json across tasks), stale locks, and worktree health; exit codes: 0=healthy, 1=warnings, 2=errors requiring action; v1 reports only, does not auto-repair
  - Positive Tests:
    - `hive doctor` in healthy state exits with code 0 and all-clear summary
    - Detects stale lock files (PID dead + age > 5 min) and reports as warnings
    - Detects orphaned worktrees (no corresponding task) and reports
    - Validates config.yml syntax and all required fields
    - Checks git CLI, configured agent tool, and `gh` CLI availability
  - Negative Tests:
    - `hive doctor` in non-initialized repo exits with error suggesting `hive init`
    - Detects state.md/state.json inconsistency and reports which file is authoritative (state.json)
    - Detects missing spec.md for an existing task and flags error

- AC-19: Agent tool adapter generation: `hive init` detects available agent CLIs and generates corresponding adapter structures
  - AC-19.1: Claude Code plugin adapter: generates `.claude-plugin/plugin.json`, skill files for user-facing commands (hive:init, hive:exec, hive:status, hive:merge, hive:audit, hive:skill, hive:doctor, hive:graph, hive:rfc), and orchestrator guard hook that blocks Write/Edit/NotebookEdit when HIVE_ROLE=orchestrator
    - Positive: Generated plugin.json contains correct metadata (name: "hive", description, version)
    - Positive: Each user-facing command has a corresponding skill markdown file with valid frontmatter
    - Positive: Orchestrator guard hook blocks Write/Edit/NotebookEdit tools when HIVE_ROLE=orchestrator
    - Negative: When `claude` CLI is not detected, Claude Code plugin adapter is skipped with warning
  - AC-19.2: Codex CLI adapter: generates `.codex/instructions.md` (system instructions referencing hive CLI commands) and `.codex/hooks.json` (if Codex hook support is available)
    - Positive: Generated instructions.md contains correct hive CLI usage guidance for Codex
    - Positive: instructions.md references all user-facing hive commands
    - Negative: When `codex` CLI is not detected, Codex adapter is skipped with warning
  - AC-19.3: Default plugin installation (humanize): `hive init` installs humanize as a default plugin for each detected agent tool
    - Positive: For Claude Code, humanize is registered as a plugin reference (e.g., in plugin.json dependencies or launch config)
    - Positive: For Codex, humanize skill content (gen-plan, start-rlcr-loop) is merged into `.codex/instructions.md`
    - Positive: For generic fallback, humanize skill files are installed to `.hive/skills/humanize/`
    - Positive: Installation is idempotent — if humanize is already installed for an agent tool, it is skipped silently
    - Negative: If humanize plugin source is not available (e.g., not found at known paths), prints warning and continues without installing
  - Positive Tests:
    - Re-running `hive init` does not overwrite existing customized adapter files (both Claude and Codex)
    - When both `claude` and `codex` CLIs are detected, both adapters are generated and humanize is installed for both
  - Negative Tests:
    - When neither `claude` nor `codex` CLI is detected, generic skill files are generated as fallback with warning
    - Generated skill/instruction files with invalid format are caught by schema validation

## Path Boundaries

Path boundaries define the acceptable range of implementation quality and choices.

### Upper Bound (Maximum Acceptable Scope)

The implementation includes all 19 acceptance criteria with comprehensive test coverage, including integration tests using fixture git repositories for crash recovery, concurrent claim, and invalid state transitions. The CLI provides helpful error messages with actionable suggestions for common mistakes. The Claude Code plugin adapter generates a complete, functional plugin structure. All Layer 0 and Layer 1 commands from the design spec (except pause/resume and 7-phase planning) are implemented with full audit trail support at all 3 levels. Diagnostic commands (`doctor`, `graph`, `show`, `list-tasks`) provide detailed output.

### Lower Bound (Minimum Acceptable Scope)

The implementation satisfies all 19 acceptance criteria at their specified positive/negative test levels. Integration tests cover critical paths (state transitions, worktree lifecycle, merge, concurrency). Error messages include exit codes and basic diagnostic information. Claude Code plugin adapter generates valid but minimal plugin structure. Audit system supports at least minimal and standard levels.

### Allowed Choices

- Can use: Rust stable toolchain, clap for CLI, serde + serde_yaml for YAML parsing, git2 crate or git CLI subprocess for git operations, ulid crate for ID generation, flock(2) for file locking, codex CLI subprocess for Codex adapter
- Can use: tokio for async if needed for concurrent task monitoring, but synchronous is acceptable for v1
- Cannot use: SQLite or other database engines (per-task JSON files is the chosen storage approach)
- Cannot use: any web framework or HTTP server (CLI-only architecture)
- Cannot use: nightly Rust features
- Fixed: task ID format is `<user_name>-<ulid>` (original design specified content hash, changed per convergence for identity stability)
- Fixed: state.md is derived-only, never authoritative (changed from design's mixed usage per convergence)
- Fixed: execution state lives only in state.json, not in spec frontmatter (per convergence)

## Feasibility Hints and Suggestions

> **Note**: This section is for reference and understanding only. These are conceptual suggestions, not prescriptive requirements.

### Conceptual Approach

```
Cargo workspace structure:
hive/
├── Cargo.toml                    # workspace root
├── crates/
│   ├── hive-cli/                 # CLI entry point, clap command definitions
│   │   └── src/
│   │       ├── main.rs
│   │       └── commands/         # one module per command group
│   ├── hive-core/                # state machine, task model, config, schema
│   │   └── src/
│   │       ├── config.rs         # YAML config parsing + dual-layer merge
│   │       ├── state.rs          # 7-state machine with transitions
│   │       ├── task.rs           # task model, ULID ID generation
│   │       ├── frontmatter.rs    # YAML frontmatter parser
│   │       ├── schema.rs         # schema validation + versioning
│   │       └── lock.rs           # flock-based concurrency
│   ├── hive-git/                 # git operations
│   │   └── src/
│   │       ├── worktree.rs       # create/delete/list worktrees
│   │       ├── branch.rs         # branch management
│   │       └── merge.rs          # rebase + PR creation
│   ├── hive-audit/               # audit logging subsystem
│   └── hive-adapter/             # agent tool adapters
│       └── src/
│           ├── claude.rs         # Claude Code adapter + plugin generation
│           ├── codex.rs          # Codex CLI adapter + instructions generation
│           └── generic.rs        # Generic CLI adapter
└── tests/                        # integration tests with fixture repos
```

### Relevant References

- `.hive/` directory structure: design Section 5
- Config format and merge rules: design Section 6
- Skill system and SKILL.md format: design Section 7
- State machine diagram and transitions: design Section 8
- Task file formats (spec.md, result.md, state.md): design Section 10
- Complexity/RLCR mapping: design Section 11
- Conflict management and merge: design Section 12
- RFC flow: design Section 13
- Audit system and levels: design Section 14
- CLI command inventory: design Section 15
- Plugin packaging and adapters: design Section 18

## Dependencies and Sequence

### Milestones

1. **Foundation**: Rust workspace, CLI framework, config system, frontmatter parser
   - Phase A: Cargo workspace with clap skeleton and error handling infrastructure
   - Phase B: YAML config parsing with dual-layer deep merge logic
   - Phase C: Frontmatter parser with schema validation engine and versioning

2. **Task Model & State Machine**: Task identity, state machine, persistent storage
   - Phase A: Task ID generation (user_name + ULID) and spec file format
   - Phase B: 7-state machine with hard-coded transition validation
   - Phase C: Per-task state.json storage and state.md view generation

3. **Git Integration**: Worktree lifecycle and branch management
   - Phase A: Git worktree create/delete operations
   - Phase B: Branch management (hive/<task_id>) and base commit tracking

4. **Execution Engine**: Layer 1 commands, concurrency, and orchestration
   - Phase A: Concurrency control (per-task flock + orchestrator lock + atomic writes)
   - Phase B: claim, isolate, launch commands (with Claude Code and Codex adapters)
   - Phase C: check (3 verifier types) and report commands
   - Phase D: `hive exec` orchestration with dependency graph resolution and retry logic

5. **Integration & Collaboration**: Merge, RFC, audit
   - Phase A: `hive merge` with rebase + platform-agnostic PR/MR creation
   - Phase B: `hive rfc --draft` with per-draft RFC document generation, approval_status tracking, platform-agnostic PR/MR
   - Phase C: Audit system (3 levels, append-only, CLI-exclusive write)

6. **Extensions & Diagnostics**: Skills, doctor, plugin adapter, constraints
   - Phase A: Skill discovery and loading from 3-tier sources with priority resolution
   - Phase B: `hive doctor`, `hive graph`, `hive status`, `hive show`, `hive list-tasks`
   - Phase C: Agent tool adapter generation (Claude Code plugin + Codex instructions/hooks + generic fallback)
   - Phase D: Numeric constraints enforcement across all subsystems

Milestone 1 has no dependencies. Milestones 2 and 3 depend on Milestone 1 and can proceed in parallel. Milestone 4 depends on Milestones 2 and 3. Milestone 5 depends on Milestone 4. Milestone 6 depends on Milestones 4 and 5.

## Task Breakdown

Each task must include exactly one routing tag:
- `coding`: implemented by Claude
- `analyze`: executed via Codex (`/humanize:ask-codex`)

| Task ID | Description | Target AC | Tag | Depends On |
|---------|-------------|-----------|-----|------------|
| task1 | Set up Cargo workspace with clap CLI skeleton, error types, and binary entry point | AC-1 | coding | - |
| task2 | Implement YAML config parser with dual-layer deep merge and `hive config --show` | AC-2 | coding | task1 |
| task3 | Implement YAML frontmatter parser with schema validation engine and versioning logic | AC-16 | coding | task1 |
| task4 | Implement 7-state task state machine with hard-coded transition rules and validation | AC-3 | coding | task1 |
| task5 | Implement task ID generation (user_name + ULID) and spec file format parser | AC-4 | coding | task4 |
| task6 | Implement per-task state.json storage, state.md view generation, and `hive status` | AC-5 | coding | task4, task3 |
| task7 | Implement `hive init` command: directory creation, config generation, .gitignore update, default plugin (humanize) installation | AC-1 | coding | task2, task3 |
| task8 | Implement git worktree operations (create/delete/list) and branch management | AC-6 | coding | task1 |
| task9 | Implement concurrency control: per-task flock, orchestrator lock, stale detection, atomic writes | AC-7 | coding | task6 |
| task10 | Implement Layer 1 commands: claim, isolate, launch (Claude Code + Codex + generic adapter), retry, cleanup | AC-7, AC-9 | coding | task4, task6, task8, task9 |
| task11 | Implement `hive check` with 3 verifier types (command/file/manual) and exit codes | AC-10 | coding | task3, task8 |
| task12 | Implement `hive report` with result.md validation, state update, state.md regeneration | AC-11 | coding | task6, task3 |
| task13 | Implement `hive exec` orchestration: dependency graph, sequential scheduling, retry logic | AC-8 | coding | task10, task11, task12 |
| task14 | Implement `hive merge` with rebase + PR creation via gh CLI, dependency-ordered --all | AC-12 | coding | task8, task6 |
| task15 | Implement `hive rfc --draft`: per-draft RFC document generation (aggregates specs + plans to .hive/rfcs/), platform-agnostic PR/MR, `hive approve --draft` | AC-13 | coding | task5, task6 |
| task16 | Implement audit system: 3-level logging, append-only per-task audit.md, CLI-exclusive write | AC-14 | coding | task6 |
| task17 | Implement skill discovery and loading from 3-tier sources, priority resolution, launch injection | AC-15 | coding | task2, task3 |
| task18 | Implement numeric constraints: retry_limit enforcement, complexity/RLCR mapping, frontmatter limits | AC-17 | coding | task3, task4 |
| task19 | Implement `hive doctor`, `hive graph`, `hive show`, `hive list-tasks` diagnostic commands | AC-18 | coding | task6, task8 |
| task20 | Implement agent tool adapters: Claude Code plugin (plugin.json, skills, guard hook) + Codex adapter (instructions.md, hooks.json) + generic fallback + humanize default plugin installation | AC-19 | coding | task7 |
| task21 | Architecture review: state management correctness, concurrency model, crash recovery paths | AC-5, AC-7 | analyze | task6, task9 |
| task22 | Integration test strategy: fixture repo design, crash recovery scenarios, concurrent claim tests | All | analyze | task13 |

## Claude-Codex Deliberation

### Agreements
- v1 should narrow to a minimum viable vertical execution chain rather than implementing the entire 18-section design spec in one pass
- Task ID should use stable identifiers (ULID) decoupled from spec content hash to maintain audit/RFC/retry continuity across spec edits
- Per-task state.json should be the single source of truth; state.md is a derived view only, never read as authoritative input
- pause/resume/checkpoint should be deferred to v2 (requires agent-specific graceful shutdown protocol that is not universally available)
- Claude Code and Codex CLI should be the first-class v1 backends with a clean adapter interface defined for future extensibility
- File-based locking (flock) with PID-based stale detection is adequate for single-machine v1 concurrency
- Schema versioning with reject-unknown-major, warn-unknown-fields, and default-to-v1 is the right migration strategy
- Auto-conflict-resolution should be deferred to v2; v1 marks merge conflicts as blocked for human resolution
- `hive check`, `hive report`, `hive doctor` must have deterministic contracts with documented exit codes and failure categories
- 7-phase interactive planning flow should be deferred to v2; v1 can use existing tools like humanize gen-plan for plan generation

### Resolved Disagreements
- **Spec frontmatter status field**: First Codex identified that keeping `status` in spec frontmatter creates a second status-bearing location conflicting with state.json as single source of truth. Resolution: execution state (pending/assigned/in_progress/etc.) exists ONLY in state.json; spec frontmatter contains `approval_status` (draft/rfc/approved) which is RFC lifecycle metadata, not execution state. This separation is clean and avoids dual-source ambiguity.
- **plan_status in v1**: Second Codex noted plan_status is leftover surface area from the deferred 7-phase planning flow. Resolution: removed plan_status entirely; replaced with `approval_status` in state.json for RFC workflow gating. Only tasks with `approval_status: approved` can be scheduled by `hive exec`.
- **hive merge semantics**: Second Codex flagged ambiguity between "PR-creation-only" and "rebase + PR creation." Resolution: `hive merge` performs rebase onto main THEN creates PR as a single atomic operation. `--mode direct` skips PR and merges directly. This is one command with two modes, not two separate features.
- **v1 scope breadth**: First Codex in Round 1 argued the original AC coverage was still too broad despite claiming scope narrowing. Resolution: materially narrowed by deferring 7-phase planning flow, full skill install/uninstall/marketplace, pause/resume/checkpoint, auto-conflict-resolution, doctor auto-repair, aggregated per-draft audit reports, and merge queue integration.
- **Path consistency**: Second Codex identified inconsistent spec path references (specs/<id>.md vs .hive/specs/<id>.md). Resolution: canonical path is `.hive/specs/<id>.md`, used consistently throughout the plan.

### Convergence Status
- Final Status: `converged`
- Convergence Rounds: 2
- Round 1: 7 REQUIRED_CHANGES identified (scope narrowing, single source of truth, paused state removal, deterministic contracts, concurrency protocol, enforcement clarification, schema evolution). All accepted by Claude and incorporated.
- Round 2: 4 minor REQUIRED_CHANGES identified (spec status field, plan_status removal, path consistency, merge semantics). All accepted. No remaining high-impact disagreements.

## Pending User Decisions

All pending user decisions have been resolved:

- DEC-1: v1 target audience
  - Claude Position: Single-person local development for faster delivery
  - Codex Position: Depends on product goals — needs user decision
  - Tradeoff Summary: Team collaboration adds RFC flow, PR mode, approval gating; increases scope but matches design vision
  - Decision Status: **Team GitHub collaboration** — RFC flow and PR mode merge included in v1

- DEC-2: Primary agent backends for v1
  - Claude Position: Claude Code first with clean adapter interface
  - Codex Position: Agrees Claude-first is pragmatic; avoid premature multi-backend claims
  - Tradeoff Summary: Supporting both Claude Code and Codex CLI as first-class backends covers the primary use cases (Claude for implementation, Codex for review); generic CLI fallback ensures extensibility for other tools
  - Decision Status: **Claude Code + Codex CLI as first-class backends** with generic CLI fallback

- DEC-3: Runtime state storage format
  - Claude Position: Per-task JSON files with state.md as derived view
  - Codex Position: SQLite offers stronger consistency but adds dependency; JSON is adequate with proper locking
  - Tradeoff Summary: JSON is simpler, git-friendly, dependency-free; SQLite is more robust for queries and concurrency
  - Decision Status: **Per-task JSON files**

- DEC-4: Quantitative metrics nature
  - Claude Position: Configurable defaults with enforcement
  - Codex Position: N/A — open question
  - Tradeoff Summary: Hard requirements ensure deterministic behavior; configurable defaults allow flexibility
  - Decision Status: **All hard requirements** — retry_limit:3, S/M/L→RLCR 2/5/8, frontmatter ≤1024 chars, description ≤500 chars are all enforced constraints

## Implementation Notes

### Code Style Requirements
- Implementation code and comments must NOT contain plan-specific terminology such as "AC-", "Milestone", "Step", "Phase", or similar workflow markers
- These terms are for plan documentation only, not for the resulting codebase
- Use descriptive, domain-appropriate naming in code instead

### Design Deviations from Original Draft
The following changes were made during Claude-Codex convergence analysis. The original draft design is preserved in full at the bottom of this document for reference.

- **Task ID format**: Changed from `<user_name>-<content_hash>` (sha256[:8]) to `<user_name>-<ulid>` for identity stability. Spec content hash retained as metadata in state.json. Rationale: content-hash coupling causes task identity to break on spec edits, disrupting audit trails, RFC PRs, and retry tracking.
- **State management**: Changed from state.md as mixed read/write to state.md as derived-only view with per-task state.json as single source of truth. Rationale: eliminates dual-source ambiguity and simplifies crash recovery.
- **Execution state location**: Moved from spec frontmatter (`status`, `plan_status`) to state.json. Spec frontmatter retains only `approval_status` for RFC lifecycle. Rationale: single authoritative location for execution state prevents state drift.
- **Paused state**: Removed from v1 state machine (7 states instead of 9). Will be added in v2 with agent-specific checkpoint protocol. Rationale: pause/resume requires agents to support SIGTERM + checkpoint write, which is not a universal CLI contract.
- **Auto conflict resolution**: Deferred to v2. v1 marks merge conflicts as blocked for human resolution. Rationale: auto-resolution with only conflict files + two specs is insufficient context; risks semantic regressions.
- **7-phase planning flow**: Deferred to v2. v1 accepts manually created spec files or specs generated by external tools (e.g., humanize gen-plan). Rationale: the planning flow is a major subsystem with its own state machine; building it alongside the execution harness exceeds v1 scope.
- **Plan generation and RFC aggregation**: In v1, spec (`.hive/specs/<id>.md`) and plan (`.hive/plans/<draft_id>/<task_id>.md`) are created together during the planning phase as local working files. `hive rfc` aggregates all specs and plans into a single RFC document (`.hive/rfcs/<draft_id>.md`) which is the only committed artifact. specs/, plans/, and tasks/ are all gitignored — the RFC is the single source of truth for what was reviewed and approved.
- **Plan file location**: Moved from `.hive/tasks/<id>/plan.md` (original design) to `.hive/plans/<draft_id>/<task_id>.md`. All planning artifacts are grouped by draft: per-task implementation plans and per-draft decision process documents (requirements.md, convergence.md) live together under `plans/<draft_id>/`. The `tasks/` directory is now purely for runtime files (state.json, result.md, audit.md, lock).
- **Draft ID format**: Changed from `<user_name>-<content_hash>` to `<user_name>-<ulid>`, consistent with task ID format. Same rationale: identity stability across content modifications.

### Enforcement Model
- **v1 enforces**: worktree directory isolation (git worktree provides this), task state transition rules (Rust hard-coded), file schema validation with versioning, CLI-exclusive state/audit writes, per-task file locking for concurrency, numeric constraints (retry limit, frontmatter limits, RLCR mapping)
- **v1 documents as convention**: Layer 2 should not access other worktrees, Layer 2 should not push to protected branches, network/secret boundaries for worker agents
- This is explicitly labeled as "minimal enforcement + documented conventions" for v1

### Platform Requirements
- Locking mechanism depends on flock(2): Linux and macOS supported; Windows support deferred
- Platform CLI optional for PR/MR creation: `gh` (GitHub) or `glab` (GitLab). Configured via `rfc.platform` in config.yml. `platform: none` works without any platform CLI. Availability checked by `hive doctor`
- Agent CLIs optional: `claude` (Claude Code) and/or `codex` (Codex CLI). `hive init` auto-detects and generates corresponding adapter structures. `hive doctor` checks availability of configured agent tool
- Git CLI required. Minimum version to be determined during implementation
- Rust stable toolchain required for building from source

--- Original Design Draft Start ---

# Hive（蜂巢） — 多代理编排 Harness 设计方案

## 1. 项目概述

### 1.1 定位

Hive 是一个 **Agent 工具无关的多代理编排 Harness**，面向团队协作场景。它被设计为三层架构（分解、隔离、并行）和 Humanize 的 RLCR 质量循环。核心是独立的 Rust CLI，通过薄适配层嵌入各种 AI 编码工具（Claude Code、Codex CLI、OpenCode 等），实现"Hive 负责编排调度，Agent 工具负责实现质量"的协作模式。

### 1.2 核心理念

- **约束层硬编码**：所有角色限制、状态转换规则、隔离边界用 Rust 实现，不依赖自然语言提示词
- **蜂巢式协作**：每个代理在独立的 worktree（蜂室）中自由工作，编排器（蜂后）只做调度和验收
- **冲突不是错误**：并行开发必然产生冲突，Hive 的职责是按正确顺序合并并尽量自动解决
- **Agent 工具无关**：Hive 只通过 CLI 接口和文件系统通信，不绑定任何特定 AI 编码工具

### 1.3 技术选型

- **实现语言**：Rust，编译为单二进制分发
- **架构**：独立 Rust CLI 为核心，针对不同 agent 工具提供薄适配层（Claude Code skill、Codex hook 等）
- **质量保证**：可插拔质量循环（Humanize RLCR、Codex 内置审查、自定义等）
- **模型策略**：可插拔模型层，默认 Claude，可配置其他模型
- **不包含**：可视化仪表盘（不需要 viz）

---

## 2. 三层架构

```
Layer 0: 编排器 (Rust CLI — hive)
├─ 交互式需求收集（参考 superpowers brainstorming 流程）
├─ 收敛式计划生成（多模型方案收敛）
├─ 任务分解 + 依赖图
├─ 状态机、审计、模型路由
⛔ 绝对禁止编码、编辑文件、操作 worktree 内容

Layer 1: 子代理层 (Rust CLI 硬编码命令)
├─ hive claim    — 领取任务
├─ hive isolate  — 创建 worktree
├─ hive launch   — 启动 worker agent
├─ hive check    — 最终验收验证
├─ hive report   — 上报结构化结果
⛔ 硬编码约束层，非自然语言

Layer 2: 实现层 (Agent tool in worktree — Claude Code / Codex / OpenCode / 其他)
├─ 写代码、跑测试、git commit
├─ 通过可配置的质量循环保证实现质量（如 humanize RLCR、Codex 内置审查等）
├─ worktree 内完全自由（完全执行权限）
⛔ 不能跨 worktree，不能碰主分支
```

### 2.1 层级职责边界

| 层级 | 能做什么 | 不能做什么 |
|------|---------|-----------|
| Layer 0 编排器 | 规划、分解、调度、审计、RLCR 轮次控制、模型路由 | 写代码、改文件、操作任何 worktree 内容 |
| Layer 1 子代理 | 创建/销毁 worktree、启动/停止 agent、验收验证、上报结果 | 绕过状态机、跳跃状态转换 |
| Layer 2 实现者 | 在自己的 worktree 内完全自由（读写文件、执行命令、git commit、跑测试、安装依赖等），可以是任何 agent 工具 | 访问其他 agent 的 worktree、操作主分支 |

### 2.2 Hive 与 Agent 工具的职责分工

| 阶段 | Hive (Layer 0/1) | Agent 工具 (Layer 2) |
|------|------------------|---------------------|
| 需求 → 设计 → 分解 | ✓ | — |
| spec → plan | 提供输入 | 可配置的 plan 生成（如 humanize `gen-plan`） |
| plan → 实现 | 调度、启动 agent | 可配置的质量循环（如 humanize RLCR、Codex 内置审查） |
| 过程审查 | — | Agent 工具内部处理 |
| 最终验收 | `hive check` 验证验收标准 | — |
| 合并 | `hive merge` | — |

### 2.3 Agent 工具适配

Hive 通过 `launch` 配置支持不同的 agent 工具：

| Agent 工具 | launch.tool | 质量循环 | 适配方式 |
|-----------|-------------|---------|---------|
| Claude Code | `claude` | humanize RLCR | Claude Code skill |
| Codex CLI | `codex` | Codex 内置审查 | Codex hook |
| OpenCode | `opencode` | 自定义 | CLI 调用 |
| 自定义 | `custom` | 自定义 | 用户提供启动命令 |

---

## 3. 代理间通信

通过 **Git + 结构化 Markdown 文件** 做共享状态通信，不使用消息队列或直接对话。

所有通信载体使用 **Markdown + YAML frontmatter** 格式，对代理和人类都友好。Rust CLI 解析 frontmatter 中的结构化字段做状态机控制，正文部分供代理和人类阅读。

### 3.1 通信方向

| 通信方向 | 机制 |
|---------|------|
| Layer 0 → Layer 1 | Rust CLI 写入 `.hive/tasks/<task_id>/spec.md`，包含验收标准、上下文文件列表、依赖关系 |
| Layer 1 → Layer 2 | `hive launch` 通过 CLI 参数传入 `--task <task_id>`，agent 从 `specs/<id>.md` + `tasks/<id>/plan.md` 读取任务规格 |
| Layer 2 → Layer 1 | Worker 完成后写入 `.hive/tasks/<task_id>/result.md`，git commit 到 worktree 分支 |
| Layer 1 → Layer 0 | `hive report` 读取 result.md，更新全局 `state.md` + 任务 `audit.md` |
| 失败上报 | Worker 写 result.md 标记 `failed` + 原因，Layer 1 上报给 Layer 0 决策重试/跳过/人工介入 |

**无直接 agent 间通信** — 所有交互都经过文件系统 + Git，编排器是唯一的协调点。

---

## 4. Task ID 设计

### 4.1 命名规则

格式：`<user_name>-<content_hash>`

- `user_name`：从 `git config user.email` 自动提取 `@` 前的部分，可在 `config.local.yml` 中覆盖
- `content_hash`：spec 内容的 `sha256[:8]`，由 `hive plan` 创建任务时自动计算

示例：
```
chao-a1b2c3d4
chao-f5e6d7c8
zevo-9a8b7c6d
```

### 4.2 唯一性保证

- 内容哈希确保同一用户下不同任务不重复
- 用户名前缀确保跨协作者不冲突
- 相同内容产生相同哈希，CLI 检测到已存在则跳过或提示

### 4.3 分支命名

每个任务对应的 git 分支：`hive/<task_id>`

示例：`hive/chao-a1b2c3d4`

---

## 5. 目录结构

```
.hive/
├── config.yml                       # 全局配置（提交）
├── config.local.yml                 # 个人配置（gitignore）
├── state.md                         # 全局任务状态表（gitignore）
│
├── specs/                           # 提交 — 任务契约（RFC 阶段提交）
│   ├── chao-a1b2c3d4.md             # Task: auth middleware
│   └── chao-f5e6d7c8.md             # Task: route handlers
│
├── reports/                         # 提交 — 审计报告（完成后生成）
│   ├── chao-a1b2c3d4.md             # Draft: user auth system
│   └── zevo-9a8b7c6d.md             # Draft: logging pipeline
│
├── plans/                           # gitignore — 决策过程
│   ├── chao-a1b2c3d4/               # Draft: user auth system
│   │   ├── requirements.md
│   │   └── convergence.md
│   └── ...
│
├── tasks/                           # gitignore — 工作文件
│   ├── chao-a1b2c3d4/               # Task: auth middleware
│   │   ├── plan.md                  # 实施步骤
│   │   ├── result.md                # 执行结果
│   │   └── audit.md                 # 审计日志
│   └── ...
│
└── worktrees/                       # gitignore — 临时路径
```

三个顶层目录各司其职：

| 目录 | 提交 | 内容 | 生命周期 |
|------|------|------|---------|
| `specs/` | ✓ | 做什么（验收标准、依赖、复杂度） | RFC 阶段创建 |
| `reports/` | ✓ | 做了什么（per-draft 聚合审计报告） | 完成后生成 |
| `plans/` | ✗ | 为什么这样做（需求澄清、决策过程） | 规划阶段 |
| `tasks/` | ✗ | 怎么做 + 过程（plan、result、audit） | 执行阶段 |

### 5.1 文件提交规则

| 文件 | 提交 | 说明 |
|------|------|------|
| `config.yml` | ✓ | 团队共享配置 |
| `config.local.yml` | ✗ | 个人配置 |
| `specs/*.md` | ✓ | 任务契约（RFC 审查对象） |
| `reports/*.md` | ✓ | 审计报告（完成证明） |
| `state.md` | ✗ | 运行时状态 |
| `plans/` | ✗ | 决策过程，本地参考 |
| `tasks/` | ✗ | 工作文件，聚合到 report |
| `worktrees/` | ✗ | 临时路径 |

仓库里只留**契约（specs）和证明（reports）**。

### 5.2 .gitignore

```gitignore
config.local.yml
state.md
plans/
tasks/
worktrees/
```

---

## 6. 配置系统

### 6.1 双层配置

`hive init` 自动创建以下两个配置文件。

```yaml
# .hive/config.yml (global, committed to repo)

# Audit level: minimal | standard | full
audit_level: standard

merge:
  # Conflict resolution: auto | manual
  conflict_strategy: auto
  # Merge mode: direct | pr
  mode: pr
  # Rebase task branch onto main before merge
  rebase_before_merge: true

# Max retry attempts before marking task as blocked
retry_limit: 3

# Agent tool and quality loop configuration
launch:
  # Agent tool: claude | codex | opencode | custom
  tool: claude
  # Quality loop: humanize | codex-builtin | none
  quality_loop: humanize
  # Custom launch command (only used when tool: custom)
  # custom_command: "my-agent --task {task_id} --worktree {worktree_path}"

# Model binding for each role
# Format: <agent_tool>-<model>-<version>, e.g. claude-opus-4-6, codex-gpt-5-4
agents:
  # Layer 0: planning and convergence
  planner: claude-opus-4-6         # drives interactive planning (Phase 1-5)
  convergence: codex-gpt-5-4       # second model for plan convergence (Phase 4)
  # Layer 2: implementation and review
  worker: claude-sonnet-4-6        # executes tasks in worktrees
  reviewer: codex-gpt-5-4          # final acceptance review (hive check)
```

```yaml
# .hive/config.local.yml (personal, gitignored)
# Overrides config.yml on a per-field basis.
# Created by `hive init` with values from git config.

user:
  # Auto-populated from: git config user.email (part before @)
  name: zevorn
  # Auto-populated from: git config user.email
  email: chao.liu.zevorn@gmail.com

# Override any global config field, e.g.:
# agents:
#   worker: claude-opus-4-6        # use opus locally instead of sonnet
#   reviewer: codex-gpt-5-4
# launch:
#   tool: codex                    # use codex locally instead of claude
#   quality_loop: codex-builtin
```

### 6.2 合并规则

逐字段深度合并，`config.local.yml` 优先：

```
最终生效配置 = deep_merge(config.yml, config.local.yml)
```

`hive config --show` 查看合并后的实际生效配置，标注每个字段来源：

```
audit_level: standard              (global)
launch.tool: claude                (global)
launch.quality_loop: humanize      (global)
agents.planner: claude-opus-4-6    (global)
agents.convergence: codex-gpt-5-4  (global)
agents.worker: claude-opus-4-6     (local override)
agents.reviewer: codex-gpt-5-4     (global)
```

### 6.3 `hive init` 初始化行为

```
$ hive init
  │
  ├─ 1. 检查当前目录是否为 git 仓库，不是则报错退出
  │
  ├─ 2. 创建 .hive/ 目录结构
  │     mkdir -p .hive/{specs,reports,plans,tasks,skills,worktrees}
  │
  ├─ 3. 生成 .hive/config.yml
  │     写入全局配置模板（含所有选项 + # 注释说明可选值）
  │
  ├─ 4. 生成 .hive/config.local.yml
  │     从 git config 自动填充 user.name 和 user.email
  │     其余字段以注释形式列出供用户按需启用
  │
  ├─ 5. 更新项目根目录 .gitignore
  │     追加以下条目（如不存在）：
  │     .hive/config.local.yml
  │     .hive/state.md
  │     .hive/plans/
  │     .hive/tasks/
  │     .hive/worktrees/
  │
  └─ 6. 输出初始化摘要
        Created: .hive/config.yml
        Created: .hive/config.local.yml (user: chao)
        Updated: .gitignore
        Run `hive doctor` to verify environment.
```

如果 `.hive/` 已存在，`hive init` 不会覆盖现有配置，仅补全缺失的文件和目录。

### 6.4 Agent 命名规则

格式：`<agent工具>-<模型>-<版本>`，版本号之间用 `-` 连接。

示例：
- `claude-opus-4-6`
- `claude-sonnet-4-6`
- `claude-haiku-4-5`
- `codex-gpt-5-4`
- `gemini-2-5-pro`

---

## 7. Skill 系统

Skill 是 Layer 2 实现层的能力扩展。Layer 1（Rust CLI 约束层）不使用 skill——它的行为必须硬编码、确定性、不可绕过。

Hive 的 skill 规范兼容 Claude Code 官方 SKILL 规范，一个 skill 是一个**目录**（不只是一个 .md 文件），可以包含脚本、引用文档、资源文件等。

### 7.1 Skill 目录结构

```
skill-name/
├── SKILL.md                         # 必需：skill 定义（YAML frontmatter + markdown）
├── scripts/                         # 可选：可执行脚本（bash、python、node）
│   ├── run-migration.sh
│   └── validate-schema.py
├── references/                      # 可选：支撑文档（按需加载到上下文）
│   ├── api-guide.md
│   └── platform-notes.md
├── examples/                        # 可选：示例文件
│   └── sample-migration.sql
└── assets/                          # 可选：模板、配置等静态资源
    └── migration-template.sql
```

### 7.2 SKILL.md 格式

兼容 Claude Code 官方 SKILL 规范的 frontmatter 字段：

```markdown
---
name: db-migration
description: Use when task involves database schema changes, migrations, or data transformations
# Optional fields:
# type: flow                         # flow for stateful workflows
# user-invocable: true               # default true, set false for internal skills
# disable-model-invocation: true     # for flow-based skills
# argument-hint: "--table <name>"    # CLI argument description
# allowed-tools: "Bash,Read,Write"   # restrict tool access
# triggers:                          # additional discovery keywords
#   - migration
#   - schema change
#   - ALTER TABLE
---

## Rules

- Always create a reversible migration
- Test migration on a copy of production schema before applying

## Scripts

Run `scripts/run-migration.sh` to execute migration in isolated environment.
```

**Frontmatter 约束：**
- `name` 和 `description` 必需，其余可选
- `name` 只允许字母、数字、连字符
- `description` 聚焦"何时触发"，不描述流程细节，≤500 字符
- frontmatter 总长度 ≤1024 字符

### 7.3 Skill 分层

```
三层 skill 来源：

1. 仓库私有 skill        .hive/skills/<name>/          提交到仓库
2. 用户全局 skill        ~/.config/hive/skills/<name>/   个人本地
3. 系统级 skill（plugin） agent 工具内置（如 humanize、superpowers）
```

目录结构：

```
.hive/skills/                        # 仓库私有 skill（提交）
├── coding-style/
│   └── SKILL.md                     # 纯文档型 skill（只有 SKILL.md）
├── db-migration/
│   ├── SKILL.md
│   ├── scripts/
│   │   └── run-migration.sh         # 可执行脚本
│   └── references/
│       └── schema-conventions.md    # 参考文档
└── deploy-checklist/
    ├── SKILL.md
    └── scripts/
        └── pre-deploy-check.sh

~/.config/hive/skills/               # 用户全局 skill
├── my-rust-patterns/
│   └── SKILL.md
└── review-checklist/
    ├── SKILL.md
    └── references/
        └── review-criteria.md
```

**简写形式**：如果一个 skill 只有 SKILL.md 没有其他文件，也允许直接放一个 `<name>.md` 文件作为简写（自动当作 `<name>/SKILL.md`）。

### 7.4 Skill 查找优先级

spec 中引用的 skill 名称按以下顺序查找，先找到的优先：

```
1. .hive/skills/<name>/SKILL.md      仓库私有（最高优先级）
2. ~/.config/hive/skills/<name>/SKILL.md  用户全局
3. agent 工具内置 plugin              系统级（如 humanize）
```

仓库私有 skill 可以覆盖同名的全局或系统级 skill，实现项目定制。

### 7.5 Skill 配置

```yaml
# .hive/config.yml

skills:
  # Always loaded for all tasks (unless explicitly excluded in spec)
  default:
    - coding-style                   # repo: .hive/skills/coding-style/

  # Available but only loaded when declared in spec
  available:
    - humanize                       # system plugin
    - db-migration                   # repo: .hive/skills/db-migration/
    - deploy-checklist               # repo: .hive/skills/deploy-checklist/
```

- `default`：所有任务自动加载（除非 spec 中用 `exclude_skills` 排除）
- `available`：声明可用 skill 列表，仅在 spec 中显式引用时加载

### 7.6 Spec 中声明 Skill

```markdown
# .hive/specs/chao-a1b2c3d4.md
---
id: chao-a1b2c3d4
skills:
  - humanize                         # 系统级 — RLCR 质量循环
  - db-migration                     # 仓库私有 — DB 迁移流程
exclude_skills:
  - coding-style                     # 排除默认 skill（该任务不需要）
---
```

`hive launch` 根据 spec 计算最终 skill 列表：

```
最终 skill = (config.default - spec.exclude_skills) + spec.skills
```

只加载最终列表中的 skill，worker agent 的上下文保持精简。

### 7.7 `hive launch` 加载行为

```bash
# tool: claude — 加载 skill 目录
claude --plugin humanize \
       --skill .hive/skills/db-migration/ \
       --agent-prompt "..."

# tool: codex — 合并 SKILL.md 到 instructions
codex --prompt "$(cat .hive/tasks/chao-a1b2c3d4/plan.md)" \
      --instructions "$(cat .hive/skills/db-migration/SKILL.md)"

# scripts/ 中的脚本通过 Bash tool 按需调用，不预加载到上下文
```

### 7.8 Skill 管理命令

```
hive skill <subcommand>
```

| 命令 | 作用 |
|------|------|
| `hive skill list` | 列出所有可用 skill（仓库 + 全局 + 系统级），标注来源、类型和加载状态 |
| `hive skill add <name> [--global]` | 创建 skill 目录和 SKILL.md 模板 |
| `hive skill remove <name> [--global]` | 删除指定 skill 目录 |
| `hive skill show <name>` | 查看 skill 内容、脚本清单和解析来源 |
| `hive skill install <url\|path> [--global]` | 从远程或本地安装 skill |
| `hive skill uninstall <name> [--global]` | 卸载已安装的 skill |

### 7.9 安装与卸载流程

**从远程安装（URL/Git）：**

```
# 安装单个 skill 目录
$ hive skill install https://github.com/user/repo/skills/tdd
  │
  ├─ 1. 下载 skill 目录（包含 SKILL.md + scripts/ + references/ 等）
  ├─ 2. 验证 SKILL.md 格式（必须包含 name, description）
  ├─ 3. 复制到 .hive/skills/tdd/（默认仓库级）
  │     或 ~/.config/hive/skills/tdd/（加 --global）
  ├─ 4. 设置 scripts/ 下文件的可执行权限
  └─ 5. 输出：Installed skill: tdd (repo) [SKILL.md, 2 scripts, 1 reference]

# 安装单个 .md 文件（简写形式）
$ hive skill install https://github.com/user/repo/skills/tdd.md
  └─ 创建 .hive/skills/tdd/ 目录，将 .md 文件放入为 SKILL.md
```

**批量安装（skill pack）：**

```
$ hive skill install https://github.com/user/skill-pack
  │
  ├─ 1. clone 仓库到临时目录
  ├─ 2. 扫描 skills/ 目录下所有 skill 子目录
  ├─ 3. 逐个验证 SKILL.md 并复制到 .hive/skills/
  ├─ 4. 设置脚本可执行权限
  └─ 5. 输出安装清单
```

**卸载：**

```
$ hive skill uninstall tdd
  │
  ├─ 1. 查找 .hive/skills/tdd/
  ├─ 2. 检查是否被任何 spec 或 config.yml 引用
  │     ├─ 有引用 → 警告并要求 --force
  │     └─ 无引用 → 删除整个 skill 目录
  └─ 3. 输出：Uninstalled skill: tdd

$ hive skill uninstall tdd --global
  └─ 删除 ~/.config/hive/skills/tdd/
```

**创建自定义 skill：**

```
$ hive skill add my-convention
  │
  ├─ 1. 创建目录结构：
  │     .hive/skills/my-convention/
  │     ├── SKILL.md                 # 模板（含 frontmatter）
  │     ├── scripts/                 # 空目录
  │     └── references/              # 空目录
  │
  ├─ 2. SKILL.md 模板内容：
  │     ---
  │     name: my-convention
  │     description: ""
  │     ---
  │
  │     (write your skill content here)
  │
  └─ 3. 输出：Created skill: .hive/skills/my-convention/

$ hive skill add my-convention --global
  └─ 创建到 ~/.config/hive/skills/my-convention/
```

### 7.10 Skill 类型

| 类型 | 说明 | 示例 |
|------|------|------|
| 文档型 | 只有 SKILL.md，提供规范和指导 | coding-style、review-checklist |
| 脚本型 | 包含 scripts/，提供可执行工具 | db-migration、deploy-checklist |
| 流程型 | `type: flow`，有状态的工作流 | 自定义质量循环 |
| 引用型 | 包含 references/，提供参考文档 | api-guide、platform-notes |
| 混合型 | 以上组合 | 包含脚本 + 引用 + 模板的完整 skill |

---

## 8. 任务状态机

### 8.1 任务执行状态 (status)

```
                    ┌──────────┐
                    │ pending  │
                    └────┬─────┘
                         │ hive claim
                         ▼
                    ┌──────────┐
                    │ assigned │
                    └────┬─────┘
                         │ hive isolate + hive launch
                         ▼
                    ┌──────────────┐
              ┌────►│ in_progress  │
              │     └──┬───┬────┬──┘
              │        │   │    │
              │  成功   │   │    │ 失败
              │        │   │    ▼
              │        │   │  ┌────────┐
              │        │   │  │ failed │
              │        │   │  └───┬────┘
     hive     │        │   │      │
     resume   │        │   │ hive pause
              │        │   ▼      │
              │        │ ┌────────┐│
              └────────┤ │ paused ││
                       │ └────────┘│
                       ▼           │ 编排器决策
                 ┌────────┐        ▼
                 │ review │  ┌───────┐ ┌─────────┐
                 └───┬────┘  │ retry │ │ blocked │
                     │       └───┬───┘ └────┬────┘
            验收通过  │           │          │
                     ▼           │          │ 人工介入
              ┌───────────┐      │          │
              │ completed │      └──► pending ◄──┘
              └───────────┘
```

### 8.2 状态转换规则（Rust 硬编码）

| 当前状态 | 可转换到 | 触发条件 |
|---------|---------|---------|
| pending | assigned | `hive claim`，且所有 `depends_on` 任务已 completed |
| assigned | in_progress | `hive isolate` 创建 worktree + `hive launch` 启动 agent |
| in_progress | paused | `hive pause`，agent 写入 checkpoint 后退出 |
| in_progress | review | Worker 写入 result.md，status: completed |
| in_progress | failed | Worker 写入 result.md，status: failed，或超时 |
| paused | in_progress | `hive resume`，从 checkpoint 恢复 |
| review | completed | `hive check` 验证验收标准全部通过 |
| review | failed | `hive check` 验证不通过 |
| failed | retry → pending | 编排器决定重试（重置任务，清理 worktree） |
| failed | blocked | 需要人工介入或依赖外部条件 |
| blocked | pending | 人工解决后释放 |

### 8.3 硬性约束

- 不可跳跃状态（如 pending 不能直接到 review）
- retry 有上限（默认 3 次，可配置），超出自动转 blocked
- 依赖未满足的任务不能被 claim
- 同一任务同一时刻只有一个 agent 持有
- paused 状态的任务保留 worktree 和 checkpoint，不清理

### 8.4 Checkpoint 机制

Agent 在执行过程中周期性写入 checkpoint（每完成一个 plan step、每完成一轮 RLCR），实现任意中断和恢复。

```markdown
# .hive/tasks/chao-a1b2c3d4/checkpoint.md
---
task_id: chao-a1b2c3d4
status: paused
paused_at: 2026-04-13 11:23
last_commit: b3c4d5e
plan_step: 3/7
rlcr_round: 2/5
---

## Progress
- [x] Step 1: Create middleware module
- [x] Step 2: Implement JWT validation
- [x] Step 3: Add error handling (in progress, 60%)
- [ ] Step 4: Route integration
- [ ] Step 5: Unit tests
- [ ] Step 6: Integration tests
- [ ] Step 7: Documentation

## Uncommitted Work
- src/middleware/auth.rs (modified, not committed)

## Resume Instructions
Continue from Step 3: error handling for edge cases.
The JWT validation is complete and committed.
```

### 8.5 Pause 流程

```
hive pause --task chao-a1b2c3d4
    │
    ├─ 1. 发送 SIGTERM 给 worktree 中的 agent 进程
    ├─ 2. Agent 收到信号后：
    │     ├─ 将当前进度写入 checkpoint.md
    │     ├─ git commit 当前工作（如有未提交变更，commit message 标记 [hive:paused]）
    │     └─ 优雅退出
    ├─ 3. Rust CLI 更新状态：in_progress → paused
    ├─ 4. 追加 audit.md：paused at ...
    └─ 5. Worktree 保留不清理

hive pause --all
    └─ 暂停所有 in_progress 任务
```

### 8.6 Resume 流程

```
hive resume --task chao-a1b2c3d4
    │
    ├─ 1. 读取 checkpoint.md 获取断点信息
    ├─ 2. 更新状态：paused → in_progress
    ├─ 3. 重新启动 agent，注入 checkpoint 上下文：
    │     "Resume from Step 3. Steps 1-2 completed.
    │      Last commit: b3c4d5e. See checkpoint.md for details."
    ├─ 4. 追加 audit.md：resumed at ...
    └─ 5. Agent 在同一 worktree 继续工作

hive resume --all
    └─ 恢复所有 paused 任务
```

### 8.7 异常中断恢复

如果 agent 进程被 kill（非优雅退出，无 checkpoint）：

```
hive resume --task chao-a1b2c3d4
    │
    ├─ 1. 检测到 checkpoint.md 不存在或已过期
    ├─ 2. 从 worktree 的 git log 推断进度
    │     （最后一次 commit 对应哪个 plan step）
    ├─ 3. 自动生成 checkpoint.md
    ├─ 4. 正常 resume 流程
    └─ 5. audit.md 标记：recovered from crash
```

### 8.8 计划审批状态 (plan_status)

```
draft → rfc → approved → executing → done
```

- `draft`：`hive plan` 生成中/刚生成
- `rfc`：`hive rfc` 已提交 PR 等待团队审查
- `approved`：PR 通过或用户直接批准
- `executing`：`hive exec` 正在执行
- `done`：执行完成

只有 `plan_status: approved` 的任务才允许被 `hive exec` 调度执行。

---

## 9. 计划生成流程

参考 superpowers brainstorming 的结构化设计流程，分 7 个阶段。

`hive plan` 的交互通过**状态机驱动的对话流程**实现。Rust CLI 不自己做问答，而是作为状态机后端，与前端 agent 工具配合：

```
Agent 工具（Claude Code / Codex / ...）
       ↕ 对话界面
     用户
       ↕ CLI 调用
  hive plan CLI（Rust 状态机）
       ↕ 文件读写
  .hive/plans/<draft_id>/
```

- `hive plan next` — 返回当前 phase 和下一步动作（该问什么问题、该生成什么方案）
- `hive plan answer --draft <id> --phase <n> --response "..."` — 提交用户回答，推进状态机
- `hive plan status --draft <id>` — 查看当前 draft 的进度

这使得 Hive 的计划流程可以嵌入任何支持对话的 agent 工具，不绑定特定平台。

每次 `hive plan` 创建一个独立的 draft（`<user>-<content_hash>`），不同需求各自独立。

### Phase 1: 探索上下文

- 自动扫描代码库结构
- 读取现有文档、最近 commit
- 生成项目现状摘要
- 产出：内部使用，不持久化

### Phase 2: 交互式澄清

- 逐个提问（≥3 个问题）
- 优先多选题，降低用户负担
- 追问直到需求无歧义
- 产出：`.hive/plans/<draft_id>/requirements.md`（gitignore，本地决策参考）

### Phase 3: 提出 2-3 种方案

- 每种方案给出权衡分析
- 明确推荐一种并说明理由
- 用户选择或要求修改

### Phase 4: 收敛式设计

- 多模型独立生成设计方案
- 标记共识 / 分歧
- 收敛轮次（最多 3 轮）
- 收敛条件：
  - 分歧数 = 0 → 自动收敛
  - 连续 2 轮分歧不减少 → 提交用户裁决
  - 达到最大轮次 → 提交用户裁决
- 逐段呈现设计，每段确认
- 产出：`.hive/plans/<draft_id>/convergence.md`（gitignore，本地决策参考）

### Phase 5: 自审

- 占位符扫描（TBD/TODO）
- 内部一致性检查
- 歧义检查
- 范围检查（是否需要进一步分解）
- 自动修复发现的问题

### Phase 6: 任务分解 + Plan 生成

分两步：

1. **Hive 分解**：将收敛后的设计分解为 task 列表，每个 task 生成 `.hive/specs/<id>.md`（目标、验收标准、上下文文件、复杂度、依赖关系）
2. **Plan 生成**：对每个 task，将 spec 作为输入调用可配置的 plan 生成工具（如 `humanize:gen-plan`）生成 `.hive/plans/<draft_id>/<task_id>.md`

产出：`.hive/specs/<id>.md`（提交）+ `.hive/plans/<draft_id>/<task_id>.md`（gitignore）

### Phase 7: 用户审批

- 展示完整任务列表 + 依赖图
- 用户可调整复杂度、RLCR 轮次、依赖关系
- 批准 → `plan_status: approved`
- 或 `hive rfc` 进入团队 PR 审查流程

---

## 10. 任务文件格式

### 9.1 specs/<id>.md

```markdown
# .hive/specs/chao-a1b2c3d4.md
---
id: chao-a1b2c3d4
draft_id: chao-b7c8d9e0
status: pending
plan_status: draft
depends_on: []
complexity: M
rlcr_max_rounds: 5
---

## Goal
Implement user authentication middleware

## Acceptance Criteria
- [ ] JWT token validation passes all edge cases
- [ ] Middleware integrates with existing route handler
- [ ] Unit tests cover token expiry, invalid signature, missing header

## Context Files
- src/middleware/mod.rs
- src/routes/auth.rs
```

### 9.2 tasks/<id>/result.md

```markdown
# .hive/tasks/chao-a1b2c3d4/result.md
---
id: chao-a1b2c3d4
status: completed
branch: hive/chao-a1b2c3d4
commit: a1b2c3d
base_commit: e8f9g0h
---

## Dependencies
- base: main @ e8f9g0h
- depends_on:
  - chao-b2c3d4e5 (merged @ f1g2h3i)
  - chao-c3d4e5f6 (merged @ g2h3i4j)

## Environment
- model: claude-opus-4-6
- reviewer: codex-gpt-5-4
- humanize: v1.16.0

## Summary
Implemented JWT authentication middleware with three validation paths.

## Changes
| File | Action | Lines |
|------|--------|-------|
| src/middleware/auth.rs | new | +87 |
| src/routes/auth.rs | modified | +12 -3 |
| src/middleware/mod.rs | modified | +1 |

## Acceptance Criteria Verification
- [x] JWT token validation passes all edge cases
- [x] Middleware integrates with existing route handler
- [x] Unit tests cover token expiry, invalid signature, missing header

## RLCR Summary
- Rounds: 3 / 5 (max)
- Round 1: 2 issues (P1: missing error handling, P2: naming)
- Round 2: 1 issue (P2: edge case test)
- Round 3: 0 issues, clean

## Test Results
- Passed: 12
- Failed: 0
- Skipped: 0

## Notes
Reused existing `TokenValidator` trait, added `JwtValidator` implementation.
```

### 9.3 state.md

```markdown
---
project: user-auth-system
created: 2026-04-13
audit_level: standard
---

## Tasks
| ID | Status | Plan Status | Depends | Assignee | Branch |
|----|--------|-------------|---------|----------|--------|
| chao-a1b2c3d4 | completed | done | — | worker-1 | hive/chao-a1b2c3d4 |
| chao-f5e6d7c8 | in_progress | executing | chao-a1b2c3d4 | worker-2 | hive/chao-f5e6d7c8 |
| chao-g6h7i8j9 | pending | approved | chao-a1b2c3d4 | — | — |
```

---

## 11. 复杂度与 RLCR 轮次

`hive plan` 分解任务时自动评估复杂度并推荐 RLCR 轮次，用户审批时可调整。

| 复杂度 | 特征 | 推荐 RLCR 轮次 |
|--------|------|----------------|
| S | 单文件改动，逻辑简单 | 2 |
| M | 2-5 文件，有接口对接 | 5 |
| L | 5+ 文件，跨模块，新架构 | 8 |

---

## 12. 冲突管理策略

### 11.1 核心原则

多个任务修改同一文件是并行开发的正常产物，不是错误。Hive 的职责是按正确顺序合并，尽量自动解决，解决不了的交给人。

### 11.2 依赖图决定合并顺序

`hive plan` 分解任务时根据逻辑依赖关系确定顺序。无依赖的任务可以并行执行，但按依赖顺序串行合并。

### 11.3 合并策略

```
hive merge --task <id>
    │
    ├─ 1. 将 hive/<task_id> 分支 rebase 到当前 main
    │
    ├─ 2. 无冲突 → 自动合并 ✓
    │
    ├─ 3. 有冲突 → 根据 conflict_strategy：
    │      ├─ auto: 启动 agent 解决冲突
    │      │        给它冲突文件 + 两个任务的 spec.md
    │      │        解决后再跑 hive check 验证验收标准
    │      └─ manual: 标记为 blocked，通知人工处理
    │
    └─ 4. 合并完成后，后续待合并任务自动 rebase
```

### 11.4 GitHub 协作模式

每个任务对应一个 PR：

```
main
 ├── hive/chao-a1b2c3d4  →  PR #1（先合并）
 ├── hive/chao-f5e6d7c8  →  PR #2（rebase on main after PR #1）
 └── hive/chao-g6h7i8j9  →  PR #3（rebase on main after PR #1）
```

`hive merge --mode pr` 自动创建 PR 而非直接合并。

### 11.5 配置项

| 场景 | conflict_strategy | mode |
|------|-------------------|------|
| 个人开发 | auto + direct | 快速合并 |
| 团队协作 | auto + pr | 每个任务一个 PR，CI 验证后合并 |
| 高合规 | manual + pr | 冲突必须人工解决 |

---

## 13. RFC 流程

### 12.1 `hive rfc` 命令

```
hive rfc --task <id>
    │
    ├─ 1. 将 .hive/specs/<id>.md 提交到 hive/<task_id> 分支
    ├─ 2. gh pr create --title "RFC: <task goal>" --label rfc
    ├─ 3. plan_status: draft → rfc
    └─ 4. 输出 PR 链接

hive rfc --all
    │
    └─ 对所有 draft 状态的任务批量创建 RFC PR
```

RFC 只提交 spec（做什么），不提交 plan（怎么做）。团队审查的是目标和验收标准，实施细节由执行者决定。

### 12.2 Plan 状态流转

```
draft       hive plan 生成完成
  │
  ├─ hive rfc → rfc（团队审查）→ PR approved → approved
  │
  └─ 用户直接批准 → approved
        │
        ├─ hive exec → executing
        │
        └─ 完成 → done
```

---

## 14. 审计系统

### 13.1 三档审计等级

| 等级 | 记录内容 | 适用场景 |
|------|---------|---------|
| minimal | 任务状态变更、最终结果、merge 记录 | 个人项目、快速迭代 |
| standard | minimal + 每轮 RLCR 摘要、收敛过程、重试原因 | 日常团队开发 |
| full | standard + 每次代理决策理由、完整 prompt/response 摘要、diff 逐条追溯 | 合规要求、事后复盘 |

### 13.2 任务级审计日志

每个任务独立一份 `audit.md`，追加写入（不可变）：

```markdown
---
task_id: chao-a1b2c3d4
audit_level: standard
---

## Timeline

### 2026-04-13 10:03 — claimed
Assigned to worker-1

### 2026-04-13 10:03 — worktree created
Branch: hive/chao-a1b2c3d4
Path: .hive/worktrees/chao-a1b2c3d4
Base commit: e8f9g0h

### 2026-04-13 10:04 — agent launched
Model: claude-opus-4-6
Context: src/middleware/mod.rs, src/routes/auth.rs

### 2026-04-13 10:31 — RLCR round 1 complete
Commits: 3
Review issues: 2 (P1: missing error handling, P2: naming convention)

### 2026-04-13 10:45 — RLCR round 2 complete
Review issues: 0, all resolved

### 2026-04-13 10:46 — result submitted
Status: completed, Commit: a1b2c3d

### 2026-04-13 10:47 — verification passed
Acceptance criteria: 3/3 ✓

### 2026-04-13 10:48 — merged
Merged to main, commit: e4f5g6h
```

### 13.3 最终审计报告

某个 draft 的所有任务完成后，`hive audit --draft <id>` 聚合生成 per-draft 报告：

```markdown
# .hive/reports/chao-b7c8d9e0.md
---
draft_id: chao-b7c8d9e0
project: user-auth-system
started: 2026-04-13 09:30
completed: 2026-04-13 14:22
audit_level: standard
---

## Overview
- Total tasks: 8
- Completed: 7
- Failed → retried: 1 (chao-e4f5g6h7, retry 1x succeeded)
- Blocked: 0
- Total RLCR rounds: 18
- Human interventions: 2 (plan approval, retry decision)

## Plan Convergence
- Models: claude-opus-4-6, codex-gpt-5-4
- Convergence rounds: 2
- Disagreements resolved: 3 (auto), 1 (user decision)

## Task Execution Summary
| ID | Goal | Status | RLCR Rounds | Duration | Worker |
|----|------|--------|-------------|----------|--------|
| chao-a1b2c3d4 | Auth middleware | ✓ | 2 | 44min | worker-1 |
| chao-f5e6d7c8 | Route handlers | ✓ | 3 | 52min | worker-2 |
| chao-g6h7i8j9 | DB migration | ✓ | 1 | 18min | worker-1 |

## Merge History
| Order | Task | Branch | Commit | Conflicts |
|-------|------|--------|--------|-----------|
| 1 | chao-a1b2c3d4 | hive/chao-a1b2c3d4 | a1b2c3d | none |
| 2 | chao-g6h7i8j9 | hive/chao-g6h7i8j9 | b2c3d4e | none |
| 3 | chao-f5e6d7c8 | hive/chao-f5e6d7c8 | c3d4e5f | 1 file (auto-resolved) |

## Issues & Decisions
1. **chao-e4f5g6h7 failed (round 1)** — OOM during test, retried with reduced batch size
2. **Convergence disagreement #4** — User chose Claude's approach for error handling

## Artifacts
- Task specs: .hive/specs/
- Per-task work files: .hive/tasks/
```

### 13.4 审计写入权限

- Rust CLI（Layer 1）控制写入，agent 不能直接修改审计文件
- 每条记录只追加不修改（append-only）

---

## 15. CLI 命令设计

```
hive <command> [options]
```

### 14.1 Layer 0 — 编排命令（人类直接使用）

| 命令 | 作用 | 阶段 |
|------|------|------|
| `hive init` | 初始化仓库（见 Section 6.3） | 环境准备 |
| `hive config [--show]` | 查看/修改配置（显示合并后生效值及来源） | 环境准备 |
| `hive plan --input <file>` | 启动计划生成流程（7 阶段） | 规划 |
| `hive rfc --task <id> \| --all` | 提交 spec 到仓库并创建 RFC PR | 审查 |
| `hive exec` | 按计划调度执行所有 approved 任务 | 执行 |
| `hive status` | 查看全局状态（任务表 + 进度） | 监控 |
| `hive audit --draft <id>` | 生成 per-draft 审计报告到 `.hive/reports/` | 报告 |
| `hive merge --task <id> \| --all` | 合并已完成任务（支持 `--mode pr`） | 集成 |
| `hive pause --task <id> \| --all` | 暂停任务，写入 checkpoint，保留 worktree（见 Section 8.5） | 控制 |
| `hive resume --task <id> \| --all` | 从 checkpoint 恢复执行（见 Section 8.6） | 控制 |
| `hive abort` | 强制终止所有运行中的 agent，保留 worktree 供复盘 | 应急 |
| `hive skill <sub>` | Skill 管理（list/add/remove/install/uninstall/show，见 Section 7.6） | 管理 |
| `hive doctor` | 检查环境（git、agent 工具、skill、配置的模型等） | 诊断 |

### 14.2 Layer 1 — 子代理命令（由 `hive exec` 内部调用）

| 命令 | 作用 |
|------|------|
| `hive claim --task <id>` | 领取任务，pending → assigned |
| `hive isolate --task <id>` | 创建 worktree + 任务分支，记录 base_commit |
| `hive launch --task <id>` | 在 worktree 中启动配置的 agent 工具 + 质量循环 |
| `hive check --task <id>` | 最终验收：对照 `specs/<id>.md` 验收标准逐项验证 |
| `hive report --task <id>` | 读取 result.md，更新 state.md + audit.md |
| `hive retry --task <id>` | 清理 worktree，重置任务为 pending |
| `hive cleanup --task <id>` | 删除已合并任务的 worktree |

### 14.3 辅助命令

| 命令 | 作用 |
|------|------|
| `hive list-tasks [--status <s>]` | 列出任务（可按状态过滤） |
| `hive show --task <id>` | 查看任务详情（spec + plan + result + audit） |
| `hive graph` | 输出任务依赖图（文本格式） |

### 14.4 典型执行流程

```bash
$ hive init
$ hive config --audit standard

$ hive plan --input feature-spec.md
  # Phase 1-7: 探索 → 澄清 → 方案 → 收敛 → 自审 → 分解(gen-plan) → 审批

$ hive rfc --all
  # 为所有任务创建 RFC PR，团队审查

$ hive exec
  # 自动调度：
  #   1. 扫描 approved + pending 任务，按依赖图确定可并行任务
  #   2. hive claim → hive isolate → hive launch
  #   3. agent 在 worktree 内用 humanize RLCR 执行 plan.md
  #   4. agent 完成后写 result.md
  #   5. hive check 最终验收
  #   6. 失败则 hive retry（不超过上限）
  #   7. 依赖解除后调度新任务
  #   8. 所有任务 completed → 提示用户

$ hive status
$ hive merge --all --mode pr
$ hive audit
```

### 14.5 `hive launch` 内部行为

根据 `launch.tool` 配置选择对应的启动方式：

```bash
# tool: claude (with humanize quality loop)
cd .hive/worktrees/chao-a1b2c3d4
cp .hive/tasks/chao-a1b2c3d4/plan.md .humanize/plan.md
claude --agent-prompt "Execute the plan using humanize:start-rlcr-loop --max 5" \
       --plugin humanize

# tool: codex (with codex-builtin quality loop)
cd .hive/worktrees/chao-a1b2c3d4
codex --approval-mode full-auto \
      --prompt "$(cat .hive/tasks/chao-a1b2c3d4/plan.md)"

# tool: custom
cd .hive/worktrees/chao-a1b2c3d4
my-agent --task chao-a1b2c3d4 --worktree .hive/worktrees/chao-a1b2c3d4
```

所有工具的共同约定：agent 完成后将结果写入 `.hive/tasks/<task_id>/result.md`。

---

## 16. 可复现性

每个 task 的 result.md 包含完整的环境快照，确保任何人都可以复现：

- `base_commit`：worktree 创建时 main 的 HEAD
- `depends_on`：依赖任务及其 merge commit
- `Environment`：使用的模型、humanize 版本
- `branch`：任务分支名
- `commit`：最终提交的 SHA

复现步骤：

```bash
git checkout <base_commit>
# cherry-pick 依赖任务的变更
git cherry-pick <依赖任务的 merge commits>
# 用相同的 specs/<id>.md + tasks/<id>/plan.md 重新执行
hive retry --task <id>
```

---

## 17. 执行流程全景图

```
用户输入需求
    │
    ▼
hive plan (Layer 0)
    ├─ Phase 1: 探索上下文
    ├─ Phase 2: 交互式澄清 → requirements.md
    ├─ Phase 3: 提出 2-3 种方案
    ├─ Phase 4: 收敛式设计 → convergence.md
    ├─ Phase 5: 自审
    ├─ Phase 6: 任务分解 → specs/<id>.md
    │           Plan 生成 → tasks/<id>/plan.md
    └─ Phase 7: 用户审批
    │
    ▼
hive rfc (可选，团队审查)
    ├─ 提交 specs/<id>.md
    └─ 创建 RFC PR → 团队 review → approved
    │
    ▼
hive exec (Layer 1 调度)
    ├─ hive claim → hive isolate → hive launch
    │                                   │
    │                                   ▼
    │                          Layer 2: worktree 内
    │                          Agent 工具 + 质量循环
    │                              ├─ 实现代码
    │                              ├─ 过程审查（Agent 工具内部）
    │                              ├─ 修复 → 循环
    │                              └─ 写 result.md
    │                                   │
    │              ◄────────────────────┘
    ├─ hive check (最终验收)
    ├─ hive report (更新状态 + 审计)
    └─ 调度下一批任务
    │
    ▼
hive merge --all (集成)
    ├─ 按依赖顺序 rebase + 合并
    ├─ 冲突自动解决或人工处理
    └─ 每个任务一个 PR（团队模式）
    │
    ▼
hive audit --draft <id> (审计报告)
    └─ 聚合生成 reports/<draft_id>.md
```

---

## 18. Plugin 封装与导出

Hive 核心是 Rust CLI，针对不同 agent 工具提供适配层。优先以 plugin 形式封装（如 Claude Code plugin），不支持 plugin 的 agent 工具退化为 skill 文件导入。

### 18.1 导出的用户命令

以下命令面向用户，需要导出为 plugin command / skill：

| 命令 | 导出名称 | 说明 |
|------|---------|------|
| `hive init` | `hive:init` | 初始化仓库 |
| `hive plan` | `hive:plan` | 启动计划生成流程 |
| `hive rfc` | `hive:rfc` | 提交 spec 创建 RFC PR |
| `hive exec` | `hive:exec` | 调度执行所有 approved 任务 |
| `hive status` | `hive:status` | 查看全局状态 |
| `hive pause` | `hive:pause` | 暂停任务 |
| `hive resume` | `hive:resume` | 恢复任务 |
| `hive merge` | `hive:merge` | 合并已完成任务 |
| `hive audit` | `hive:audit` | 生成审计报告 |
| `hive skill` | `hive:skill` | Skill 管理 |
| `hive doctor` | `hive:doctor` | 环境检查 |
| `hive graph` | `hive:graph` | 任务依赖图 |

### 18.2 不导出的内部命令

以下命令由 `hive exec` 内部调用（Layer 1），不暴露给用户：

```
hive claim / hive isolate / hive launch / hive check
hive report / hive retry / hive cleanup
```

### 18.3 Claude Code Plugin（优先）

完整的 plugin 封装，包含 skills、hooks、agents：

```
.claude-plugin/
├── plugin.json                      # 插件元数据

skills/
├── hive-init/SKILL.md               # /hive:init
├── hive-plan/SKILL.md               # /hive:plan （交互式，状态机驱动）
├── hive-rfc/SKILL.md                # /hive:rfc
├── hive-exec/SKILL.md               # /hive:exec
├── hive-status/SKILL.md             # /hive:status
├── hive-pause/SKILL.md              # /hive:pause
├── hive-resume/SKILL.md             # /hive:resume
├── hive-merge/SKILL.md              # /hive:merge
├── hive-audit/SKILL.md              # /hive:audit
├── hive-skill/SKILL.md              # /hive:skill
├── hive-doctor/SKILL.md             # /hive:doctor
└── hive-graph/SKILL.md              # /hive:graph

hooks/
├── hooks.json
├── hive-orchestrator-guard.sh       # PreToolUse: 阻止 Layer 0 agent 写代码
└── hive-exec-stop-gate.sh           # Stop: 任务未全部完成时阻止退出

agents/
├── hive-planner.md                  # Layer 0 规划代理（禁止编码）
└── hive-worker.md                   # Layer 2 实现代理（worktree 内自由）
```

**plugin.json 示例：**

```json
{
  "name": "hive",
  "description": "Agent-agnostic multi-agent orchestration framework",
  "version": "0.1.0",
  "author": { "name": "hive" },
  "license": "MIT"
}
```

**Skill 示例（hive-plan/SKILL.md）：**

```markdown
---
name: hive:plan
description: Start interactive planning flow for a new feature or requirement
user_invocable: true
---

This skill drives the Hive planning flow (Phase 1-7) through a state-machine
driven conversation. It wraps `hive plan` CLI commands.

## Usage
/hive:plan --input <requirements_file>

## Flow
1. Call `hive plan start --input <file>` to create a new draft
2. Loop: call `hive plan next --draft <id>` to get next action
3. Present question/options to user, collect response
4. Call `hive plan answer --draft <id> --phase <n> --response "..."`
5. Repeat until all phases complete
6. Present task list + dependency graph for user approval
```

**Hook 示例（hive-orchestrator-guard.sh）：**

```bash
#!/bin/bash
# PreToolUse hook: block code-writing tools when running as Layer 0 orchestrator
if [ "$HIVE_ROLE" = "orchestrator" ]; then
    if [[ "$TOOL_NAME" =~ ^(Write|Edit|NotebookEdit)$ ]]; then
        echo '{"result":"block","message":"Layer 0 orchestrator cannot write code"}'
        exit 0
    fi
fi
echo '{"result":"allow"}'
```

### 18.4 Codex CLI 适配

Codex 不支持完整 plugin 体系，使用 instructions 文件和 hook 适配：

```
.codex/
├── instructions.md                  # Codex 系统指令，引导调用 hive CLI
└── hooks.json                       # Codex hook（如有支持）
```

**instructions.md 示例：**

```markdown
You are working in a Hive-managed repository. Use the `hive` CLI for all
orchestration tasks:

- `hive plan --input <file>` to start planning
- `hive exec` to execute approved tasks
- `hive status` to check progress
- `hive pause/resume` to control tasks
- `hive merge --all` to integrate completed work
- `hive audit --draft <id>` to generate reports

Do NOT write code directly in the main workspace. All implementation
must happen through `hive exec` which creates isolated worktrees.
```

### 18.5 通用 Skill 文件适配

对于不支持 plugin 也不支持特定集成格式的 agent 工具（如 OpenCode 等），提供独立的 skill markdown 文件：

```
adapters/
├── claude/                          # Claude Code plugin（完整）
│   ├── .claude-plugin/
│   ├── skills/
│   ├── hooks/
│   └── agents/
├── codex/                           # Codex 适配
│   └── .codex/
└── generic/                         # 通用 skill 文件
    ├── hive-init.md
    ├── hive-plan.md
    ├── hive-exec.md
    ├── hive-status.md
    ├── hive-pause.md
    ├── hive-resume.md
    ├── hive-merge.md
    ├── hive-audit.md
    ├── hive-skill.md
    └── hive-doctor.md
```

通用 skill 文件格式与 Claude Code SKILL.md 相同（YAML frontmatter + markdown），任何支持 markdown skill 的 agent 工具都可以直接加载。

### 18.6 适配层选择

`hive init` 根据检测到的 agent 工具自动安装对应适配层：

```
$ hive init
  │
  ├─ 检测 agent 工具环境
  │   ├─ 发现 claude CLI → 安装 Claude Code plugin 适配
  │   ├─ 发现 codex CLI → 安装 Codex 适配
  │   └─ 其他 → 安装通用 skill 文件
  │
  └─ 输出：
     Detected: claude (Claude Code)
     Installed: Claude Code plugin adapter
     Skills: 12 commands exported as hive:* skills
```

多个 agent 工具共存时，可以同时安装多个适配层。

--- Original Design Draft End ---
