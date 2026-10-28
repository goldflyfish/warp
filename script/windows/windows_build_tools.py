"""Process-local build settings and discovery; no system settings are changed."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil


ENVIRONMENT_KEYS = {
    "cargo_home": "CARGO_HOME", "rustup_home": "RUSTUP_HOME",
    "protoc": "PROTOC", "protoc_include": "PROTOC_INCLUDE",
    "libclang_path": "LIBCLANG_PATH", "target_dir": "CARGO_TARGET_DIR",
    "build_jobs": "CARGO_BUILD_JOBS",
}
TOOL_KEYS = ("opencode", "opencode_license", "iscc")


def configure(args):
    location = args.build_config or args.repo / "windows-build.local.json"
    settings = {}
    if location.is_file():
        settings = json.loads(location.read_text(encoding="utf-8-sig"))
        if not isinstance(settings, dict):
            raise ValueError(f"Build settings must be a JSON object: {location}")
        unknown = set(settings) - set(ENVIRONMENT_KEYS) - set(TOOL_KEYS)
        if unknown:
            raise ValueError(f"Unknown build settings in {location}: {', '.join(sorted(unknown))}")
        for key, value in settings.items():
            if not isinstance(value, (str, int)) or isinstance(value, bool):
                raise ValueError(f"Invalid build setting: {key}")
            if key == "build_jobs":
                if not str(value).isdigit() or int(value) < 1:
                    raise ValueError("build_jobs must be a positive integer")
                settings[key] = str(value)
            else:
                candidate = Path(os.path.expandvars(str(value))).expanduser()
                settings[key] = str((location.parent / candidate).resolve() if not candidate.is_absolute() else candidate)
        for key, variable in ENVIRONMENT_KEYS.items():
            if key in settings:
                os.environ[variable] = settings[key]
        for key in TOOL_KEYS:
            if getattr(args, key, None) is None and key in settings:
                setattr(args, key, Path(settings[key]))
    elif args.build_config:
        raise ValueError(f"Build settings file does not exist: {location}")

    cargo_home = Path(os.environ.get("CARGO_HOME") or Path.home() / ".cargo")
    additions = [cargo_home / "bin", args.repo / ".build-tools/bin"]
    os.environ["PATH"] = os.pathsep.join(str(path) for path in additions if path.is_dir()) + os.pathsep + os.environ.get("PATH", "")
    os.environ.setdefault("CARGO_BUILD_JOBS", "4")
    if not os.environ.get("PROTOC") and shutil.which("protoc"):
        os.environ["PROTOC"] = shutil.which("protoc")
    if not os.environ.get("LIBCLANG_PATH"):
        for variable in ("ProgramFiles", "ProgramFiles(x86)"):
            base = Path(os.environ.get(variable, r"C:\Program Files"))
            candidates = sorted(base.glob("Microsoft Visual Studio/*/*/VC/Tools/Llvm/x64/bin/libclang.dll"), reverse=True)
            if candidates:
                os.environ["LIBCLANG_PATH"] = str(candidates[0].parent)
                break

    if args.opencode is None:
        executable = shutil.which("opencode.exe")
        if executable:
            args.opencode = Path(executable)
        else:
            candidates = [args.repo / ".build-tools/opencode/opencode.exe"]
            for variable, suffix in (("APPDATA", "npm/node_modules/opencode-ai/bin/opencode.exe"), ("LOCALAPPDATA", "opencode/bin/opencode.exe")):
                if os.environ.get(variable):
                    candidates.append(Path(os.environ[variable]) / suffix)
            shim = shutil.which("opencode.cmd")
            if shim:
                candidates.append(Path(shim).parent / "node_modules/opencode-ai/bin/opencode.exe")
            args.opencode = next((path for path in candidates if path.is_file()), None)
    if getattr(args, "iscc", None) is None:
        local_iscc = args.repo / ".build-tools/inno/ISCC.exe"
        if local_iscc.is_file():
            args.iscc = local_iscc
    return location if location.is_file() else None
