# 开发指南 / Development Guide

本文面向贡献者，覆盖环境搭建、架构总览、测试体系、代码规范与构建打包。
For contributors: setup, architecture, testing, conventions, and packaging.

## 1. 环境要求

| 依赖 | 说明 |
| --- | --- |
| Node.js 22+ / npm | 前端工具链（Vite、Vitest、vue-tsc） |
| Rust 工具链 | MSRV：`crates/pdf-core` 为 1.88，`src-tauri` 为 1.93（CI 强制校验） |
| Tauri 系统依赖 | Linux 需要 `webkit2gtk-4.1`、`gtk3`、`librsvg`、`patchelf`；其他平台见 Tauri 官方文档 |
| cargo-fuzz（可选） | 模糊测试，需要 nightly 工具链 |
| cargo-mutants（可选） | 变异测试 |

```bash
# 安装 JS 依赖
npm install

# 安装可选的 Rust 工具（按需）
cargo install cargo-fuzz cargo-mutants
```

## 2. 常用命令速查

```bash
npm run dev                  # 仅启动前端（浏览器预览，无原生能力）
npm run tauri dev            # 完整桌面应用开发模式（推荐日常使用）
npm test                     # 前端单元测试（Vitest）
npm run build                # 前端类型检查（vue-tsc）+ 生产构建

cargo test --workspace       # Rust 单元 + 集成测试 + bindings 再生成
cargo clippy --workspace --all-targets -- -D warnings   # CI 同款 lint
cargo bench -p pdf-core      # 压缩性能基准（criterion）

cargo run -p pdf-core --bin pdf-cli -- analyze <file.pdf>
cargo run -p pdf-core --bin pdf-cli -- compress <file.pdf> --preset maximum
cargo run -p pdf-core --bin pdf-cli -- compress <file.pdf> --target-size 5MB
```

## 3. 架构总览

```
┌─────────────────────────────┐       ┌──────────────────────────────┐
│  前端 (src/, Vue 3 + TS)     │  IPC  │  桌面壳 (src-tauri, Tauri 2) │
│  App.vue ─ 5 个面板组件      │ ◄───► │  commands.rs（10 个命令）     │
│  usePdfCompressor（核心状态）│ typed │  任务注册表 / 输出路径白名单    │
└─────────────────────────────┘  IPC  └──────────────┬───────────────┘
                                                      │ 直接调用
                                        ┌─────────────▼──────────────┐
                                        │ 引擎 (crates/pdf-core)      │
                                        │ analyzer / compressor /     │
                                        │ target_size / settings      │
                                        └────────────────────────────┘
```

### 分层规则

- **`crates/pdf-core`**：纯 PDF 引擎，不依赖 Tauri/UI。公共 API 只有
  `analyze_pdf_with_progress`、`compress_pdf_with_progress`、
  `compress_pdf_to_target_size`、`CompressionSettings(Overrides)` 与错误/模型类型，
  在 `src/lib.rs` 统一导出。四个前端共享它：Tauri 应用、`pdf-cli`、criterion 基准、cargo-fuzz。
  引擎内部分层：`pdf/compressor.rs`（文档编排与批量调度）、`pdf/encode.rs`（单图
  编解码：跳过启发式/解码/缩放/JPEG 重编码/软蒙版）、`pdf/search.rs`（目标大小搜索
  状态：逐图缓存与探测/物化）、`pdf/target_size.rs`（搜索调度与入口）、
  `pdf/workers.rs`（共享有界作用域线程池，图片优化与探测轮共用）。
- **`src-tauri`**：薄 IPC 壳。命令解析请求、合并设置、注册取消标志、`spawn_blocking`
  调引擎、把输出路径登记进 `SessionOutputRegistry`（`open_path`/`reveal_path_in_folder`
  只接受本会话产物，防止 IPC 沦为“打开任意文件”的原语）。
- **前端**：无 Pinia，状态集中在单例 composable `usePdfCompressor`
  （队列、逐任务设置、并发调度、取消、localStorage 持久化）。
  `src/lib/bindings.ts` 是 tauri-specta 生成的类型化 IPC 层（勿手改）；
  `src/lib/tauri.ts` 在其上封装进度通道归一化与浏览器降级。

### 一次压缩请求的数据流

1. 前端调用生成的 binding → Tauri IPC → `commands::compress_pdf`。
2. 命令解析路径、过滤 `target_size_bytes`、经 `CompressionSettings::from_sources`
   合并设置（优先级：overrides > payload > preset 默认值，并夹紧范围）。
3. 注册 `AtomicBool` 取消标志 → `spawn_blocking` → 按是否设置目标大小路由到
   `compress_pdf_to_target_size`（内存中二分搜索质量/边长）或 `compress_pdf_with_progress`。
4. 进度经 `tauri::ipc::Channel<ProgressUpdate>` 回传；引擎内 `ensure_not_cancelled` 轮询取消。
5. 输出写为 `<原名>__optimized-<preset>.pdf`（重名追加数字后缀），响应登记输出路径。

## 4. 测试体系

