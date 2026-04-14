# Hive v2 Design Notes

Incremental design decisions for v2 features. Each section documents a specific design choice that deviates from or refines the original draft.

## Pause/Resume via File-Based State Communication

**Original draft (Section 8.5-8.7)**: Used SIGTERM to pause agents. Agent receives signal, writes checkpoint, exits gracefully. This was deferred from v1 because it requires agents to support SIGTERM + checkpoint write, which is not a universal CLI contract.

**v2 design**: Replace SIGTERM with a file-based notification protocol, consistent with hive's overall communication model (git + structured Markdown files).

### Protocol

```
hive pause --task <id>
  1. Write marker file: .hive/tasks/<id>/pause-requested
  2. Task state remains in_progress (waiting for agent to respond)
  3. Agent periodically checks for the pause-requested marker
  4. Agent discovers marker → writes checkpoint.md → git commit → exits
  5. hive detects agent exit + checkpoint.md present → transitions to paused

hive resume --task <id>
  1. Read checkpoint.md for resumption context
  2. Delete pause-requested marker
  3. Transition state: paused → in_progress
  4. Relaunch agent with checkpoint context injected into prompt

hive pause --all
  → Write pause-requested for all in_progress tasks

hive resume --all
  → Resume all paused tasks
```

### State Machine Changes

Add `paused` state (7 → 8 states):

```
in_progress → paused    (agent wrote checkpoint after seeing pause-requested)
paused → in_progress    (hive resume)
```

### Checkpoint File Format

```markdown
# .hive/tasks/<task_id>/checkpoint.md
---
task_id: <task_id>
status: paused
paused_at: 2026-04-14 11:23
last_commit: b3c4d5e
---

## Progress
- [x] Step 1: completed
- [x] Step 2: completed
- [ ] Step 3: in progress (60%)
- [ ] Step 4-7: not started

## Resume Instructions
Continue from Step 3. Steps 1-2 are committed.
```

### Agent Contract

Agents must implement one behavior to support pause:

> Periodically check for `.hive/tasks/<task_id>/pause-requested`. When found, write `checkpoint.md` with current progress, commit work, and exit with code 0.

The check frequency is agent-defined (recommended: after each logical step, each RLCR round, or every N minutes). hive does NOT dictate timing — the agent pauses at its next convenient safe point.

### Crash Recovery (No Checkpoint)

If the agent exits without writing checkpoint.md (killed, crash, OOM):

```
hive resume --task <id>
  1. Detect checkpoint.md missing
  2. Infer progress from worktree git log (last commit message)
  3. Auto-generate checkpoint.md from git history
  4. Resume with inferred context
  5. Audit entry: "recovered from crash, no checkpoint"
```

### Advantages Over SIGTERM

| Aspect | SIGTERM (original) | File-based (v2) |
|--------|-------------------|-----------------|
| Agent support | Requires signal handler | Only needs file check |
| Platform | Unix only | Cross-platform |
| Interruption safety | May interrupt mid-write | Agent chooses safe point |
| Communication model | Out-of-band (signal) | In-band (file system) |
| Debuggability | Signal is invisible | Marker file is inspectable |
| Testability | Hard to test signals | Easy to test file presence |

### Implementation Notes

- `pause-requested` is a zero-byte marker file (presence = signal, no content needed)
- `checkpoint.md` uses the same YAML frontmatter + Markdown body format as other hive files
- `hive doctor` should warn about stale `pause-requested` markers (marker exists but task is not in_progress)
- Audit entry appended on both pause and resume transitions
- Worktree is preserved during paused state (not cleaned up)
