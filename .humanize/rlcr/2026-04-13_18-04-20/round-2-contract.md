# Round 2 Contract

## Mainline Objective

Close the remaining `hive exec` scheduling gaps by making approved-task preflight deterministic and making scheduler outcomes converge without turning documented skips into generic stalls.

## Target ACs

- AC-8: `hive exec` orchestrates the full execution chain with deterministic preflight, skip, dependency, and retry behavior
- AC-10: `hive check --task <id>` remains part of the deterministic execution contract by keeping verification outcomes explicit in the scheduler flow

## Blocking Issues In Scope

- `hive exec` still suppresses malformed spec errors during dependency loading and can therefore bypass dependency/validation guarantees
- `hive exec` currently warns on missing plans but later collapses those skipped tasks into a generic `execution stalled` failure
- Downstream tasks whose dependencies end in `blocked`/non-runnable states are left non-terminal instead of converging deterministically
- Execution-path confidence is still blocked by missing fixture coverage for malformed spec preflight, missing-plan skip, blocked dependency fan-out, and retry/block convergence

## Queued Issues Out of Scope

- Skill parsing/injection and adapter generation completeness (AC-1, AC-9, AC-15, AC-19)
- RFC/approve/merge collaboration flow completeness (AC-12, AC-13)
- Audit/doctor completeness beyond what is already needed by the execution path (AC-14, AC-18)
- Analyze deliverables and broader fixture-harness work beyond the targeted `exec` regression cases for this round

## Round Success Criteria

1. Approved-task preflight fails immediately and clearly when a spec cannot be parsed or validated for dependency resolution
2. Approved tasks missing `plan.md` are reported as skipped and do not cause the round to end in a misleading `execution stalled` error
3. Tasks blocked forever by upstream `blocked`/failed convergence are moved to deterministic terminal state instead of lingering pending
4. Binary/integration tests cover malformed spec preflight failure, missing-plan skip, blocked dependency fan-out, and retry/block convergence
5. Targeted `cargo test` coverage passes with the repaired scheduler behavior
