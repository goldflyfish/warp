# Build Windows packages

Run either pair from the repository root:

| Build | Batch entry point | Python entry point | Output |
| --- | --- | --- | --- |
| Portable | `build-portable.bat` | `python build-portable.py` | Ready-to-run folder, ZIP and manifest in `Packages\Portable` |
| Installed | `build-installer.bat` | `python build-installer.py` | Setup EXE and manifest in `Packages\Installer` |

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
  works too. Missing `cargo-about` is installed automatically into `.build-tools` using a pinned
  version. Other tools must be installed or supplied in the local build settings. The scripts
  do not change system settings.

## Run without arguments

Double-click `build-portable.bat` or `build-installer.bat`. Each script compiles the OSS app
with `release_bundle,gui`, generates licenses and the settings schema by running the built
application, then produces the requested distribution. The second script reuses Cargo's build
cache. The first release compilation can take a long time; subsequent builds are incremental.

Build tools on PATH and standard install locations are detected. For tools installed elsewhere,
copy `windows-build.example.json` to `windows-build.local.json` in the repository root and edit
the paths. This ignored, machine-local file applies only to the build process; it is never
included in packages. Remove entries for tools already available through PATH.

Supported settings: `cargo_home`, `rustup_home`, `protoc`, `protoc_include`, `libclang_path`,
`target_dir`, `opencode`, `opencode_license`, `iscc`, and `build_jobs`. Relative paths resolve
beside the settings file. Explicit command-line tool paths override the local file.
Use `--build-config PATH` for another build settings file. These settings are separate from
the application's provider config and never need API keys.

The console shows five numbered stages, colors in supported terminals, elapsed time and final
artifact paths. The external manifest records the archive/setup SHA-256 and size. Complete compiler output and an HTML build report are saved under `Packages\Logs`.
Failures return a nonzero exit code and show the compiler's final diagnostic lines. Double-clicked
batch windows stay open; Python and noninteractive runs exit normally. `--plain` disables colors,
`--pause` keeps an interactive console open explicitly, and `--no-pause` makes a batch invocation
exit immediately. `NO_COLOR` is also respected.

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
version. The default output folders are `Packages\Portable` and `Packages\Installer` under the
repository root, regardless of the directory from which the script is launched.
`--output-dir PATH` explicitly overrides that default. Existing outputs are kept, including a
portable profile used after an earlier build. Rebuilding the same version automatically adds
`-build2`, `-build3`, and so on to the new package names.

## Storage

| Content | Portable | Installed |
| --- | --- | --- |
| Program | Extracted package folder | `%LOCALAPPDATA%\Programs\MyWarp` by default |
| Configuration files | `Configurations` beside `MyWarp.exe` | `%LOCALAPPDATA%\MyWarp\Configurations` |
| Generated runtime files | `Runtime` beside `MyWarp.exe` | `%LOCALAPPDATA%\MyWarp\Runtime` |
| Settings and execution profiles | `Configurations\settings.toml` | Same path under `%LOCALAPPDATA%\MyWarp` |
| Themes and workflows | `Configurations\themes` and `Configurations\workflows` | Same paths under `%LOCALAPPDATA%\MyWarp` |
| Provider/model/MCP settings | `Configurations\providers\opencode.jsonc` | Same path under `%LOCALAPPDATA%\MyWarp` |
| API-key files | `Configurations\providers\keys` | Same path under `%LOCALAPPDATA%\MyWarp` |
| Database and history | `Runtime\state` and `Runtime\providers\data` | Same paths under `%LOCALAPPDATA%\MyWarp` |
| Caches, logs, installed tools | `Runtime` subfolders | Same paths under `%LOCALAPPDATA%\MyWarp` |

The empty `mywarp-portable` / `mywarp-installed` marker determines the mode. Directly launching
`MyWarp.exe` works; the batch launcher is optional. Keep the marker beside the executable.
Unpackaged development builds retain upstream storage paths. Windows instance mutexes and URI
pipes are separated by package profile, so installed and portable builds can run independently.

To back up or transfer configuration, close MyWarp and OpenCode and copy the entire
`Configurations` folder, including its subfolders. Paste it beside the portable executable or
under `%LOCALAPPDATA%\MyWarp` for an installed copy. No runtime directory or database is needed
to load the copied settings. Fresh packages ship actual settings and provider configuration files.
Both the GUI and `OpenCode.bat` split older combined profiles into declarative configuration and
runtime data. Copied configuration takes precedence; replaced legacy data is retained under Runtime.
Provider subscription sign-in sessions remain runtime data and may require a new sign-in.

Both package modes isolate OpenCode configuration from the build computer's environment/config
overrides. Provider API-key environment variables remain supported. The native chat still forces
attended `permission: ask`. To connect, run the packaged `OpenCode.bat` and use `/connect`, or
copy the OpenRouter example from `resources\provider-config` into `Configurations\providers`
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