| 层 | 命令 | 覆盖内容 |
| --- | --- | --- |
| 前端单元 | `npm test` | 队列/调度/取消（usePdfCompressor）、通知计时（useErrorToasts）、右键菜单键盘可达性、设置面板（预设/校验/应用到全部）、App 装配冒烟、en/zh-CN key 树一致性 |
| 引擎单元 | `cargo test -p pdf-core --lib` | 设置夹紧与优先级、分析推荐公式、JPEG 头解析、worker 数量界、resize 行为、目标大小搜索调度数学（质量二分/边长收缩/边界重置） |
| 引擎集成 | `cargo test -p pdf-core --lib`（`pdf/tests.rs`） | 真实 lopdf 构造的 PDF：往返保文本且缩减>50%、去重、SMask、灰度、目标大小、96 用例变异语料不 panic |
| CLI 单元 | `cargo test -p pdf-core --bin pdf-cli` | `parse_size`/`flag_value` 参数解析 |
| 桌面壳单元 | `cargo test -p app --lib` | 任务注册表（注册/取消/注销）、输出路径白名单（含规范化）、预设配置原子写/读回/清除/损坏 JSON、`existing_paths` 过滤 |
| Bindings | `cargo test --workspace`（含 `export_bindings`） | 由 Rust 签名再生成 `src/lib/bindings.ts` —— **命令签名变更后必须运行并提交再生成结果** |
| 基准 | `cargo bench -p pdf-core` | 全管线各预设、编码器对比（jpeg-encoder vs image crate）；夹具生成器共享自 `testutil` |
| 变异测试 | `cargo mutants`（在 `crates/pdf-core` 下，配置 `.cargo/mutants.toml`） | 每周一 CI 自动跑（`.github/workflows/mutants.yml`），报告在 `mutants.out/` |
| 模糊测试 | `cd crates/pdf-core && cargo +nightly fuzz run pipeline` | 任意字节跑 analyze+compress+目标大小搜索；CI 每次 push 冒烟 60s |

夹具共享：`crates/pdf-core/src/testutil.rs` 提供确定性图片/JPEG 生成器
（`deterministic_rgb_image`/`fixture_rgb_image`/`encode_jpeg`），由 `testutil` feature
控制，经 crate 对自身的 dev-dependency 只在测试/基准/示例中启用（resolver = "2"
保证不泄漏进正常构建）。集成测试、两个 bench 与 `examples/make_fixture.rs` 共用它，
不要在各处复制生成器。

CI（`.github/workflows/ci.yml`）在每次 push/PR 执行：前端测试+类型检查+构建、
Rust clippy `-D warnings` + 测试、两个 MSRV 检查、60s 模糊测试、依赖审计
（cargo audit + npm audit）。

### 测试约定

- 前端测试在 `src/**/__tests__/*.spec.ts`，`lib/tauri` 一律 mock，只测状态转换。
- 引擎集成测试用 `tests.rs` 顶部的 fixture 生成器构造 PDF，不依赖外部文件。
- 新增 Tauri 命令：改 `commands.rs` → `cargo test --workspace` 再生成 bindings → 前端经
  `src/lib/tauri.ts` 封装调用，禁止组件直接 `invoke`。

## 5. 代码规范与约定

- **Rust**：workspace 级 clippy（`Cargo.toml [workspace.lints]`），CI 以 `-D warnings` 执行；
  `redundant_clone`/`too_many_arguments` 等为 deny。提交前本地跑一遍同款命令。
- **TypeScript/Vue**：`npm run build` 内含 vue-tsc 类型检查；组件内 props down / emits up，
  业务状态只进 `usePdfCompressor`。
- **i18n**：所有用户可见文案进 `src/locales/{en,zh-CN}.ts`，两份文件 key 必须一致；
  后端文案用 code（如 `compress.warning.targetSizeMissed`）+ values 传参，前端
  `backendMessages.ts` 负责本地化与回退（en 缺失时回退 backend `fallback` 文本）。
- **注释**：模块头双语（英/中）说明职责，函数注释只写“为什么”。
- **版本**：`package.json` 是唯一版本源，改版本后运行 `npm run sync-version`
  同步到 `tauri.conf.json` 与 `src-tauri/Cargo.toml`。

## 6. 构建与打包

```bash
npm run build        # 前端生产包（dist/）
npm run tauri build  # 桌面安装包（Windows: NSIS）
npm run tauri:build  # 安装包 + 便携版可执行文件（scripts/postbuild-portable.mjs）
```

- Linux（Arch）：AUR 源码包在 `aur/pdf-compressor/`，PKGBUILD 走
  `npm ci && npm run build` + `cargo build --release --locked`。
- Release 流程见 `.github/workflows/release.yml`；产物命名与标识符见 README「Release metadata」。

## 7. 开发常见问题

- **浏览器预览里 Browse 按钮不可用 / 拖拽拿不到路径**：预期行为。原生能力检测用
  `hasNativeCommands()`（内部 `isTauri()`），浏览器模式自动降级；要验证原生交互请用
  `npm run tauri dev`。
- **改了命令签名后前端类型报错**：运行 `cargo test --workspace` 再生成 bindings 并提交。
- **压缩后文件没变小**：先看分析结果 `documentKind`（text-native 压缩空间有限）、
  是否无内嵌图片、或图片过滤器不受支持（JPX/JBIG2/CCITT/Crypt 会跳过）。
- **目标大小模式未达标**：引擎最多尝试 6 轮，产出“当前可达的最小结果”并返回
  `compress.warning.targetSizeMissed` 提示；前端以警告 toast + 状态卡 notes 呈现。
- **`cargo bench` 名字冲突**：基准每轮使用独立临时目录，避免 100 次重名上限。
