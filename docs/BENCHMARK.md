# 引擎基准与回归门槛（2026-10-18 建立）

本文件是 `docs/PLAN-1.0.md` 发布门槛第 3 项（固定真实扫描件记录 RSS、
耗时、取消时延）的执行记录，也是 `docs/REVIEW-2026-10-02.zh-CN.md` §5.3
后续优化的度量基座。**先测量再优化**：任何性能 PR 先跑 `--check`。

## 方法与边界

- 测量对象是真实发布路径：`cargo build --release` 的 `pdf-compressor-cli`
  管道模式（`compress - --stdout`），不经任何 mock。
- 峰值内存取子进程 `/proc/<pid>/status` 的 **VmHWM**（高水位，20 ms 采样），
  不是函数返回后的缓存计数。耗时为墙钟。
- 每个主夹具：**冷启动单列**，随后 5 次热跑取 p50/p95；缩放与目标场景
  1 次预热 + 3 次测量。并发场景标注为受负载测量，不作正式基线。
- 结果 JSON：`target/bench/results.json`（含环境、逐夹具数据）。
- 单机单数据，不做跨机宣称；输入哈希见 `target/bench/corpus/manifest.json`，
  换机重跑前先对哈希（编码器版本不同则只作本机基线）。

## 语料（`scripts/bench/corpus.py`，确定性种子）

| 夹具 | 内容 | 规模 |
| --- | --- | --- |
| scan-photo-small | 照片感扫描页（渐变+噪声 JPEG q95） | 3 页 1600×2200 |
| scan-photo-medium | 同上 | 10 页 2400×3200 |
| scan-photo-large | 同上 | 20 页 2400×3200 |
| scan-bw-medium | 近黑白文本扫描（深色笔画+纸噪） | 10 页 2000×2800 |
| mixed-medium | 图文混排 | 6 页 1600×1200 |
| text-small | 纯文本（无嵌入图） | 5 页 |
| scale-01/05/10/20/40 | **每页唯一图**（防 dedup 塌缩工作量） | 1/5/10/20/40 页 2400×3200 |
| corrupt | `%PDF-` 头 + 2 MB 随机字节 | 必须干净报错 |

## 复现

```bash
scripts/bench.sh            # 构建 release CLI + 取消探针，语料缺失则生成，全量测量
scripts/bench.sh --check    # 与 scripts/bench/baseline.json 比，>10% 触发人工复核（exit 2）
python3 scripts/bench/corpus.py   # 仅重生成语料
```

取消时延探针：`crates/pdf-core/examples/cancel_latency.rs`——真实公共
bytes API + watchdog 置取消位，记录「置位 → 返回」的时延；在取消点前
完成的运行记 `latency=null`，不计入时延统计。

## 基线（本机，2026-10-18，release，默认 feature）

环境：Linux x86_64，16 线程逻辑 CPU，15 GiB RAM，rustc 1.98.1，基线提交
`1f8dc3c`（精确信息见 results.json 的 environment）。

### 主夹具

| 夹具 | 冷启动 ms | 热 p50 ms | 热 p95 ms | 峰值 RSS MiB |
| --- | ---: | ---: | ---: | ---: |
| scan-photo-small | 128 | 116 | 142 | 86 |
| scan-photo-medium | 800 | 613 | 632 | 459 |
| scan-photo-large | 1070 | 1072 | 1364 | 625 |
| scan-bw-medium | 225 | 240 | 244 | 284 |
| mixed-medium | 79 | 83 | 90 | 92 |
| text-small | 3 | 3 | 3 | 1 |
| corrupt（exit 1，干净报错） | 11 | - | - | 1 |

### 缩放（唯一页，每页解码工作量恒定）

| 页数 | 输入 MiB | 热 p50 ms | 峰值 RSS MiB | 扣 I/O 后驻留* |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 5 | 141 | 66 | 55 |
| 5 | 26 | 306 | 289 | 237 |
| 10 | 52 | 520 | 468 | 363 |
| 20 | 104 | 928 | 620 | 411 |
| 40 | 208 | 1748 | 813 | 397 |
| 80 | 416 | 3376 | 1224 | 391 |

\* 近似值：`峰值 RSS − 2×输入`（输入字节 + 输出/原始缓冲的粗估）。

**结论（发布门槛第 3 项的核心证据）**：热耗时随页数线性增长（正常），
而扣除 I/O 缓冲后的引擎驻留在 20 页后稳定在 **约 400 MiB**——不随页数
继续增长，与 R07 的驻留缓存裁剪/逐出机制一致。首版缩放语料曾用同页
图片，被 dedup 塌缩了工作量；现语料每页唯一图，`imagesDeduplicated=0`。

### 目标大小 / 并发 / 取消

- 目标 2 MB（scan-photo-large）：p50 7067 ms，峰值 1554 MiB，输出
  2 039 590 字节达标（Q19）。**目标搜索的整轮峰值明显高于普通压缩**——
  768 MiB 驻留缓存预算之外还有输入/输出缓冲与单图解码瞬态，见
  PLAN-1.0「验收边界与遗留」第 2 条。
- 并发（large + bw，受负载）：932 / 1160 ms，峰值 576 / 276 MiB。
- 取消（250 ms 后置位，3 次）：时延 53–80 ms，均返回 `Cancelled`，
  无输出提交。

## 回归政策

- 同机同语料：热 p50 耗时或峰值 RSS 相对基线增长 **>10% → 人工复核**
  （`--check` exit 2），不直接断言不可发布。
- 新夹具加入时同步更新基线与本文；基线 JSON 为
  `scripts/bench/baseline.json`。
