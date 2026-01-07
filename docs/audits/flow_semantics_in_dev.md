# Flow Semantics Implemented in greentic-dev

## Flow-Related Commands and Codepaths

| Command / Path | Upstream intent | Extra logic in greentic-dev | Classification | Location |
| --- | --- | --- | --- | --- |
| `flow validate` | `greentic_flow::flow_bundle::load_and_validate_bundle` | Path-safety gate (`normalize_under_root`), reads file, serializes bundle to JSON (pretty/compact). | SAFETY POLICY / UX | `src/flow_cmd.rs::validate` |
| Manifest normalization | greentic-flow normalizes legacy manifests | Invokes `normalize_manifest_value` before deserializing `ComponentManifest` to tolerate `operations: ["echo"]`. | PASS-THROUGH (minimal) | `src/flow_cmd.rs::run_add_step` |
| `flow add-step` (overall) | `greentic_flow::add_step::add_step_from_config_flow` | Wrapper loads manifest/config flow, resolves component if remote, prompts for anchor (TTY) using `anchor_candidates`, builds catalog from the provided manifest path, forces `allow_cycles = false`, and writes the updated YAML returned by greentic-flow. | UX/safety surface + single-manifest catalog | `src/flow_cmd.rs::run_add_step` |
| Component bundle resolution | Should be caller-supplied | Fetches component via distributor when coordinate is remote (`run_component_add`) instead of requiring pre-resolved bundle. | UX (but changes failure surface) | `src/flow_cmd.rs::resolve_component_bundle` |
| Catalog building for add-step | Should come from greentic-flow | Builds `ManifestCatalog` only from the provided manifest path; required fields limited to that manifest even if pack has more components. | MUST MOVE (validation coverage) | `src/flow_cmd.rs::run_add_step` |
| Anchor selection | greentic-flow provides `anchor_candidates` | Interactive prompt when `--after` not provided; uses `anchor_candidates` ordering for UX. | MAY STAY (UX) | `src/flow_cmd.rs::prompt_routing_target` |

## Semantic Behaviors and Repro Snippets

- **Single-manifest catalog (MUST MOVE)**  
  Required-field validation uses only the manifest passed via `--manifest`; multi-component packs will miss required-field errors for other components. Repro: pack referencing component B without its manifest will not surface required-field diagnostics.

- **Allow-cycles hardcoded off (MUST MOVE)**  
  `allow_cycles` is always `false` when invoking greentic-flow add-step helpers; users cannot allow cycles through greentic-dev CLI today.

- **Component auto-resolve (behavioral)**  
  Non-local coordinates trigger `component add` resolution (with offline/stub behavior) before add-step runs. Failure surface differs from pure library use.

- **Interactive anchor selection (MAY STAY)**  
  When `--after` is omitted, greentic-dev prompts on TTY using greentic-flow’s `anchor_candidates` ordering; non-TTY skips prompting and lets greentic-flow default anchor apply.

- **Path safety on `flow validate` (SAFETY POLICY)**  
  Ensures the validated file lives under the current workspace root; rejects paths outside via `normalize_under_root`.

## Notes on Behavior Gaps vs greentic-flow

- Catalog used for add-step only knows about the single manifest passed; multi-component packs may miss validation of required fields for other nodes.  
- Users can still route add-step via interactive prompt, which is CLI-only behavior not represented in greentic-flow.  
- Component bundle resolution is automatic (downloads on demand) rather than requiring pre-existing bundle; upstream greentic-flow add-step is library-level and assumes callers have components in place.
