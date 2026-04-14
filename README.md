# Hive — Multi-Agent Orchestration Harness

Hive is an agent-tool-agnostic orchestration harness for team-based multi-agent task execution. It coordinates AI coding agents (Claude Code, Codex CLI, or any custom CLI tool) through a three-layer architecture, enforcing isolation, state management, and quality control via a standalone Rust CLI.

## Architecture

```
Layer 0: Orchestrator (hive CLI)
├─ Task decomposition + dependency graph
├─ State machine, audit, scheduling
⛔ Never writes code or edits worktree contents

Layer 1: Sub-agent (hive CLI commands)
├─ claim, isolate, launch, check, report
⛔ Hard-coded constraints, not natural language

Layer 2: Worker (AI agent in worktree)
├─ Writes code, runs tests, commits
├─ Full freedom within its own worktree
⛔ Cannot access other worktrees or the main branch
```

## Quick Start

```bash
# Build from source
cd hive
cargo build --release

# Initialize in a git repository
hive init

# Check environment health
hive doctor
```

`hive init` creates the `.hive/` directory structure, generates config files, detects available agent CLIs (Claude Code, Codex), and installs default plugins.

## Workflow

```
init → plan (spec + plan.md) → rfc → approve → exec → merge
```

1. **Plan**: Create task specs (`.hive/specs/<id>.md`) and implementation plans (`.hive/plans/<draft_id>/<task_id>.md`)
2. **RFC**: `hive rfc --draft <id>` aggregates specs and plans into a single RFC document for team review
3. **Approve**: `hive approve --draft <id>` gates tasks for execution after team consensus
4. **Exec**: `hive exec` orchestrates the full chain — claim → isolate → launch → check → report — respecting dependency order with retry logic
5. **Merge**: `hive merge --task <id>` rebases onto main and creates a PR, or `hive merge --all` processes completed tasks in dependency order

## Commands

### Core Workflow

| Command | Description |
|---------|-------------|
| `hive init` | Initialize project, generate config and agent adapters |
| `hive exec` | Orchestrate full execution chain for approved tasks |
| `hive merge --task <id>` | Rebase task branch and create PR |
| `hive merge --all` | Merge all completed tasks in dependency order |
| `hive rfc --draft <id>` | Generate RFC document aggregating specs + plans |
| `hive approve --draft <id>` | Approve a draft for execution |

### Task Lifecycle (Layer 1)

| Command | Description |
|---------|-------------|
| `hive claim --task <id>` | Claim a pending task (acquires lock) |
| `hive isolate --task <id>` | Create git worktree for the task |
| `hive launch --task <id>` | Start the configured agent in the worktree |
| `hive check --task <id>` | Verify acceptance criteria (exit: 0/1/2/3) |
| `hive report --task <id>` | Process worker's result.md |
| `hive retry --task <id>` | Retry a failed task |
| `hive cleanup --task <id>` | Remove worktree after merge |

### Diagnostics

| Command | Description |
|---------|-------------|
| `hive status` | Show task status overview |
| `hive doctor` | Validate environment, config, state consistency |
| `hive graph` | Display task dependency graph |
| `hive show --task <id>` | Show detailed task information |
| `hive list-tasks` | List all tasks with optional state filter |
| `hive config --show` | Display merged config with source annotations |
| `hive audit --task <id>` | Query per-task audit log |

## Configuration

Dual-layer YAML configuration with deep merge:

- `.hive/config.yml` — team-shared settings (committed)
- `.hive/config.local.yml` — personal overrides (gitignored)

```yaml
# .hive/config.yml
audit_level: standard          # minimal | standard | full
launch:
  tool: claude                 # claude | codex | custom
rfc:
  platform: github             # github | gitlab | none
skills:
  default:
    - coding-style
```

`hive config --show` displays the effective merged configuration with source annotations for each field.

## Task State Machine

Seven states with hard-coded transition rules:

```
pending → assigned → in_progress → review → completed
                         ↓            ↓
                       failed ←───────┘
                         ↓
              blocked (retry limit exceeded)
```

- **pending → assigned**: all dependencies must be completed
- **failed → pending**: automatic retry up to `retry_limit` (default: 3)
- **failed → blocked**: retry limit exceeded, requires manual intervention

## Task Identity

- **Format**: `<user_name>-<ulid>` (e.g., `chao-01JRZK5M3FNPB9Y0VPW2QR8X7E`)
- **Stable**: identity does not change when spec content is edited
- **Git branch**: `hive/<task_id>`

## Directory Structure

```
.hive/
├── config.yml              # Team config (committed)
├── config.local.yml        # Personal overrides (gitignored)
├── state.md                # Derived status view (gitignored)
├── specs/                  # Task specs (gitignored, aggregated into RFC)
├── plans/                  # Per-draft implementation plans (gitignored)
│   └── <draft_id>/
│       └── <task_id>.md
├── rfcs/                   # RFC documents (committed)
├── tasks/                  # Runtime state per task (gitignored)
│   └── <task_id>/
│       ├── state.json      # Authoritative task state
│       ├── audit.md        # Append-only audit log (HMAC-signed)
│       └── lock            # flock-based concurrency control
├── skills/                 # Repo-level skills
└── worktrees/              # Git worktrees (gitignored)
```

`state.json` is the single source of truth. `state.md` is a derived view, never used as authoritative input.

## Agent Backends

| Backend | Config | Launch Behavior |
|---------|--------|-----------------|
| Claude Code | `launch.tool: claude` | Plugin reference + agent prompt with task plan |
| Codex CLI | `launch.tool: codex` | `--approval-mode full-auto` with skills merged into instructions |
| Custom | `launch.tool: custom` | `custom_command` with `{task_id}` and `{worktree_path}` substitution |

## Skill System

Three-tier skill resolution (repo > user > system):

1. `.hive/skills/<name>/SKILL.md` — repo-level (highest priority)
2. `~/.config/hive/skills/<name>/SKILL.md` — user-level
3. System plugins — agent-tool built-in

Skills are injected into the agent launch context. Configure defaults in `config.yml`:

```yaml
skills:
  default: [coding-style]     # Auto-loaded for all tasks
```

Per-task skill selection via spec frontmatter:

```yaml
skills: [humanize, db-migration]
exclude_skills: [coding-style]
```

## Audit System

Three-level append-only logging with HMAC-SHA256 integrity:

- **minimal**: state changes, final results, merge events
- **standard**: + RLCR round summaries, retry reasons
- **full**: + agent decision rationale

`hive doctor` verifies audit integrity and detects tampering.

## Concurrency

- Per-task file locking via `flock(2)`
- Orchestrator-level lock prevents concurrent `hive exec`
- Atomic state writes (tmp file + rename)
- Stale lock detection (PID check + 5-minute age threshold)

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `hive-cli` | CLI entry point, clap command definitions |
| `hive-core` | State machine, task model, config, schema validation, locking |
| `hive-git` | Git worktree and branch operations |
| `hive-audit` | Audit logging with HMAC integrity |
| `hive-adapter` | Agent tool adapter generation (Claude, Codex, generic) |

## Platform Requirements

- Linux or macOS (flock-based locking; Windows deferred)
- Git CLI
- Rust stable toolchain (for building)
- Optional: `gh` (GitHub CLI) or `glab` (GitLab CLI) for PR creation
- Optional: `claude` and/or `codex` CLI for agent backends

## License

MIT
