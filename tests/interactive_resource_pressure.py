#!/usr/bin/env python3
"""Exercise the real historical Helix terminal under changing CPU/OS resources.

Linux PTY integration test, not a lookup-only microbenchmark. Requires an hx
release binary built from the checked-out revision. Runs only in GitHub CI.
"""
import errno
import fcntl
import json
import os
import pathlib
import pty
import select
import signal
import statistics
import struct
import subprocess
import sys
import tempfile
import termios
import time

MAX_ACK_SECONDS = 8.0  # Only a broken/unresponsive editor fails this timeout.
SAMPLES_PER_PHASE = 200
RESULTS = {"runner_cpus": [], "scenarios": {}, "diagnostics": [], "failures": []}


def pct(values, percentile):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, max(0, int((len(ordered) - 1) * percentile + 0.5)))]


def record(name, values):
    data = {
        "events": len(values),
        "p50_ms": round(pct(values, 0.50), 3),
        "p95_ms": round(pct(values, 0.95), 3),
        "p99_ms": round(pct(values, 0.99), 3),
        "max_ms": round(max(values), 3),
        "over_100ms": sum(v > 100 for v in values),
        "over_500ms": sum(v > 500 for v in values),
    }
    RESULTS["scenarios"][name] = data
    print("DIRTY_BUSINESS " + json.dumps({"phase": name, **data}), flush=True)


def resize(fd, rows, cols):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def receive(fd, proc, timeout=MAX_ACK_SECONDS):
    started = time.monotonic()
    payload = bytearray()
    while time.monotonic() - started < timeout:
        if proc.poll() is not None:
            raise RuntimeError("hx exited unexpectedly with code " + str(proc.returncode))
        ready, _, _ = select.select([fd], [], [], min(0.1, timeout - (time.monotonic() - started)))
        if ready:
            try:
                chunk = os.read(fd, 65536)
            except OSError as exc:
                if exc.errno == errno.EIO:
                    raise RuntimeError("PTY closed unexpectedly (hx crashed/exited)") from exc
                raise
            if chunk:
                payload.extend(chunk)
                elapsed = (time.monotonic() - started) * 1000.0
                # Drain pending screen output; the measured number is time
                # to first terminal output, not guaranteed full-frame latency.
                while select.select([fd], [], [], 0.004)[0]:
                    try:
                        rest = os.read(fd, 65536)
                    except OSError as exc:
                        if exc.errno == errno.EIO:
                            break
                        raise
                    if not rest:
                        break
                    payload.extend(rest)
                return elapsed, bytes(payload)
    raise TimeoutError("hx produced no terminal output for %.1f seconds" % timeout)


def press(fd, proc, raw):
    if proc.poll() is not None:
        raise RuntimeError("hx exited before input: " + str(proc.returncode))
    os.write(fd, raw)
    latency, data = receive(fd, proc)
    if b"panicked at" in data or b"thread 'main' panicked" in data:
        raise RuntimeError("Rust panic found in PTY stream")
    return latency


def set_affinity_all(pid, allowed):
    # A Rust application can have multiple worker threads. Apply to *every*
    # existing Linux TID, not merely the thread-group leader.
    task_dir = pathlib.Path("/proc") / str(pid) / "task"
    tids = [int(p.name) for p in task_dir.iterdir()]
    for tid in tids:
        try:
            os.sched_setaffinity(tid, allowed)
        except ProcessLookupError:
            continue
    return tids


