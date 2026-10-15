#!/usr/bin/env python3
"""Persistence acceptance against actual hx PTY and on-disk bytes.

Must not infer correctness from the fact that the process emitted terminal bytes.
"""
import json
import os
import pathlib
import pty
import subprocess
import sys
import tempfile

from interactive_resource_pressure import press, receive, resize

results = []


def scenario(binary, root, name):
    target = root / (name + ".txt")
    existing = name in ("dirty_guard", "overwrite_save")
    if existing:
        target.write_text("original\n", encoding="utf-8")
    master, slave = pty.openpty()
    resize(slave, 20, 70)
    env = os.environ.copy()
    env["TERM"] = "xterm-256color"
    env["RUST_BACKTRACE"] = "1"
    argv = [binary] if name == "unnamed_save_as" else [binary, str(target)]
    stderr = root / (name + ".stderr")
    with stderr.open("w+b") as error_log:
        process = subprocess.Popen(argv, stdin=slave, stdout=slave, stderr=error_log,
                                   start_new_session=True, env=env)
        os.close(slave)
        try:
            receive(master, process, timeout=15)
            for raw in (b"i", b"Z", b"\x1b"):
                press(master, process, raw)
            if name == "dirty_guard":
                for raw in (b":", b"q", b"\r"):
                    press(master, process, raw)
                assert process.poll() is None, "dirty :q terminated the process"
                for raw in (b"\x1b", b":", b"q", b"!"):
                    press(master, process, raw)
                os.write(master, b"\r")
                expected = "original\n"
            else:
                if name == "unnamed_save_as":
                    command = "w " + str(target)
                elif name == "overwrite_save":
                    command = "wq"
                else:
                    command = "wq"
                press(master, process, b":")
                for c in command:
                    press(master, process, c.encode("utf-8"))
                os.write(master, b"\r")
                expected = "Z" + ("original\n" if existing else "")
            process.wait(timeout=12)
            assert process.returncode == 0, "nonzero exit: %s" % process.returncode
            actual = target.read_text(encoding="utf-8") if target.exists() else None
            assert actual == expected, (name, actual, expected)
            error_log.flush()
            error_log.seek(0)
            err = error_log.read().decode("utf-8", "replace")
            assert "panicked at" not in err, err[-300:]
            results.append({"case": name, "passed": True, "exit_code": process.returncode,
                            "persisted_bytes": len(actual.encode("utf-8"))})
            print("PERSISTENCE_PTY " + json.dumps(results[-1]), flush=True)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=4)
            os.close(master)


def main(binary):
    with tempfile.TemporaryDirectory(prefix="hx-persist-") as tmp:
        root = pathlib.Path(tmp)
        for case in ("dirty_guard", "overwrite_save", "new_named_file", "unnamed_save_as"):
            scenario(binary, root, case)


if __name__ == "__main__":
    failure = None
    try:
        main(os.path.abspath(sys.argv[1]))
    except BaseException as exc:
        failure = "%s: %s" % (type(exc).__name__, exc)
        print("PERSISTENCE_PTY_FAILURE " + failure, file=sys.stderr, flush=True)
    pathlib.Path("persistence-results.json").write_text(
        json.dumps({"cases": results, "failure": failure}, indent=2) + "\n"
    )
    sys.exit(1 if failure else 0)
