#!/usr/bin/env python3
"""Real terminal command-prompt regression: UTF-8, narrow terminals and exit semantics.

Tests events against the actual hx process. Measures only first PTY output, not
full-frame latency; no terminal-cell correctness is inferred from byte presence.
"""
import json
import os
import pathlib
import pty
import signal
import subprocess
import sys
import tempfile

from interactive_resource_pressure import press, receive, resize

CASES = ("unknown_then_edit", "tab_completion", "unicode_graphemes", "resize_live")
results = []


def run_case(binary, folder, name):
    file_path = folder / (name + ".txt")
    file_path.write_text("source\nsecond\n", encoding="utf-8")
    master, slave = pty.openpty()
    resize(slave, 20, 60)
    env = os.environ.copy()
    env["TERM"] = "xterm-256color"
    env["RUST_BACKTRACE"] = "1"
    err_path = folder / (name + ".stderr")
    with err_path.open("w+b") as err:
        proc = subprocess.Popen([binary, str(file_path)], stdin=slave, stdout=slave,
                                stderr=err, start_new_session=True, env=env)
        os.close(slave)
        try:
            receive(master, proc, timeout=20)
            latencies = []
            def step(raw):
                latencies.append(press(master, proc, raw))

            step(b":")
            if name == "unknown_then_edit":
                step(b"x")
                step(b"\r")  # Unrecognized command cannot close the editor.
                step(b"\x7f")
                step(b"q")  # q is input in a prompt, not the Normal-mode quit key.
                step(b"\x1b")  # cancel without executing
                step(b"i")
                step(b"q")  # q is literal text in Insert mode.
                step(b"\x1b")
                os.write(master, b"q")
            elif name == "tab_completion":
                step(b"\t")  # Select a candidate without rewriting typed prefix.
                os.write(master, b"\r")  # Accept the selected actual :q command.
            elif name == "unicode_graphemes":
                for codepoint in "中🙂e\u0301":
                    step(codepoint.encode("utf-8"))
                step(b"\x1b[D")
                step(b"\x7f")
                step(b"\x1b[H")
                step(b"\x1b[3~")
                step(b"\x1b")
                os.write(master, b"q")
            else:
                for columns, rows in [(29, 3), (12, 2), (9, 2), (7, 2), (60, 20)]:
                    resize(master, rows, columns)
                    os.kill(proc.pid, signal.SIGWINCH)
                    step(b"x")
                    step(b"\x7f")
                step(b"\x1b")
                os.write(master, b"q")

            proc.wait(timeout=12)
            err.flush()
            err.seek(0)
            errors = err.read().decode("utf-8", "replace")
            if proc.returncode != 0 or "panicked at" in errors or "thread 'main' panicked" in errors:
                raise RuntimeError("hx exited %s or panicked: %s" % (proc.returncode, errors[-500:]))
            result = {"case": name, "events": len(latencies), "max_first_output_ms":
                      round(max(latencies), 3), "exit_code": proc.returncode}
            results.append(result)
            print("PROMPT_PTY " + json.dumps(result), flush=True)
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait(timeout=5)
            os.close(master)


def main(binary):
    with tempfile.TemporaryDirectory(prefix="hx-prompt-") as temp:
        for name in CASES:
            run_case(binary, pathlib.Path(temp), name)


if __name__ == "__main__":
    failure = None
    try:
        main(os.path.abspath(sys.argv[1]))
    except BaseException as exc:
        failure = "%s: %s" % (type(exc).__name__, exc)
        print("PROMPT_PTY_FAILURE " + failure, file=sys.stderr, flush=True)
    pathlib.Path("prompt-pty-results.json").write_text(
        json.dumps({"cases": results, "failure": failure}, indent=2) + "\n"
    )
    sys.exit(1 if failure else 0)
