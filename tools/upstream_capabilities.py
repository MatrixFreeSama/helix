#!/usr/bin/env python3
"""Compare observable editor capabilities without equating PR counts with features.

This tool never changes application code. It inspects actual Git trees and
prints a factual upstream/fork capability matrix for migration planning.
"""
import argparse
import json
import pathlib
import subprocess
from collections import Counter

FEATURES = (
    ("LSP client", ("helix-lsp/src/client.rs",)),
    ("DAP debugger", ("helix-dap/src/client.rs",)),
    ("VCS integration", ("helix-vcs/src/lib.rs",)),
    ("Multiple views / windows", ("helix-view/src/editor.rs", "helix-term/src/ui/editor.rs")),
    ("File picker", ("helix-term/src/ui/picker.rs",)),
    ("LSP completion menu", ("helix-term/src/ui/completion.rs",)),
    ("Documentation popup", ("helix-term/src/ui/markdown.rs",)),
    ("Modern terminal component system", ("helix-term/src/ui/overlay.rs",)),
    ("Event engine", ("helix-event/src/lib.rs",)),
    ("Syntax query runtime", ("runtime/queries/rust/highlights.scm",)),
    ("User language registry", ("languages.toml",)),
    ("Language configuration", ("book/src/editor.md",)),
    ("Real-file persistence PTY regression", ("tests/persistence_pty.py",)),
    ("Live CPU pressure PTY regression", ("tests/interactive_resource_pressure.py",)),
)


def git(*args):
    return subprocess.check_output(("git",) + args, text=True).strip()


def files_at(ref):
    return set(filter(None, git("ls-tree", "-r", "--name-only", ref).splitlines()))


def facts(files):
    queries = [p for p in files if p.startswith("runtime/queries/") and p.endswith(".scm")]
    crates = sorted(
        p.rsplit("/", 1)[0] for p in files
        if p.endswith("/Cargo.toml") and p.count("/") == 1
    )
    syntax_submodules = sorted(
        p for p in files if p.startswith("helix-syntax/languages/tree-sitter-")
    )
    result = {
        "tracked_paths": len(files),
        "workspace_crate_directories": crates,
        "workspace_crate_count": len(crates),
        "syntax_query_files": len(queries),
        "syntax_query_languages": len({p.split("/")[2] for p in queries}),
        "legacy_tree_sitter_submodule_paths": len(syntax_submodules),
        "features": {},
    }
    for name, required_paths in FEATURES:
        result["features"][name] = all(path in files for path in required_paths)
    return result


def build_report(fork_ref, upstream_ref):
    fork_files = files_at(fork_ref)
    upstream_files = files_at(upstream_ref)
    local, remote = facts(fork_files), facts(upstream_files)
    data = {
        "fork": {"sha": git("rev-parse", fork_ref), **local},
        "upstream": {"sha": git("rev-parse", upstream_ref), **remote},
    }
    return data


def markdown(data):
    fork, upstream = data["fork"], data["upstream"]
    rows = [
        "## Helix functional baseline comparison",
        "",
        "This is an evidence-of-implementation inventory, not a functional parity certification.",
        "",
        "| Measured property | Industrial fork | Current upstream |",
        "|---|---:|---:|",
        f"| Root crate dirs | {fork['workspace_crate_count']} | {upstream['workspace_crate_count']} |",
        f"| Tracked paths | {fork['tracked_paths']} | {upstream['tracked_paths']} |",
        f"| Query files | {fork['syntax_query_files']} | {upstream['syntax_query_files']} |",
        f"| Query-language directories | {fork['syntax_query_languages']} | {upstream['syntax_query_languages']} |",
        "",
        "| Capability evidence | Industrial fork | Current upstream |",
        "|---|---|---|",
    ]
    for name in fork["features"]:
        rows.append(
            f"| {name} | {'source present' if fork['features'][name] else 'absent'} "
            f"| {'source present' if upstream['features'][name] else 'absent'} |"
        )
    rows.extend((
        "",
        f"Fork commit: {fork['sha']}",
        f"Upstream commit: {upstream['sha']}",
        "",
        "Presence of a source path does not prove runtime behavior; absent paths "
        "do not prove all equivalent features are absent. Tests and runtime "
        "behavior decide admission.",
        "",
    ))
    return "\n".join(rows)


def self_test():
    assert facts(set())["workspace_crate_count"] == 0
    sample = {
        "helix-core/Cargo.toml",
        "helix-view/Cargo.toml",
        "runtime/queries/rust/highlights.scm",
        "runtime/queries/rust/indents.scm",
        "runtime/queries/python/highlights.scm",
        "helix-lsp/src/client.rs",
    }
    result = facts(sample)
    assert result["workspace_crate_count"] == 2
    assert result["syntax_query_languages"] == 2
    assert result["syntax_query_files"] == 3
    assert result["features"]["LSP client"]
    assert not result["features"]["DAP debugger"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fork", default="HEAD")
    parser.add_argument("--upstream", default="FETCH_HEAD")
    parser.add_argument("--json-out")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("upstream capability inventory self-test passed")
        return
    data = build_report(args.fork, args.upstream)
    if args.json_out:
        pathlib.Path(args.json_out).write_text(
            json.dumps(data, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
    print(markdown(data))


if __name__ == "__main__":
    main()
