# Wizard Architecture (Launcher-Only)

## Overview

`greentic-dev wizard` is a launcher-first flow:

1. Collect launcher selection (`pack` or `bundle`) via interactive prompt, or load AnswerDocument.
2. Build deterministic launcher plan (`launcher.main`).
3. Persist `answers.json` + `plan.json`.
4. Dry-run: render plan only.
5. Apply: confirm and execute delegated command; append `exec.log`.

## Modules

- `src/wizard/registry.rs`: launcher registration (`launcher.main`)
- `src/wizard/provider.rs`: provider trait + shell launcher provider
- `src/wizard/plan.rs`: `plan_version: 1` model and step types
- `src/wizard/persistence.rs`: output dir and plan/answers persistence
- `src/wizard/confirm.rs`: interactive/non-interactive execute confirmation
- `src/wizard/executor.rs`: allowlist enforcement + command execution logging

## Delegation

- `selected_action = pack` -> `greentic-pack wizard`
- `selected_action = bundle` -> `greentic-bundle wizard`
- if `answers.delegate_answer_document` is present, persist it under the launcher output dir and delegate through `wizard apply --answers <persisted-file>`

## AnswerDocument Rules

Launcher identity is accepted directly:

- `wizard_id = greentic-dev.wizard.launcher.main`
- `schema_id = greentic-dev.launcher.main`

Top-level bundle and pack AnswerDocuments may also be passed to `greentic-dev wizard --answers <FILE>`.
They are normalized into launcher answers with `selected_action` plus `delegate_answer_document`.

Other non-launcher IDs are rejected.

## Determinism

- Plan has explicit `plan_version: 1`.
- Step ordering is deterministic by construction.
- Default output dir includes time-based run id; use `--out` for fixed paths.

## Destructive Control

- `RunCommand` steps may mark `destructive: true`.
- Executor rejects destructive plans unless `--allow-destructive` is set.
