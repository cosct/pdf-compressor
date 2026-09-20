# 1.0 开发计划：成熟度声明（工作文档，2026-09-20 立项）

> 1.0 不是功能版本——是兼容性承诺的生效点与对外稳定声明（定位见
> DEVELOPMENT.md §8「1.0 计划」）。本文件是落地工作文档：触发条件状态、
> 内容清单进度、冻结前决策窗口、升级走查与发布 checklist。

## 1. 触发条件状态（2026-09-20 盘点）

| 条件 | 状态 |
| --- | --- |
| 0.10.0（2026-09-16 发布）后 ≥2 周无兼容性/正确性回报 | ⏳ 第 4 天；且 0.11.0「审查收口 + 接口冻结准备」批次待发布（含 §3 前置收口清单全部落地）——按「出现 0.10.x patch 则重新计时」的精神，0.11.0 发布后重新起算，**最早 1.0 窗口 ≈ 0.11.0 发布 + 2 周** |
| 信号源（GitHub issues / release 评论 / AUR 评论） | 全零（无负信号，样本小） |
| 兼容性盘点缺口清零 | 0.10.0 P1 补钉后无已知缺口；0.11.0 又收紧一层（错误码 taxonomy 双侧钉死随批次更新为 11 码） |

**版本策略**：0.11.0 尚未 tag——§3 的前置收口项**并入 0.11.0**（同属
「审查收口」主题，无需新开 0.12），发布后起算观察期，1.0 纯声明殿后。
若跳过 0.11.0 直发 1.0.0，则 1.0 携带功能外内容（偏离「1.0 零功能」
叙事）——不采用。

## 2. 内容清单（全部非功能）

| # | 内容 | 状态 |
| --- | --- | --- |
| 1 | README（双语）Stability 节：兼容面五项清单、破坏性变更只随 major + 弃用窗口、默认值不冻结边界 | ✅ 已落地（本批次） |
| 2 | 弃用流程写入 DEVELOPMENT.md §8（双写 + deprecation 通知 ≥1 minor，major 移除，配置类走迁移链） | ✅ 已落地（本批次） |
| 3 | 升级走查：0.9/0.10 → 1.0 updater 路径实测 + 五类产物新装走查 | ⏳ 发布时执行（§4 清单就绪） |
| 4 | CHANGELOG 1.0 条目：成熟度声明 + 0.x 行为变化台账引用 + 0.5→1.0 里程碑回顾 | ⏳ 发布时定稿（0.x 台账已在 §8 与 CHANGELOG 各版本节） |
| 5 | 面清单的机械化防线确认：bindings 再生比对（CI diff 门禁）、错误码双侧钉、cli_e2e、迁移链测试 | ✅ 均已在 CI/质量门（0.11.0 批次补齐 bindings diff 门禁） |

## 3. 1.0 前置收口清单（2026-09-20 拍板：剩余事项全部纳入，0.11.0 落地）

冻结前窗口的四个决策已定夺：**全部纳入、在 0.11.0 内完成**（CLI 两个
panic 已随 `9968325` 修复；其余各项如下）。1.0 冻结的是「现有接口的语义」，
新码/新命令后续仍可追加（不破坏），因此这些必须在 tag v1.0.0 之前落地——
之后同类变更只能走 major + 弃用窗口。

### 3.1 实施项（0.11.0，改动接口语义/签名）

| # | 事项 | 方案要点 | 验收 |
| --- | --- | --- | --- |
| A1 | **CLI quick 模式部分失败退出码** | `files_failed > 0 → 非零`（与全失败同 FAILURE 即可，语义简单可预测）；cli_e2e 现行钉同步翻转；CHANGELOG「行为变更」节 + 用户指南显著标注（0.x 内允许） | e2e 双向钉（部分失败/全失败/全成功三态）；文档同步 |
| A2 | **capabilities 收窄** | `core:default` 拆为实际使用的精确权限集（event/window/webview/app 级别的最小集合，去掉 image/menu/tray/path/resources）；`windows: ["main"]` 对自定义命令无效的事实写入注释（app ACL manifest 不引入——维护成本大于收益，webview 不加载远程内容的前提下风险可接受） | 前端全功能回归（对话/更新/重启/窗口控制逐项）；capabilities 注释说明取舍 |
| A3 | **同步命令 async 化**（0.11.0 的 4.6 转正） | `save_preset_user_config`（sync_all 阻塞）、`reveal_path_in_folder`（D-Bus/child.wait）、`existing_paths`（逐路径 exists）三个改 `async fn` + `spawn_blocking`；其余只读轻命令维持同步（避免无谓 churn） | bindings 再生零漂移由 CI diff 把关；主线程无阻塞 IO（手动走查设置保存/打开目录） |
| A4 | **分析取消** | `analyze_pdf` 接入 `CompressionTaskRegistry`（与压缩同款 cancel_flag + taskId），前端分析阶段取消按钮生效 | 取消路径单测 + GUI 手动走查 |

