# retrosim-core

<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Howard M. Harte -->

Bus-agnostic traits and shared types for retro system simulators. CPU,
peripheral, debugger, and transport implementations use these interfaces to
interoperate without depending on a particular system bus.

## Documentation

| Document | Contents |
|---|---|
| [Design](doc/retrosim-core-guide.md) | Module contracts, simulator integration, and verification scope. |
| [Documentation conventions](https://github.com/s100box/retrosim-build/blob/main/doc/documentation-style.md) | Shared release documentation format. |

Generate the complete API reference with `cargo doc --no-deps --open`.

## Repository layout

| Path | Contents |
|---|---|
| `src/` | Public interfaces, helpers, and unit tests. |
| `doc/` | Design and integration reference. |
| `.githooks/` | Local format, lint, documentation, and test gate. |
| `.github/workflows/` | Stable Rust CI. |

## Requirements

Use stable Rust with `rustfmt` and `clippy`. `Cargo.toml` declares Rust 1.74.1
as the minimum; CI exercises stable Rust. No sibling checkout is required
to build or test this crate. The documentation-conventions link is optional.

## Getting started

From the repository root:

```bash
rustup component add rustfmt clippy
cargo check --all-targets
git config core.hooksPath .githooks
```

For a consumer checked out alongside this repository, add:

```toml
[dependencies]
retrosim-core = { path = "../retrosim-core" }
```

The [integration section](doc/retrosim-core-guide.md#10-building-a-new-simulator)
explains how a simulator implements the interfaces.

## Verification

From the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --workspace --no-deps
```

The pre-commit hook runs these gates with documentation warnings treated as
errors. CI runs checks, tests, Clippy, documentation, and formatting on stable
Rust. Successful commands exit with status zero.

For coverage, install `llvm-tools-preview` and `cargo-llvm-cov`, then run
`cargo llvm-cov --summary-only` or `cargo llvm-cov --html --open`. Coverage
figures belong in generated reports; the [design](doc/retrosim-core-guide.md#12-verification)
describes the test scope and clock-injection rule.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.

### Contributions

Unless explicitly stated otherwise, contributions submitted for inclusion
are licensed under the same terms, without additional conditions.
