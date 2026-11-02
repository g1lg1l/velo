# Contributing to Velo

Bug reports, ideas and pull requests are welcome. Start with [ARCHITECTURE.md](ARCHITECTURE.md) and the docs it links.

## Before you code

1. **Open an issue first** for anything that changes behavior, adds a dependency, or touches a decision in the [decision log](ARCHITECTURE.md#decision-log).
2. **Update the docs with the code.** A change to a decision updates the relevant doc and the decision log in the same PR.
3. **Small PRs**, one concern each.

A new feature should fit the [non-goals](docs/product.md#3-what-it-is-what-it-is-not) and the [budgets](docs/product.md#5-budgets).

## Engineering rules

Velo runs on stage, so these are checked in review and, where possible, in CI.

**Realtime**
- The audio callback never allocates, locks, logs or makes syscalls.
- The render loop never allocates, blocks, compiles shaders or creates pipelines: that work happens on the compile thread.
- Nothing on the UI thread may be required for the next output frame.

**Architecture**
- `velo-core` has no GPU, audio or UI dependencies and is fully unit-tested.
- Every user action is an **operator** (id, props, `poll`, `exec`): that's what gives undo, keymaps, the command palette, MIDI and OSC bindings and the agents their reach.
- The project document is the source of truth; engine runtimes are disposable and rebuildable.

**UI**
- No color literals outside the theme module (CI checks it). Sizes that define the look (control height, type scale, radii, spacing) and durations are theme tokens; a widget's internal geometry may stay a named constant in the widget.
- Animations carry information and repaint only while they run; Reduce motion turns them off.

**Dependencies**
- The libraries are **MIT OR Apache-2.0**, the app is **GPL-3.0-or-later** (it ships Ableton Link). Only permissive dependencies (checked by `cargo-deny`), Link being the one exception. NDI's runtime is loaded at run time, never bundled.
- Every new dependency is justified in the PR: what it replaces, its size, its maintenance.

## Building

The app needs cmake and a C++ compiler (for Link).

- **Windows:** Link's bindings need libclang: install LLVM (`winget install LLVM.LLVM`), which bindgen finds in its default folder (else set `LIBCLANG_PATH`).
- **macOS:** `fatal error: 'cstddef' file not found` means an old Command Line Tools install left an incomplete `/Library/Developer/CommandLineTools/usr/include/c++`. Reinstall the Command Line Tools, or build with `CXXFLAGS="-nostdinc++ -isystem $(xcrun --show-sdk-path)/usr/include/c++/v1"`.
- **Linux:** install the ALSA headers (`libasound2-dev`).

`cargo run -p velo --no-default-features` builds without Link.

## Checks

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

The tests cover the project model and operators (`velo-core`), the drum model against fixtures (`velo-audio`), shaders and graphs rendered on the GPU (`velo-engine`), and the canvas and panels driven headless (`velo`).

## Style

- Rust 2024 edition, stable toolchain.
- Comments explain *why*; names explain *what*.
- Commit messages: one plain sentence saying what changed.

## License

By contributing you agree that your contributions to the libraries are dual-licensed under MIT OR Apache-2.0, and to the app (`crates/velo`) under GPL-3.0-or-later, without additional terms.
