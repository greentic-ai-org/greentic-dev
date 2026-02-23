# Wizard Architecture (PR-01)

## Overview

`greentic-dev wizard` is plan-first orchestration:

1. Resolve registry entry (`target.mode`)
2. Build deterministic plan via provider
3. Persist `answers.json` + `plan.json`
4. Dry-run: print plan JSON
5. Execute: confirm + run allowed command steps + append `exec.log`

## Modules

- `src/wizard/registry.rs`: supported PR-01 keys
- `src/wizard/provider.rs`: provider trait + shell provider
- `src/wizard/plan.rs`: `plan_version: 1` model and step types
- `src/wizard/persistence.rs`: output dir and replay load/store
- `src/wizard/confirm.rs`: interactive/non-interactive execution confirmation
- `src/wizard/executor.rs`: command allowlist enforcement + execution logging

## Determinism

- Plan has explicit `plan_version: 1`.
- Plan ordering is deterministic by construction (static registry mapping + ordered step vectors).
- Snapshot tests must ignore nondeterministic fields (timestamps/temp paths).
- Current baseline snapshot lives at `tests/snapshots/wizard_pack_build_plan.json`.

## Replay Semantics

- Replay consumes persisted `answers.json` and sibling `plan.json`.
- If plan has pinned resolved versions/digests, replay should use them.
- Floating refs may re-resolve during execute; behavior should be documented in user-facing docs.
- Phase-1 scaffolding stores optional provider references in `inputs.provider_refs.*` when supplied.
- Execute captures `resolved_versions.<program>` from `<program> --version` where available and persists them back to `plan.json`.
- Replay execution enforces pinned versions and fails on mismatch.

## Destructive Control

- `RunCommand` steps can mark `destructive: true`.
- Executor rejects destructive plans unless `--allow-destructive` is explicitly provided.