def worker_process(kind, cpu, mem_megabytes, filename):
    program = """
import os,sys,time
kind=sys.argv[1]
if kind=="cpu":
    x=1
    while True:
        x=(x*1664525+1013904223)&0xFFFFFFFF
elif kind=="memory":
    n=int(sys.argv[2])*1024*1024
    b=bytearray(n)
    for i in range(0,n,4096): b[i]=1
    while True:
        for i in range(0,n,4096*512): b[i]=(b[i]+1)&255
        time.sleep(.02)
elif kind=="io":
    path=sys.argv[3]
    block=b"x"*(256*1024)
    with open(path,"w+b",buffering=0) as f:
        while True:
            f.seek(0)
            for _ in range(16): f.write(block)
            os.fsync(f.fileno())
"""
    p = subprocess.Popen([sys.executable, "-c", program, kind, str(mem_megabytes), filename],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        os.sched_setaffinity(p.pid, {cpu})
    except (OSError, AttributeError):
        pass
    return p


def stop_workers(workers):
    for worker in workers:
        if worker.poll() is None:
            worker.terminate()
    for worker in workers:
        try:
            worker.wait(timeout=2)
        except subprocess.TimeoutExpired:
            worker.kill()
            worker.wait(timeout=2)


def input_phase(name, fd, proc, count=SAMPLES_PER_PHASE):
    latencies = []
    for i in range(count):
        # Do not send 'q': the historical editor treats it as global Quit.
        raw = b"\\t" if i % 11 == 0 else b"\\r" if i % 17 == 0 else b"a"
        latencies.append(press(fd, proc, raw))
    record(name, latencies)


def available_memory_mib():
    for line in pathlib.Path("/proc/meminfo").read_text().splitlines():
        if line.startswith("MemAvailable:"):
            return int(line.split()[1]) // 1024
    return 1024

def memory_pressure_budget_mib():
    available = available_memory_mib()
    cgroup_max = pathlib.Path("/sys/fs/cgroup/memory.max")
    if cgroup_max.exists():
        content = cgroup_max.read_text().strip()
        if content.isdigit():
            available = min(available, int(content) // (1024 * 1024))
    # Use up to one quarter of a bounded, observed memory budget.
    return max(128, min(3072, available // 4))


def resident_mib(pid):
    for line in (pathlib.Path("/proc") / str(pid) / "status").read_text().splitlines():
        if line.startswith("VmRSS:"):
            return int(line.split()[1]) // 1024
    return 0


def main(binary):
    cpus = sorted(os.sched_getaffinity(0))
    RESULTS["runner_cpus"] = cpus
    if len(cpus) < 2:
        raise RuntimeError("Need >=2 schedulable CPUs for a genuine running-process CPU withdrawal test")
    # A short file with no known syntax extension avoids loading unrelated
    # tree-sitter configurations into this input/terminal regression.
    with tempfile.TemporaryDirectory(prefix="helix-dirty-") as tmp:
        file = pathlib.Path(tmp) / "fixture.txt"
        file.write_text("start\\n" + "".join("line %03d  ab\\tcd\\n" % n for n in range(60)))
        master, slave = pty.openpty()
        resize(slave, 30, 100)
        env = os.environ.copy()
        env["TERM"] = "xterm-256color"
        env["RUST_BACKTRACE"] = "1"
        workers = []
        stderr_path = pathlib.Path(tmp) / "editor-stderr.txt"
        with stderr_path.open("w+b") as error_log:
            proc = subprocess.Popen([binary, str(file)], stdin=slave,
                                    stdout=slave, stderr=error_log,
                                    start_new_session=True, env=env)
            os.close(slave)
            try:
                boot_ms, boot_output = receive(master, proc, timeout=15.0)
                RESULTS["boot_ms"] = round(boot_ms, 3)
                RESULTS["boot_bytes"] = len(boot_output)
                press(master, proc, b"i")  # Enter insert mode before any pressure.
                input_phase("baseline", master, proc)
                set_affinity_all(proc.pid, {cpus[0]})
                RESULTS["diagnostics"].append({
                    "event": "withdraw_cpu_during_editing",
                    "available_before": len(cpus), "available_after": 1
                })
                input_phase("one_cpu_after_live_withdrawal", master, proc)
                set_affinity_all(proc.pid, set(cpus))
                input_phase("cpu_restored", master, proc, 24)

                memory_mib = memory_pressure_budget_mib()
                RESULTS["memory_pressure_target_mib"] = memory_mib
                load_file = str(pathlib.Path(tmp) / "bounded_io_pressure.dat")
                cpu_workers = max(2, min(8, len(cpus) * 2))
                RESULTS["cpu_contenders"] = cpu_workers
                for i in range(cpu_workers):
                    workers.append(worker_process("cpu", cpus[i % len(cpus)], memory_mib, load_file))
                memory_worker = worker_process("memory", cpus[0], memory_mib, load_file)
                workers.append(memory_worker)
                workers.append(worker_process("io", cpus[-1], memory_mib, load_file))
                for attempt in range(40):
                    if all(worker.poll() is None for worker in workers) and resident_mib(memory_worker.pid) >= memory_mib * 0.8:
                        break
                    time.sleep(0.2)
                for worker in workers:
                    if worker.poll() is not None:
                        raise RuntimeError("resource contender exited before load test")
                RESULTS["memory_contender_rss_mib"] = resident_mib(memory_worker.pid)
                if RESULTS["memory_contender_rss_mib"] < memory_mib * 0.8:
                    raise RuntimeError("memory contention did not actually allocate target pages")
                input_phase("cpu_memory_io_contention", master, proc)
                set_affinity_all(proc.pid, {cpus[0]})
                input_phase("cpu_withdrawal_under_contention", master, proc)
                set_affinity_all(proc.pid, set(cpus))
                stop_workers(workers)
                workers.clear()
                input_phase("recovered_after_resource_release", master, proc)

                resize(master, 10, 30)
                os.kill(proc.pid, signal.SIGWINCH)
                input_phase("terminal_resize_small", master, proc, 20)
                resize(master, 30, 100)
                os.kill(proc.pid, signal.SIGWINCH)
                input_phase("terminal_resize_recovered", master, proc, 20)

                # Quit intentionally ends the process; do not demand an ACK
                # from a PTY that may close before we can read its last frame.
                os.write(master, b"q")
                proc.wait(timeout=10)
                if proc.returncode != 0:
                    raise RuntimeError("hx did not exit cleanly: " + str(proc.returncode))
                RESULTS["exit_code"] = proc.returncode
                RESULTS["process_alive_after_each_phase"] = True
            finally:
                stop_workers(workers)
                if proc.poll() is None:
                    proc.kill()
                    proc.wait(timeout=5)
                os.close(master)
                error_log.flush()
                error_log.seek(0)
                errors = error_log.read().decode("utf-8", "replace")
                RESULTS["editor_stderr_tail"] = errors[-2000:]
                if "panicked at" in errors or "thread 'main' panicked" in errors:
                    RESULTS["failures"].append("Rust panic detected in stderr")


if __name__ == "__main__":
    try:
        main(os.path.abspath(sys.argv[1]))
    except BaseException as exc:
        RESULTS["failures"].append("%s: %s" % (type(exc).__name__, exc))
        print("DIRTY_BUSINESS_FAILURE " + repr(exc), file=sys.stderr, flush=True)
    finally:
        pathlib.Path("dirty-business-results.json").write_text(
            json.dumps(RESULTS, indent=2, ensure_ascii=False) + "\\n")
        print("DIRTY_BUSINESS_SUMMARY " + json.dumps(RESULTS, ensure_ascii=False), flush=True)
    sys.exit(1 if RESULTS["failures"] else 0)
