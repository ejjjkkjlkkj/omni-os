# Contributing

## Current stage

The project is in bootstrap. Changes should keep the workspace buildable and should prefer small, auditable interfaces over speculative large subsystems.

## Required checks

Run before proposing a change:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Accessibility requirement

New native UI abstractions must define their semantic role, name/value/state contract, focus behavior and emitted accessibility events before visual styling is considered complete.

## Source provenance

Do not submit code originating from leaked or unauthorized proprietary operating-system sources. If a contribution derives from an external open-source project, document the source repository, revision and license in the pull request.
