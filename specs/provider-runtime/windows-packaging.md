# Build Windows packages

Run either pair from the repository root:

| Build | Batch entry point | Python entry point | Output |
| --- | --- | --- | --- |
| Portable | `build-portable.bat` | `python build-portable.py` | A ready-to-run folder and ZIP |
| Installed | `build-installer.bat` | `python build-installer.py` | A per-user Setup EXE with uninstaller and shortcuts |

Python is required on the build computer, not on computers running the packaged app.
The package includes OpenCode. Codex and Claude Code are separate installations.

## Requirements

- Windows x64 or ARM64, Python 3.11+, Rust from `rust-toolchain.toml`, and the repository's
  Windows build dependencies (`script/windows/bootstrap.ps1`, Visual Studio C++ tools,
  Protocol Buffers and LLVM). Download Git LFS assets with `git lfs pull`.
- `cargo-about` on PATH: `cargo install cargo-about --locked`.
- A native `opencode.exe` for the selected architecture. Pass `--opencode` when it is not on
  PATH. Npm's `.cmd` wrapper cannot be bundled in place of the real executable.
- OpenCode's license beside the executable or in its parent package folder; otherwise use
  `--opencode-license PATH`.
- Installer builds also require [Inno Setup 6.3+](https://jrsoftware.org/isinfo.php).
  Use `--iscc PATH` if `ISCC.exe` is not found automatically. A portable Inno Setup compiler
  works too. The scripts do not install build tools or change system settings.

## Examples

```bat
build-portable.bat --opencode "C:\Tools\opencode\bin\opencode.exe"
build-installer.bat --opencode "C:\Tools\opencode\bin\opencode.exe" --iscc "C:\Tools\Inno Setup 6\ISCC.exe"
```

Release builds are the default. `--profile dev` creates a faster development build with
assets embedded so the resulting executable can leave the source tree. Cargo's configured
target directory is respected. `--arch x64` or `--arch arm64` selects an explicit MSVC target.
For an ARM64 build on x64, provide a compatible `--settings-schema PATH`.

To package an existing binary, built with `release_bundle,gui`, and already prepared resources:

```bat
build-portable.bat --skip-build --binary "C:\Build\warp-oss.exe" --resources-dir "C:\Build\resources" --opencode "C:\Tools\opencode\bin\opencode.exe"
```

The same flags work with `build-installer.bat`. Prepared resources must include the settings
schema, bundled resources, and `THIRD_PARTY_LICENSES.txt`; the normal build prepares them with
the existing Windows resource script. `--version 0.1.1` changes artifact names and the installer
version. `--output-dir PATH` selects a different output folder. Existing outputs are never
overwritten, including a portable profile used after an earlier build.

## Storage

| Content | Portable | Installed |
| --- | --- | --- |
| Program | Extracted package folder | `%LOCALAPPDATA%\Programs\MyWarp` by default |
| Profile | `profile` beside `MyWarp.exe` | `%LOCALAPPDATA%\MyWarp` |
| Warp settings | `config\settings.toml` in the profile | Same profile-relative path |
| Database, logs, protected credentials | `state` in the profile | Same profile-relative path |
| Themes and workflows | `data` in the profile | Same profile-relative path |
| Cache | `cache` in the profile | Same profile-relative path |
| OpenCode config | `providers\config\opencode\opencode.jsonc` in the profile | Same profile-relative path |
| OpenCode saved credentials | `providers\data\opencode\auth.json` in the profile | Same profile-relative path |

The empty `mywarp-portable` / `mywarp-installed` marker determines the mode. Directly launching
`MyWarp.exe` works; the batch launcher is optional. Keep the marker beside the executable.
Unpackaged development builds retain upstream storage paths. Windows instance mutexes and URI
pipes are separated by package profile, so installed and portable builds can run independently.

Both package modes isolate OpenCode configuration from the build computer's environment/config
overrides. Provider API-key environment variables remain supported. The native chat still forces
attended `permission: ask`. To connect, run the packaged `OpenCode.bat` and use `/connect`, or
copy the OpenRouter example from `resources\provider-config` into the profile's OpenCode config
folder and save the key as `keys\openrouter.key`. Keys, logins and history are never copied into
packages from the build computer. Existing provider configuration is preserved on startup.

The installer needs no administrator rights and preserves the user profile when uninstalled.
It refuses to install into a portable folder. Generated packages are unsigned unless signed
separately. Windows-protected Warp credentials may need a new login on another Windows account
or computer; keeping files in the portable folder does not remove Windows encryption bindings.

## Checks

```text
python -m unittest discover -s script/windows -p test_package_mywarp.py -v
cargo test --locked -p warp_core --features local_fs windows_package
cargo test --locked -p warp_core --features local_fs paths::tests
```

Packaging tests use synthetic executable headers and temporary files; they do not claim a
successful desktop run. A full release build and manual desktop test are separate checks.
Validation was performed on Windows x64. ARM64 packages require matching native sidecars;
the current upstream `arm64/vcruntime140_1.dll` identifies as x64, so ARM64 packaging is rejected
until that asset is corrected. This does not affect the x64 build.
