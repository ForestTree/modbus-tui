# Contributing to modbus-tui

Thank you for your interest in contributing to modbus-tui! This document provides guidelines to help you get started.

## Getting Started

1. Fork the repository and clone your fork
2. Install the [Rust toolchain](https://rustup.rs/) (stable)
3. Build the project:
   ```sh
   cargo build
   ```

## Development Workflow

### Building

```sh
cargo build                # Dev build
cargo build --release      # Release build
```

### Code Quality

Before submitting a PR, make sure your code passes all checks:

```sh
cargo fmt --check          # Check formatting
cargo clippy --all-targets -- -D warnings   # Lint (zero warnings enforced)
cargo test --release       # Run tests
```

To auto-format your code:

```sh
cargo fmt
```

### CI

CI runs automatically on every push and pull request to `main`. It checks:

- Build across platforms (Linux musl, macOS x86/ARM, Windows)
- Clippy with `-D warnings` (no warnings allowed)
- Rustfmt formatting

Please make sure all checks pass before requesting a review.

## Submitting Changes

1. Create a new branch from `main` for your changes
2. Make your changes in focused, logical commits
3. Ensure all checks pass (`cargo fmt --check`, `cargo clippy`, `cargo test`)
4. Open a pull request against `main` with a clear description of what you changed and why

## Reporting Issues

If you find a bug or have a feature request, please [open an issue](https://github.com/ForestTree/modbus-tui/issues) with:

- A clear description of the problem or feature
- Steps to reproduce (for bugs)
- Expected vs actual behavior (for bugs)

## Code Conventions

- Rust edition 2024, stable toolchain
- No `unsafe` code
- Error handling via `anyhow` for main/config paths
- Use "unit" (not "slave") for Modbus unit ID terminology
- Register types: `HoldingRegisters`, `InputRegisters`, `Coils`, `DiscreteInputs`

## License

By contributing, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).
