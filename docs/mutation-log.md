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

- The P0/P1 fixes landed with new pins first; the full post-batch campaign
  ran 2026-09-20 (next entry).

## 2026-09-20 — 0.11.0 终态全量 campaign（PLAN-1.0 §3.2 B2）

- 范围：全 workspace（pdf-core + src-tauri；bin 沿用 mutants.toml 排除）。
- 80 变异体：23 杀、38 漏、19 不可行、0 超时。
- **pdf-core 引擎零漏杀**——0.11.0 审查收口批次的验收标准达成。
- 漏杀全部位于 src-tauri 壳层（GUI 入口、IPC handler、配置 IO——无
  集成 harness 的既有盲区，0.11.0 前即存在）。其中 A4 新增的
  `argv_pdf_paths` 漏杀直接暴露冷启动双 skip 真 bug（首份 PDF 被
  吞）：已修复并补纯函数测试；其余维持记录在案，列入 1.x 测试债
  （壳层集成测试需 mock runtime，另立工作）。
