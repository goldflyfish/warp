"""Console progress and a self-contained report for Windows package builds."""

from __future__ import annotations

import ctypes
from datetime import datetime
import html
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time
import os


class BuildUI:
    def __init__(self, mode: str, output: Path, *, plain: bool = False):
        self.mode = mode
        self.started = time.monotonic()
        self.steps = []
        self.current = None
        self.live = False
        self.last_progress = 0.0
        self.detail = "Starting"
        self.color = not plain and "NO_COLOR" not in os.environ and sys.stdout.isatty()
        if self.color and sys.platform == "win32":
            kernel = ctypes.windll.kernel32
            handle = kernel.GetStdHandle(-11)
            flags = ctypes.c_ulong()
            self.color = bool(kernel.GetConsoleMode(handle, ctypes.byref(flags)) and kernel.SetConsoleMode(handle, flags.value | 4))
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
        self.log_path = output.parent / "Logs" / f"{mode}-{stamp}.log"
        self.report_path = self.log_path.with_suffix(".html")
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        self.log = self.log_path.open("w", encoding="utf-8", buffering=1)

    def paint(self, message: str, color: str) -> str:
        return f"\033[{color}m{message}\033[0m" if self.color else message

    def clear_live(self):
        if self.live:
            print("\r\033[2K", end="", flush=True)
            self.live = False

    def line(self, message: str = "", color: str = "0"):
        self.clear_live()
        print(self.paint(message, color), flush=True)
        self.log.write(message + "\n")

    def banner(self, version: str, arch: str, profile: str):
        self.line()
        self.line("  +----------------------------------------------------------+", "36")
        self.line("  |  M Y W A R P                           PACKAGE BUILDER   |", "1;36")
        self.line("  +----------------------------------------------------------+", "36")
        self.line(f"  {self.mode.upper()}  /  {version}  /  Windows {arch}  /  {profile}", "1")
        self.line(f"  Build log: {self.log_path}", "90")
        self.line()

    def complete_step(self):
        if self.current and self.current["status"] == "running":
            self.current.update(status="passed", duration=time.monotonic() - self.current["start"])
            self.line(f"  [OK] {self.current['title']}  ({self.current['duration']:.1f}s)", "32")

    def step(self, number: int, title: str):
        self.complete_step()
        self.current = {"number": number, "title": title, "status": "running", "start": time.monotonic(), "duration": 0}
        self.steps.append(self.current)
        self.detail = title
        self.last_progress = 0
        self.line(f"\n  [{number}/5] {title}", "1;36")

    def progress(self, detail: str | None = None):
        if detail and detail.strip():
            self.detail = detail.strip()
        now = time.monotonic()
        if now - self.last_progress < (0.12 if self.color else 15):
            return
        self.last_progress = now
        elapsed = int(now - self.started)
        spinner = ("|", "/", "-", "\\")[int(now * 5) % 4]
        message = f"  {spinner}  {elapsed // 60:02d}:{elapsed % 60:02d}  {self.detail[:95]}"
        if self.color:
            print("\r\033[2K" + self.paint(message, "36"), end="", flush=True)
            self.live = True
        else:
            print(message, flush=True)

    def run(self, arguments, *, repo: Path, env=None):
        command = [str(argument) for argument in arguments]
        self.log.write("\n> " + subprocess.list2cmdline(command) + "\n")
        process = subprocess.Popen(command, cwd=repo, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        lines = queue.Queue()

        def read_output():
            try:
                for line in process.stdout:
                    lines.put(line)
            finally:
                process.stdout.close()
                lines.put(None)

        reader = threading.Thread(target=read_output, daemon=True)
        reader.start()
        tail = []
        try:
            while True:
                try:
                    line = lines.get(timeout=0.2)
                except queue.Empty:
                    self.progress()
                    continue
                if line is None:
                    break
                self.log.write(line)
                tail = (tail + [line.rstrip()])[-24:]
                self.progress(line)
            result = process.wait()
        except BaseException:
            if sys.platform == "win32":
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
            else:
                process.kill()
            process.wait()
            raise
        finally:
            self.clear_live()
            reader.join(timeout=2)
        if result:
            for line in tail:
                self.line("    " + line, "31")
            raise subprocess.CalledProcessError(result, command)

    def finish(self, *, error: str | None = None, artifacts=(), profile: str = ""):
        if error and self.current:
            self.current.update(status="failed", duration=time.monotonic() - self.current["start"])
        elif not error:
            self.complete_step()
        self.line()
        duration = time.monotonic() - self.started
        self.line(f"  {'BUILD FAILED' if error else 'PACKAGE READY'}  |  {duration / 60:.1f} minutes", "1;31" if error else "1;32")
        if error:
            self.line("  " + error, "31")
        for path in artifacts:
            size = f"  ({path.stat().st_size / 1024 / 1024:.1f} MB)" if path.is_file() else ""
            self.line(f"  {path}{size}", "32")
        if profile:
            self.line("  Configuration: " + profile)
        self.line("  Full log: " + str(self.log_path), "90")
        self.line("  Build report: " + str(self.report_path), "90")
        esc = html.escape
        rows = "".join(f'<li class="{step["status"]}"><span>{step["number"]:02}</span><strong>{esc(step["title"])}</strong><small>{step["duration"]:.1f}s / {step["status"]}</small></li>' for step in self.steps)
        links = "".join(f'<p><a href="{path.as_uri()}">{esc(path.name)}</a></p>' for path in artifacts)
        self.report_path.write_text(f'''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>MyWarp build report</title><style>
body{{background:#10151e;color:#e9eff7;font:16px/1.5 Segoe UI,sans-serif;margin:0}}main{{max-width:920px;margin:60px auto;padding:32px}}header{{border-top:4px solid #59dac5;padding:28px 0}}h1{{font-size:40px;margin:8px 0}}.label{{letter-spacing:4px;color:#59dac5;font-size:12px}}.muted,small{{color:#9aadc5}}ul{{padding:0;list-style:none}}li{{display:flex;gap:20px;background:#1b2534;border:1px solid #2b3b51;border-radius:12px;padding:20px;margin:12px 0;align-items:center}}li span{{color:#59dac5}}li small{{margin-left:auto}}.failed span,.error{{color:#ff9b91}}a{{color:#78dff1;overflow-wrap:anywhere}}.files{{background:#192b2c;border-radius:12px;padding:12px 24px}}code{{overflow-wrap:anywhere}}@media(max-width:650px){{main{{margin:12px auto;padding:20px}}li{{flex-wrap:wrap}}li small{{margin-left:0}}}}</style><main><header><div class="label">MYWARP / WINDOWS BUILD</div><h1>{'Build needs attention' if error else 'Your package is ready'}</h1><p class="muted">{esc(self.mode.title())} &middot; {duration / 60:.1f} minutes &middot; {datetime.now():%d %b %Y, %H:%M}</p></header>{'<p class="error">' + esc(error) + '</p>' if error else ''}<ul>{rows}</ul><div class="files">{links}<p>Configuration: <code>{esc(profile or 'See build log')}</code></p></div><p><a href="{self.log_path.as_uri()}">Open the full build log</a></p></main></html>''', encoding="utf-8")
        self.log.close()
