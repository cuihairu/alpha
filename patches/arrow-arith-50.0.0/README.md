This is a local override for the `arrow-arith` crate used by the workspace.

Why:
- `arrow-arith` v50.0.0 fails to compile with newer `chrono` due to an ambiguous `quarter()` method call.

What changed:
- `src/temporal.rs`: disambiguate `quarter()` by calling `ChronoDateExt::quarter(&t)` explicitly.

This override is wired via `[patch.crates-io]` in the workspace `Cargo.toml`.
