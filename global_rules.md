# Global Rules for greentic-dev

- greentic-dev is a pass-through CLI: prefer delegating semantics to upstream crates (`greentic-flow`, `greentic-component`, `greentic-distributor-client`, `greentic-pack`, `greentic-runner`). Do not add new flow/component semantics here unless explicitly required.
- When upstream provides functionality, remove local duplicates and call upstream APIs instead of re-implementing.
- If tests fail, first determine whether the failure points to an upstream bug or a faulty test. Treat exposing upstream bugs as the primary objective.
- UX-only behavior (logging, prompts) and explicit safety policies (path safety, offline guards) may remain, but must not change underlying semantics.
- Keep `docs/developer-guide.md` examples synchronized with automated tests; when the guide changes, update or add tests that cover the documented steps.
