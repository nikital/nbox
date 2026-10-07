# Testing philosophy

Tests are end-to-end only — they exercise the public API and verify observable
behaviour against real podman, not internal state.  Everything lives in a single
`#[test]` function per test file to sidestep `cargo test` parallelism issues
(shared podman state, image store contention).  Helpers are minimal and live in
the same file.  No mocking, no unit tests for plumbing — if it can't be observed
from the outside, it doesn't need a test.
