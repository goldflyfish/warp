"""Build clean Windows distributions; user profiles are never packaging inputs."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile


REPOSITORY = Path(__file__).resolve().parents[2]
MARKERS = {"portable": "mywarp-portable", "installer": "mywarp-installed"}
MACHINES = {"x64": 0x8664, "arm64": 0xAA64}
TARGETS = {"x64": "x86_64-pc-windows-msvc", "arm64": "aarch64-pc-windows-msvc"}
DLLS = ("conpty.dll", "dxcompiler.dll", "dxil.dll", "vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll")
DEFAULT_CONFIG = {"$schema": "https://opencode.ai/config.json"}
OPENROUTER_EXAMPLE = {
    **DEFAULT_CONFIG,
    "provider": {"openrouter": {"options": {"apiKey": "{file:./keys/openrouter.key}"}}},
}


class PackagingError(Exception):
    pass


def run(arguments: list[str | Path], *, repo: Path, env: dict[str, str] | None = None) -> None:
    subprocess.run([str(argument) for argument in arguments], cwd=repo, env=env, check=True)


def require_file(path: Path) -> Path:
    if not path.is_file() or path.stat().st_size == 0:
        raise PackagingError(f"Required file is missing or empty: {path}")
    with path.open("rb") as file:
        if file.read(43).startswith(b"version https://git-lfs.github.com/spec/"):
            raise PackagingError(f"Git LFS content has not been downloaded: {path}. Run git lfs pull first.")
    return path


def require_pe(path: Path, arch: str) -> Path:
    require_file(path)
    with path.open("rb") as file:
        header = file.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise PackagingError(f"Not a Windows executable or DLL: {path}")
        file.seek(struct.unpack_from("<I", header, 60)[0])
        signature = file.read(6)
    if len(signature) != 6 or signature[:4] != b"PE\0\0" or struct.unpack_from("<H", signature, 4)[0] != MACHINES[arch]:
        raise PackagingError(f"Wrong or invalid executable architecture; expected {arch}: {path}")
    return path


def copy_file(source: Path, destination: Path) -> None:
    require_file(source)
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def write_text(path: Path, text: str, *, batch: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\r\n" if batch else "\n")


@contextlib.contextmanager
def staging_directory(output: Path):
    output.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=".mywarp-package-", dir=output)).resolve()
    try:
        yield stage
    finally:
        if not stage.is_relative_to(output.resolve()) or stage == output.resolve() or not stage.name.startswith(".mywarp-package-"):
            raise PackagingError("Refusing to remove an unverified staging directory.")
        shutil.rmtree(stage)


def host_arch() -> str:
    machine = os.environ.get("PROCESSOR_ARCHITEW6432", platform.machine()).lower()
    if machine in ("amd64", "x86_64"):
        return "x64"
    if machine in ("arm64", "aarch64"):
        return "arm64"
    raise PackagingError(f"Unsupported Windows architecture: {machine}")


def find_opencode(explicit: Path | None, arch: str) -> Path:
    candidate = explicit or (Path(shutil.which("opencode.exe")) if shutil.which("opencode.exe") else None)
    if candidate is None:
        raise PackagingError("OpenCode's native executable is required. Pass --opencode C:\\path\\to\\opencode.exe. A .cmd npm shim is not a portable runtime.")
    return require_pe(candidate.resolve(), arch)


def find_opencode_license(explicit: Path | None, executable: Path) -> Path:
    if explicit is not None:
        return require_file(explicit.resolve())
    for directory in (executable.parent, executable.parent.parent):
        for name in ("LICENSE", "LICENSE.md", "LICENSE.txt"):
            candidate = directory / name
            if candidate.is_file():
                return require_file(candidate)
    raise PackagingError("OpenCode's license must accompany its executable. Pass --opencode-license C:\\path\\to\\LICENSE.")


def find_iscc(explicit: Path | None) -> Path:
    if explicit is not None:
        return require_file(explicit.resolve())
    found = shutil.which("ISCC.exe")
    if found:
        return Path(found)
    for variable in ("ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"):
        base = os.environ.get(variable)
        if base:
            for suffix in ("Inno Setup 6/ISCC.exe", "Inno Setup 7/ISCC.exe", "Programs/Inno Setup 6/ISCC.exe", "Programs/Inno Setup 7/ISCC.exe"):
                candidate = Path(base) / suffix
                if candidate.is_file():
                    return candidate
    raise PackagingError("Inno Setup 6.3+ is required for the installer. Install it or pass --iscc C:\\path\\to\\ISCC.exe.")


def build_binary(args: argparse.Namespace, arch: str) -> Path:
    if args.skip_build:
        if args.binary is None or args.resources_dir is None:
            raise PackagingError("--skip-build requires both --binary and --resources-dir from a build with release_bundle enabled.")
        return require_pe(args.binary.resolve(), arch)
    if args.binary is not None or args.resources_dir is not None:
        raise PackagingError("--binary and --resources-dir require --skip-build.")
    if not shutil.which("cargo"):
        raise PackagingError("Rust/Cargo is unavailable. Run script\\windows\\bootstrap.ps1 first.")
    if shutil.which("cargo-about") is None:
        raise PackagingError("cargo-about is required for bundled attributions. Install it with: cargo install cargo-about --locked")
    environment = os.environ.copy()
    environment.update(CARGO_BIN_NAME="oss", WARP_APP_NAME="MyWarp", GIT_RELEASE_TAG=args.version, CARGO_FULL_PROFILE="debug" if args.profile == "dev" else "release")
    profile_dir = "debug" if args.profile == "dev" else "release"
    # app/build.rs still copies Windows DLLs through its repository-local target path.
    (args.repo / "target" / profile_dir).mkdir(parents=True, exist_ok=True)
    command = ["cargo", "build", "--locked", "-p", "warp", "--bin", "warp-oss", "--profile", args.profile, "--features", "release_bundle,gui"]
    target = os.environ.get("CARGO_BUILD_TARGET")
    if args.arch is not None:
        target = TARGETS[arch]
        command.extend(["--target", target])
    if target:
        (args.repo / "target" / target / profile_dir).mkdir(parents=True, exist_ok=True)
    run(command, repo=args.repo, env=environment)
    metadata = subprocess.check_output(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=args.repo, env=environment, text=True)
    target_dir = Path(json.loads(metadata)["target_directory"])
    if target:
        target_dir /= target
    binary = target_dir / profile_dir / "warp-oss.exe"
    return require_pe(binary, arch)


def prepare_resources(args: argparse.Namespace, binary: Path, destination: Path) -> None:
    if args.skip_build:
        source = args.resources_dir.resolve()
        if not source.is_dir():
            raise PackagingError(f"Prepared resources directory is missing: {source}")
        require_file(source / "THIRD_PARTY_LICENSES.txt")
        require_file(source / "settings_schema.json")
        shutil.copytree(source, destination, symlinks=True)
    else:
        environment = os.environ.copy()
        environment.pop("SKIP_SETTINGS_SCHEMA", None)
        environment["GIT_RELEASE_TAG"] = args.version
        if args.settings_schema:
            environment.pop("SETTINGS_SCHEMA_EXECUTABLE", None)
            environment["SETTINGS_SCHEMA_SOURCE"] = str(require_file(args.settings_schema.resolve()))
        else:
            if host_arch() == "x64" and args.arch == "arm64":
                raise PackagingError("Cross-building ARM64 requires --settings-schema from a compatible build.")
            environment.pop("SETTINGS_SCHEMA_SOURCE", None)
            environment["SETTINGS_SCHEMA_EXECUTABLE"] = str(binary)
        powershell = shutil.which("pwsh") or shutil.which("powershell")
        if powershell is None:
            raise PackagingError("PowerShell is required to prepare bundled resources.")
        run([powershell, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", args.repo / "script/windows/prepare_bundled_resources.ps1", "-DestinationDir", destination, "-Channel", "oss"], repo=args.repo, env=environment)
    require_file(destination / "THIRD_PARTY_LICENSES.txt")
    require_file(destination / "settings_schema.json")
    if not isinstance(json.loads((destination / "settings_schema.json").read_text(encoding="utf-8-sig")), dict):
        raise PackagingError("The prepared settings schema must be a JSON object.")
    if not (destination / "bundled").is_dir() or not any((destination / "bundled").rglob("*")):
        raise PackagingError("The prepared resources must include the nonempty bundled directory.")


def validate_assets(repo: Path, arch: str) -> None:
    assets = repo / "app/assets/windows" / arch
    for name in DLLS + ("OpenConsole.exe",):
        require_pe(assets / name, arch)
    require_file(repo / "app/assets/bundled/bootstrap/pwsh.ps1")
    require_file(repo / "app/channels/oss/icon/no-padding/icon.ico")


def opencode_wrapper() -> str:
    return r'''@echo off
setlocal
for %%I in ("%~dp0..\..") do set "MYWARP_PACKAGE=%%~fI"
if exist "%MYWARP_PACKAGE%\mywarp-portable" (
  set "MYWARP_PROFILE=%MYWARP_PACKAGE%\profile"
) else (
  if defined MYWARP_PROFILE_DIR (set "MYWARP_PROFILE=%MYWARP_PROFILE_DIR%") else (set "MYWARP_PROFILE=%LOCALAPPDATA%\MyWarp")
)
set "XDG_CONFIG_HOME=%MYWARP_PROFILE%\providers\config"
set "XDG_DATA_HOME=%MYWARP_PROFILE%\providers\data"
set "XDG_CACHE_HOME=%MYWARP_PROFILE%\providers\cache"
set "XDG_STATE_HOME=%MYWARP_PROFILE%\providers\state"
set "OPENCODE_CONFIG_DIR=%XDG_CONFIG_HOME%\opencode"
set "OPENCODE_CONFIG=%OPENCODE_CONFIG_DIR%\opencode.jsonc"
set "OPENCODE_CONFIG_CONTENT="
if not exist "%OPENCODE_CONFIG_DIR%\keys" mkdir "%OPENCODE_CONFIG_DIR%\keys"
if errorlevel 1 exit /b 1
if not exist "%OPENCODE_CONFIG%" copy /y "%MYWARP_PACKAGE%\resources\provider-config\opencode.jsonc" "%OPENCODE_CONFIG%" >nul
if errorlevel 1 exit /b 1
"%MYWARP_PACKAGE%\tools\opencode\opencode.exe" %*
exit /b %errorlevel%
'''


def write_provider_templates(destination: Path) -> None:
    write_text(destination / "opencode.jsonc", json.dumps(DEFAULT_CONFIG, indent=2) + "\n")
    write_text(destination / "openrouter.example.jsonc", json.dumps(OPENROUTER_EXAMPLE, indent=2) + "\n")
    write_text(destination / "keys/README.txt", "For OpenRouter, copy openrouter.example.jsonc to opencode.jsonc and save only your API key in keys/openrouter.key. Alternatively run OpenCode.bat and use /connect. No credentials are included in this package.\n")


def stage_payload(args: argparse.Namespace, mode: str, arch: str, binary: Path, opencode: Path, destination: Path) -> None:
    destination.mkdir()
    copy_file(binary, destination / "MyWarp.exe")
    assets = args.repo / "app/assets/windows" / arch
    for name in DLLS:
        copy_file(require_pe(assets / name, arch), destination / name)
    copy_file(require_pe(assets / "OpenConsole.exe", arch), destination / arch / "OpenConsole.exe")
    copy_file(args.repo / "app/assets/bundled/bootstrap/pwsh.ps1", destination / "pwsh.ps1")
    copy_file(args.repo / "app/channels/oss/icon/no-padding/icon.ico", destination / "icon.ico")
    for name in ("LICENSE-AGPL", "LICENSE-MIT"):
        copy_file(args.repo / name, destination / name)
    copy_file(opencode, destination / "tools/opencode/opencode.exe")
    copy_file(find_opencode_license(args.opencode_license, opencode), destination / "tools/opencode/LICENSE.txt")
    prepare_resources(args, binary, destination / "resources")
    write_provider_templates(destination / "resources/provider-config")
    write_text(destination / MARKERS[mode], "")
    write_text(destination / "tools/bin/opencode.cmd", opencode_wrapper(), batch=True)
    write_text(destination / "OpenCode.bat", '@echo off\r\ncall "%~dp0tools\\bin\\opencode.cmd" %*\r\nexit /b %errorlevel%\r\n')
    write_text(destination / "MyWarp.bat", '@echo off\r\nstart "" "%~dp0MyWarp.exe" %*\r\n')
    if mode == "portable":
        write_provider_templates(destination / "profile/providers/config/opencode")
    instructions = (
        "MyWarp Windows package\n\n"
        "Start MyWarp.exe (or MyWarp.bat). Python is only needed to build packages.\n"
        + ("Configuration and data live in the profile folder beside MyWarp.exe. Keep the mywarp-portable marker.\n" if mode == "portable" else "Configuration and data live in %LOCALAPPDATA%\\MyWarp. Keep the mywarp-installed marker.\n")
        + "Warp settings: config\\settings.toml under that profile.\n"
        "OpenCode settings: providers\\config\\opencode\\opencode.jsonc under that profile.\n"
        "Use OpenCode.bat and /connect for provider setup, or the OpenRouter key-file example.\n"
        "Fresh packages include no API keys, logins, chat history, or settings from the build computer.\n"
        "OpenCode is bundled; Codex and Claude Code remain separately installed tools with their own logins.\n"
        "Windows-protected Warp credentials may require signing in again on another computer/account.\n"
    )
    write_text(destination / "README.txt", instructions)


def write_manifest(destination: Path, args: argparse.Namespace, mode: str, arch: str) -> dict:
    files = {}
    for path in sorted(destination.rglob("*")):
        if path.is_symlink():
            raise PackagingError(f"Symbolic links are not accepted in the payload: {path}")
        if path.is_file():
            with path.open("rb") as file:
                digest = hashlib.file_digest(file, "sha256").hexdigest()
            files[path.relative_to(destination).as_posix()] = digest
    try:
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.repo, stderr=subprocess.DEVNULL, text=True).strip()
    except (OSError, subprocess.CalledProcessError):
        revision = "unknown"
    manifest = {"name": "MyWarp", "version": args.version, "architecture": arch, "mode": mode, "revision": revision, "profile": "profile" if mode == "portable" else "%LOCALAPPDATA%\\MyWarp", "files": files}
    write_text(destination / "package-manifest.json", json.dumps(manifest, indent=2) + "\n")
    return manifest


def make_zip(source: Path, destination: Path) -> None:
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for path in sorted(source.rglob("*")):
            if path.is_file():
                archive.write(path, Path(source.name) / path.relative_to(source))


def main(mode: str, argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=f"Build a MyWarp Windows {mode} distribution.")
    parser.add_argument("--repo", type=Path, default=REPOSITORY)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--version", default="0.1.0")
    parser.add_argument("--profile", choices=("release", "dev"), default="release")
    parser.add_argument("--arch", choices=tuple(MACHINES))
    parser.add_argument("--opencode", type=Path, help="Native opencode.exe, not an npm .cmd shim")
    parser.add_argument("--opencode-license", type=Path, help="Defaults to LICENSE beside OpenCode or in its package directory")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--resources-dir", type=Path)
    parser.add_argument("--settings-schema", type=Path)
    if mode == "installer":
        parser.add_argument("--iscc", type=Path)
    args = parser.parse_args(argv)
    try:
        if sys.version_info < (3, 11):
            raise PackagingError("Python 3.11 or newer is required.")
        if sys.platform != "win32":
            raise PackagingError("These packaging scripts run on Windows.")
        if re.fullmatch(r"\d+\.\d+\.\d+(?:\.\d+)?", args.version) is None or any(int(part) > 65535 for part in args.version.split(".")):
            raise PackagingError("--version must contain three or four numeric parts, each no larger than 65535.")
        args.repo = args.repo.resolve()
        require_file(args.repo / "Cargo.toml")
        output = (args.output_dir or args.repo / "dist").resolve()
        arch = args.arch or host_arch()
        name = f"MyWarp-{args.version}-{arch}"
        artifact = output / (f"{name}-portable.zip" if mode == "portable" else f"{name}-Setup.exe")
        folder = output / f"{name}-portable"
        receipt = output / f"{name}-{mode}-manifest.json"
        for path in [artifact, receipt] + ([folder] if mode == "portable" else []):
            if path.exists():
                raise PackagingError(f"Output already exists and was left untouched: {path}. Choose another --output-dir or --version.")
        opencode = find_opencode(args.opencode, arch)
        find_opencode_license(args.opencode_license, opencode)
        iscc = find_iscc(args.iscc) if mode == "installer" else None
        validate_assets(args.repo, arch)
        binary = build_binary(args, arch)
        with staging_directory(output) as stage:
            payload = stage / (folder.name if mode == "portable" else "payload")
            stage_payload(args, mode, arch, binary, opencode, payload)
            manifest = write_manifest(payload, args, mode, arch)
            if mode == "portable":
                temporary_artifact = stage / artifact.name
                make_zip(payload, temporary_artifact)
                payload.rename(folder)
                temporary_artifact.rename(artifact)
            else:
                run([iscc, "/Qp", f"/DPayloadDir={payload}", f"/DPackageVersion={args.version}", f"/DPackageArch={arch}", f"/O{stage}", f"/F{artifact.stem}", args.repo / "script/windows/mywarp-installer.iss"], repo=args.repo)
                require_file(stage / artifact.name).rename(artifact)
            write_text(receipt, json.dumps(manifest, indent=2) + "\n")
        print(f"Created: {artifact}")
        if mode == "portable":
            print(f"Ready to run: {folder / 'MyWarp.exe'}")
        print(f"Profile: {manifest['profile']}")
        return 0
    except (PackagingError, OSError, subprocess.CalledProcessError, ValueError) as error:
        print(f"Packaging failed: {error}", file=sys.stderr)
        return 1
