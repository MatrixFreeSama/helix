# Modern upstream integration

The industrial `master` and the modern upstream integration are distinct on purpose.

- `master` retains the audited early-Helix implementation and its PTY tests.
- `integration/modern-upstream` is fast-forward-only and contains the exact
  upstream source revision that passed the updater's checks.
- The weekly updater lives at `.github/workflows/upstream-modernization.yml`.
  It also supports explicit dispatch. A failed check prevents staging.
- `tools/upstream_capabilities.py` emits the source-level feature gap.
  Presence of a file is not evidence of behavioral correctness.
- No synchronization step force-pushes, merges, or resets `master`.

## Observable behavior contract

Modern Helix defaults to `insert-final-newline = true`, so a newly typed
`Z` is saved as `Z\\n` in a new file. The industrial fork preserves
the exact input bytes. The explicit parity profile at
`tools/modern-parity.toml` sets `insert-final-newline = false` and preserves
upstream's native `atomic-save = true`.

The CI copies the profile to a private temporary file and passes its
absolute path to the real modern binary via the documented `--config` CLI
argument. It does not depend on a particular XDG base-directory strategy.
The CI then runs the **same five real-file persistence PTY cases** against
the modern binary. A default-setting discrepancy is reported, not silently ignored.

For a local modern build, start `hx --config /path/to/modern-parity.toml FILE`
or merge this profile into the user Helix configuration (usually
`~/.config/helix/config.toml` on Unix). Explicit `--config` is the preferred
reproducible contract, and avoids guessing a configuration directory.

## Industrial promotion requirements

Staging is **not** promotion. Before switching the fork's main editor:

1. Preserve or strengthen its complete user-facing commands, navigation,
   selection and persistence semantics; establish modern-language coverage.
2. Translate the proven industrial invariants into the modern core's shared
   transaction, selection, document and terminal mechanisms, not compatibility
   wrappers or benchmark-specific paths.
3. Run randomized structure-level invariants, Unicode boundaries, 70k+ line
   real PTY navigation, terminal resizing, resource contention/CPU affinity
   withdrawal and byte-for-byte disk persistence.
4. Reconcile clean and dirty close, external file changes, read-only file
   behavior and atomic writes. Compare p95/p99 response distributions without
   overstating measurements of first PTY output as whole-frame latency.
5. Promote only after the complete branch passes its own acceptance suite.
   Remove the superseded old implementation once the functional transition
   is proven rather than maintaining parallel engines indefinitely.

Upstream uses MPL-2.0; preserve license notices if adapting its code.