### 3.2 调查/验证项（0.11.0 发布前置）

| # | 事项 | 方案要点 | 验收 |
| --- | --- | --- | --- |
| B1 | **CMYK APP14 反转调查**（PLAN-0.11.0 §6.7 遗留） | 造「APP14 transform=0 且无 /Decode」夹具（hex 构造 Adobe 标记或 Photoshop 产出），与 poppler 渲染对齐像素级判定；二选一结论：补 APP14 解析（带 PSNR 钉子）或以证据文档化现状约定正确（修 cmyk.rs 两处矛盾注释） | 夹具 + 像素级对比记录；注释矛盾消除 |
| B2 | **变异 campaign 全量补跑** | 0.11.0 终态跑 `cargo mutants`，结果记 docs/mutation-log.md；漏杀变异体当日补测 | mutation-log 更新；0 漏或漏杀全有对应测试 |
| B3 | **jpx-indexed.jp2 再生配方** | 扩展 make-jpx-fixtures.sh 生成等价 indexed JP2 夹具（jpeg2k/opj_compress 或 Python glymur/Azure 工具链）；无法等价再生则降级为 TESTING.md 声明「历史遗留夹具，无再生路径」 | 脚本可重跑产出字节等价夹具，或显式声明 |
| B4 | **OpenJPEG 升级复查** | tag v1.0.0 前复查 openjpeg-sys 是否已发布 vendored ≥2.5.4 的版本；是则升级 + 全门禁回归（依赖健康节预案）；否则维持受控追踪不变 | 复查结论记录于依赖健康节（日期 + 版本） |

### 3.3 明确不做（记录在案，防再议）

- **app ACL manifest**（build.rs 命令属性）：收益是 `windows:["main"]` 对
  自定义命令生效，代价是每次增删命令要同步 manifest；在 webview 不加载
  远程内容、splash 也受信的前提下不引入。若未来加载远程内容则必须重评。
- **安卓 / TypeScript 7**：维持「1.x 首项 / 跟踪 vue-tci」既有结论。

## 4. 升级走查清单（发布 1.0 时执行并记录到本节）

1. **updater 路径**：装 0.9.x 与 0.10.x 各一台（或容器），配置 updater
   端点指向 1.0 的 `latest.json`，验证 `remote > current` 的 semver 判定
   推送 1.0 并验签安装（语义已核实 2026-09-16，重验签名链）。
2. **五类产物新装走查**（每类：全新环境安装 → 拖入含图 PDF 压缩 → 打开
   产物 → 右键/快速压缩入口各一次）：
   - Windows NSIS `.exe`（含 Explorer 右键菜单 + CLI 分发）
   - macOS `.dmg`（universal；Gatekeeper 放行提示按 README 签名说明）
   - Linux `.deb` / `.AppImage`
   - Arch `.pkg.tar.zst`（AUR -bin 与源码包各一）
3. **跨版本队列/配置恢复**：0.10 的 localStorage 队列与 v3 配置在 1.0
   下恢复无丢失（迁移链测试已钉，走查做端到端确认）。
4. 走查结果（日期、环境、通过项）记录回本节；任何失败项阻塞发布。

## 5. 发布 checklist（1.0.0）

- [ ] 观察期满且信号源无回归报告（观察期自 0.11.0——含 §3 前置收口
      清单全部落地——发布之日起算）；
- [ ] §3.1 四项实施完毕（随 0.11.0 发布），§3.2 四项调查/验证结论入档；
- [ ] §4 走查完成并记录；
- [ ] `pnpm sync-version` 升 1.0.0（五处一致，release 前置 job 会断言）；
- [ ] CHANGELOG 定稿 1.0 条目（成熟度声明引用 README Stability 节 +
      0.x 行为变化台账 + 里程碑回顾）；
- [ ] tag `v1.0.0` → Release 工作流（版本断言/构建/NSIS 冒烟/SHA256SUMS/
      AUR）全绿；
- [ ] 发布后：DEVELOPMENT.md §8 把 1.0 从计划移入历史收口；1.x 路线
      （安卓 spike → 立项）启动。

## 6. 1.0 之后的路标（已在 §8 定调，此处仅索引）

- **安卓**：1.x 首项，独立立项，spike 先行（tauri android init +
  jpx/cmyk-cms NDK 编译 + 字节入口跑夹具，1-2 天）。
- **TypeScript 7**：跟踪 vue-tsc 适配（依赖 issue 保持开放）。
- 其余 issue 驱动。
