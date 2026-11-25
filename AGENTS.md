# weirdnet-chart agents

The collective `AGENTS.md` in `WeirdBrains/weirdnet-agents` is mandatory and wins on conflict.

- Build outside the source checkout, with `CARGO_TARGET_DIR` on a scratch disk; clean targets afterwards.
- The core (everything outside `src/backend/`) must not depend on GPUI, Ratatui or any other GUI crate.
  Backends only draw `Scene` primitives; layout decisions live in `scene.rs`.
- Never open a window in tests: Ratatui renders into `Buffer`/`TestBackend`, GPUI tests use the
  `gpui-test-support` test platform.
- Keep the crate generic: no app-, exchange- or market-specific code.
- `gpui` must come from crates.io (no git dependencies), so the crate stays publishable.
