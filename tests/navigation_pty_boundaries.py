#!/usr/bin/env python3
"""Exercise real hx viewport/navigation on empty, tiny and 70k-line documents."""
import json
import os
import pathlib
import pty
import signal
import subprocess
import sys
import tempfile
import time

from interactive_resource_pressure import press, receive, resize

CASES = [
    ("empty", "", 24),
    ("single_line", "x", 24),
    ("few_lines_no_final_newline", "a\nb\nc", 24),
    ("few_lines_with_final_newline", "a\nb\nc\n", 24),
    ("unbroken_very_long_line", "x" * 10_000, 24),
    ("very_long_document", "a\n" * 70_000, 24),
]
NAVIGATION = [
    b"g", b"g", b"g", b"e",  # gg, ge
    b"\x1b[6~", b"\x1b[5~",  # PageDown, PageUp
    b"\x04", b"\x15",         # Ctrl-D, Ctrl-U
    b"g", b"x",              # unexpected continuation safely exits Goto
    b"g", b"g",
]
results = []

def run_case(binary, root, name, content, rows):
    file_path = root / (name + ".txt")
    file_path.write_text(content, encoding="utf-8")
    master, slave = pty.openpty()
    resize(slave, rows, 100)
    env = os.environ.copy()
    env["TERM"] = "xterm-256color"
    env["RUST_BACKTRACE"] = "1"
    stderr_path = root / (name + ".stderr")
    with stderr_path.open("w+b") as err:
        p = subprocess.Popen([binary, str(file_path)], stdin=slave, stdout=slave,
                             stderr=err, env=env, start_new_session=True)
        os.close(slave)
        try:
            startup_ms, data = receive(master, p, timeout=20)
            if not data:
                raise RuntimeError("no first terminal frame")
            latency = [press(master, p, event) for event in NAVIGATION]
            resize(master, 2, 9)
            os.kill(p.pid, signal.SIGWINCH)
            # A tiny terminal cannot display the normal full viewport.
            # Recover before querying terminal response again.
            time.sleep(0.05)
            if p.poll() is not None:
                raise RuntimeError("exited during tiny-terminal resize")
            resize(master, rows, 100)
            os.kill(p.pid, signal.SIGWINCH)
            latency.append(press(master, p, b"g"))
            latency.append(press(master, p, b"e"))
            os.write(master, b"q")
            p.wait(timeout=12)
            err.flush()
            err.seek(0)
            errors = err.read().decode("utf-8", "replace")
            if p.returncode != 0 or "panicked at" in errors or "thread 'main' panicked" in errors:
                raise RuntimeError("hx returned %r or panic output: %s" % (p.returncode, errors[-500:]))
            result = {
                "file": name, "file_lines": content.count("\n") + 1,
                "events": len(latency), "boot_ms": round(startup_ms, 3),
                "max_first_output_ms": round(max(latency), 3),
                "exit_code": p.returncode,
            }
            results.append(result)
            print("NAVIGATION_BOUNDARY " + json.dumps(result), flush=True)
        finally:
            if p.poll() is None:
                p.kill()
                p.wait(timeout=5)
            os.close(master)

def main(binary):
    with tempfile.TemporaryDirectory(prefix="hx-nav-boundaries-") as folder:
        root = pathlib.Path(folder)
        for name, content, rows in CASES:
            run_case(binary, root, name, content, rows)

if __name__ == "__main__":
    failure = None
    try:
        main(os.path.abspath(sys.argv[1]))
    except BaseException as exc:
        failure = "%s: %s" % (type(exc).__name__, exc)
        print("NAVIGATION_BOUNDARY_FAILURE " + failure, file=sys.stderr, flush=True)
    pathlib.Path("navigation-boundaries-results.json").write_text(
        json.dumps({"cases": results, "failure": failure}, indent=2) + "\n"
    )
    sys.exit(1 if failure else 0)
