## What changes

<!-- What this pull request does and why, in a paragraph. Name the issue or the ADR it
     follows from, if there is one. -->

## World semantics

Does this touch `configs/**`, `crates/liminis-core/src/kernels/**`, or the order in
which processes run in a tick? Tick one:

- [ ] No.
- [ ] Yes, and `WORLD_FORMAT_VERSION` in `crates/liminis-core/src/version.rs` moved.
- [ ] Yes, but the change genuinely cannot affect dynamics. Why:

<!-- The `world semantics version (ADR-020)` job fails a pull request that changes a
     config or a kernel without moving the constant, and asks for the reason to be
     recorded here rather than in a review comment that nobody will find later.
     Results produced under different values do not belong on the same plot. -->

## Checks

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `scripts/test-hooks.sh`

<!-- If a reference under tests/golden/ moved, name it and say why the new answer is
     the correct one. A new environment process needs a golden test against an
     analytical solution — see CONTRIBUTING.md. -->
