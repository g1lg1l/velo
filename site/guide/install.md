# Install

Velo runs on Windows and on macOS 14 (Sonoma) or later with Apple silicon. Every release is on the [releases page](https://github.com/g1lg1l/velo/releases/latest).

## Windows

1. Download the setup (`.exe`) from the latest release.
2. Run it. It installs for you alone, in your own programs folder, so it doesn't ask for an administrator.
3. Velo isn't signed yet. If SmartScreen stops the setup, click **More info**, then **Run anyway**.

The setup can also add a desktop icon and open `.velo` projects with Velo. A `.zip` with `velo.exe` and `velo-mcp.exe` is on the same page if you'd rather not install.

## macOS

1. Download the `.dmg` from the latest release (Apple silicon).
2. Open it and drag **Velo** into Applications.
3. The app is signed ad hoc, not notarized, so macOS stops it the first time. Open **System Settings → Privacy & Security** and click **Open Anyway** under the message about Velo.

The first time Velo listens to the computer's audio, macOS asks for permission to capture it. Allow it: without it, macOS hands Velo silence.

## Updates

Velo can check GitHub for a newer release once at start (it sends nothing else). When one is out, **Preferences → Updates** installs it: a copy from the releases replaces itself in place, and a copy built from the sources gets a link to the release page.

## Build from the sources

You need [Rust](https://rustup.rs), CMake and a C++ compiler (for Ableton Link), and on Windows also LLVM.

```bash
git clone https://github.com/g1lg1l/velo
cd velo
cargo dev
```

`cargo dev` is an alias for `cargo run -p velo`. To open a project from the command line, put it after `--`:

```bash
cargo dev -- show.velo
```

To build without Ableton Link (no CMake or C++ compiler needed):

```bash
cargo run -p velo --no-default-features
```

::: tip On Windows
If the build can't find libclang, point `LIBCLANG_PATH` at LLVM's `bin` folder, for example `C:\Program Files\LLVM\bin`.
:::

The libraries (`velo-core`, `velo-engine`, `velo-audio`, `velo-mcp`) are MIT or Apache-2.0. The app itself is GPL-3.0-or-later, because it ships with Ableton Link.
