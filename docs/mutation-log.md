# Mutation Testing Log

Mutation results are working artifacts (gitignored under `mutants.out/`),
which made historical "0 missed" claims unreproducible. This file is the
durable record: every mutation campaign logs its scope, counts, and any
missed mutants with the test that then killed them. Rerun locally with
`cargo mutants` (see `.cargo/mutants.toml`; ~110 s/mutant — not in CI by
design, the 6 h job limit is too tight).

## 2026-09-09 — post-0.9.0 engine hardening pass

- Scope: default test suite, engine crate.
- 35 mutants generated; 33 caught, **2 missed**, 0 timed out.
- Missed mutants (both `target_size.rs:394/395`, removal of the
  `output_was_smaller`/`saved_bytes` fields in the probe accounting):
  covered by dedicated assertions added the same day (the target-size
  report tests now pin those fields; see
  `target_size_pipe_accounting_describes_the_emitted_bytes`).

## 2026-09-09 — targeted re-run (post-fix verification)

- 13 mutants (the affected region); 12 caught, 0 missed, 1 unviable.

## 2026-09-18 — 0.11.0 audit-hardening batch

- The P0/P1 fixes landed with new pins first; a full post-batch campaign is
  pending (run `cargo mutants` after review and append the counts here
  before the 0.11.0 release).
