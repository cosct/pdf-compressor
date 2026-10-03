# 1.0 开发与验收计划（2026-10-02）

本计划替代 [旧计划](PLAN-1.0-2026-09-20.md)，依据 [全面审查](REVIEW-2026-10-02.zh-CN.md)。1.0 定位为桌面、CLI、Android 共用引擎的可靠性收口版本。先完成开发验收，再执行发布验收；版本号与正式 tag 在发布条件满足后更新。

## 范围与原则

- 修复 R01–R15：文件所有权与原子替换、取消提交边界、搜索缓存、桌面任务生命周期、持久化字段校验、Android SAF/密码/取消/顺序队列、原生构建与发布校验和。
- 维持现有 CLI、IPC、预设与配置迁移兼容；不新增编解码器或替换框架。
- 不将取消 ACK 当作任务结束，不将单流/轮末缓存限额当作进程峰值保证。
- 自动测试完成与实机/发布验收分别记录，未完成项不得标绿。

## 实施批次

| 批次 | 范围 | 验收 | 状态 |
| --- | --- | --- | --- |
| 1 文件安全 | R01/R13/R14，共用临时文件所有权和替换实现 | 同前缀文件保留、失败不丢旧值、提交前取消 | 开发验收通过 |
| 2 内存与调度 | R07，有界在途结果与逐项缓存控制 | 多图片缓存测试、取消/错误无死锁、保真回归 | 开发验收通过 |
| 3 桌面队列 | R11/R12/R15 | cancel ACK 先返回、大小写路径、坏配置恢复 | 开发验收通过 |
| 4 Android | R02/R03/R04/R05/R06/R10 | Kotlin 测试、APK 构建/lint、权限/读入限量/密码/取消/顺序链 | 开发验收通过 |
| 5 交付 | R08/R09、版本及 ABI 检查 | 原生库缺失失败、variant 分离、GH_REPO 明确 | 开发验收通过 |
| 6 集成验收 | 全量质量门、文档与变更记录 | 默认及可选 feature、前端构建、Android 检查 | 开发验收通过 |

## 发布门槛

- [x] 上述开发批次全部通过，测试结果与限制记录入本文件。
- [ ] Windows/macOS/Linux 最终安装包各完成选档、压缩、打开、CLI/右键入口烟测。
- [ ] Android API 34+ 队列前台服务、密码、取消、SAF 失败/权限撤销；arm64 真机低内存与后台恢复。
- [ ] 固定真实扫描件记录 RSS、耗时、取消时延；确认缓存机制不会随页数无限增长。整体解析/单图峰值仍须实测。
- [ ] 草稿 Release 校验和完整，旧版本升级验签通过。
- [ ] 观察期从包含本批修复的候选版本开始重新计时，至少两周无阻塞回归。
- [ ] 更新单源版本到 1.0.0、Android versionCode 递增，生成变更说明并发布。

## 后续优化（不作为本轮开发完成条件）

以真实基准驱动独立进程隔离、路径/流 FFI、设备动态总内存预算、签名 PDF 行为提示、进一步模块拆分。Gradle 依赖校验与供应链长跑单独跟踪。任何相关能力未经测量，不宣称已有硬进程内存上限或立即取消保证。

## 实施记录

2026-10-02：六批开发实现完成，R01–R15 对应修改与本地回归已落地。旧审查报告保持原始证据，不改写为修复后的状态。

| 审查项 | 实施结果 | 验证证据 |
| --- | --- | --- |
| R01/R14 | `pdf-core::atomic_file` 统一 PDF/配置写入；临时文件对象仅清理自己；失败保留旧目标；预设读改写串行 | 原子替换/取消/同前缀文件、并发预设保存回归 |
| R13 | 序列化后、最终提交前检查取消；目标搜索候选只在内存验证，最终结果才写入 | 提交前取消、全部目标搜索和 CLI e2e 回归 |
| R07 | workers 拉取任务、主线程并行消费有界结果；每图缓存配额在返回前裁剪；Android 单图片 worker、64 MiB 保留缓存 | 背压/取消/失败回归、强制缓存逐出及重编码一致、既有多图 RSS 测试 |
| R11/R12/R15 | 等待原始 native Promise、冻结取消队列；保留路径大小写；版本化队列读取兼容旧数组、字段规范化 | cancel ACK 先返回、后续分析不启动、路径/存储测试 |
| R02/R03 | dataSync 专用权限；SAF IO dispatcher、已知大小检查与未知长度限量读取 | 最终 APK `aapt dump permissions`；限额与取消读入测试 |
| R04/R05 | 分析阶段密码重试；Kotlin CancellationException 恢复状态；generation 隔离旧任务 | ViewModel 密码、协程取消、旧任务覆盖测试 |
| R06/R10 | WorkManager 逐项 then；SAF 写入失败/空流删除本次 URI | 工作链依赖及失败写入测试 |
| R08/R09 | 校验和 job 设置 GH_REPO；AGP 按 variant 生成 JNI 库、验证 ABI，脚本使用 --locked | 双 ABI APK 校验；缺 NDK 预检退出 1；assemble dry-run 包含 buildRustDebug/buildRustRelease |

### 最终本地检查

- `bash scripts/quality-gate.sh` 全部通过：workspace 260 项通过、1 项忽略；核心默认库 191 项，可选 `jpx,cmyk-cms` 核心库 218 项；桌面壳两种 feature 组合各 18 项；前端 10 个文件、81 项通过，类型检查/构建通过。各 feature 轮次有重复测试，不能相加作为独立总数。
- `./gradlew testDebugUnitTest assembleDebug lintDebug --no-daemon` 通过：Kotlin 11 项（ViewModel 3、限量读取/输出 6、工作链 2），无失败/忽略。
- Android lint 0 error、23 warning：依赖版本、图标、未使用资源、复数文案与已有 API 常量提示；本轮未宣称 warning 清零。
- `python3 scripts/check-android-apk.py .../app-debug.apk` 通过，两个 ABI 均有非空 ELF 引擎；最终 APK 包含 `FOREGROUND_SERVICE_DATA_SYNC`。
- `./gradlew assemble --dry-run --no-daemon` 通过，同时包含 Debug/Release 原生任务；未执行 release APK 构建。
- `cargo fmt --all -- --check`、`git diff --check` 通过；Rust/TypeScript/Kotlin 公共生成绑定未变。
- 生成的可安装开发包：`android/app/build/outputs/apk/debug/app-debug.apk`。

### 验收边界与遗留

1. 本次为代码和本地开发验收。尚未运行新的 API 34+ 模拟器/arm64 真机、Windows/macOS 安装包、远程草稿 Release 或更新验签；上方发布门槛保留未勾选。
2. 缓存限额保证保留解码缓存不会随图片总数无限累计；输入对象图、序列化和单图瞬时分配不属于这一限额。独立进程隔离、设备总 RSS 硬预算仍属后续优化。
3. SAF 写入异常会尽力清理本次 URI，但 provider 不保证原子提交；系统硬杀进程后的恢复/去重仍需真机验证，不能承诺跨 provider 的 exactly-once 输出。
4. 原子写入失败保留旧目标；硬杀进程可能留下专属随机临时文件，不再扫描用户目录删除疑似残留。启动时清理机制需另行设计所有权协议。
5. 版本保持 `0.11.0` 开发树；在发布验收与观察期完成前，不标记正式 1.0.0 或创建发布 tag。
