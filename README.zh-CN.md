# PDF 压缩器

[English](README.md) | 简体中文

本地优先的 PDF 瘦身桌面应用——不破坏重要的东西：文字仍是文字，矢量仍是矢量，只优化真正占体积的部分（图片、字体、冗余对象）。

![主界面](docs/screenshots/app-main.png)

## 为什么选它

- **选择性压缩，不是盲目重写**：先分析分类每个文档并推荐预设；优化器只重写可证明安全的内容（绝不栅格化文本和矢量指令）
- **认真的图片编解码**：SIMD JPEG 重编码、下采样、灰度转换、黑白扫描件无损 CCITT G4；G3/G4 传真件输入可直接解码转码
- **结构级无损优化**：字节相同的流对象去重、未引用的字体/XObject 资源清理、内嵌 CID TrueType 字体按用到的字形子集化（可选）
- **目标大小模式**：给一个字节预算（比如邮件附件 5MB 以内），自动搜索质量/分辨率参数直到达标——并按图片内容细节分配质量
- **构造即安全**：需要密码的加密文档有明确处理（弹窗输入或 `--password`），owner 密码件干净解锁，压不小于原文件的结果绝不落盘
- **本地优先**：不上传、无遥测，一切都在你的机器上
- **批量友好**：GUI 多文件队列、CLI 一行命令、Windows 资源管理器 / macOS 访达 / KDE Dolphin / GNOME 文件右键集成

## 安装

| 平台 | 来源 |
| --- | --- |
| Arch Linux | AUR：`yay -S pdf-compressor-bin`（二进制包，源为 [Releases](https://github.com/cosct/pdf-compressor/releases) 资产；本地自建可用 `pnpm run tauri:arch`） |
| Windows | [Releases](https://github.com/cosct/pdf-compressor/releases) 的 NSIS 安装包（内置 CLI 与资源管理器右键菜单） |
| macOS | [Releases](https://github.com/cosct/pdf-compressor/releases) 的 `.dmg`（可选装访达快速操作，见用户指南） |
| 任意平台 | 源码构建：`pnpm install && pnpm run tauri build`（Rust 1.93+、Node 22+） |

## 快速上手

**图形界面**——拖入 PDF，看一眼分析结果，点“开始压缩”。输出写在原文件旁边：`原名__optimized-<预设>.pdf`。

**命令行**——分析、压缩、右键同款后台模式：

```bash
pdf-compressor-cli analyze input.pdf
pdf-compressor-cli compress input.pdf --preset maximum
pdf-compressor-cli compress input.pdf --target-size 5MB   # 压到预算以内
pdf-compressor-cli quick input.pdf --bilevel g4            # 后台压缩 + 桌面通知
```

## 文档

| 文档 | 适合谁 |
| --- | --- |
| [用户指南](docs/USER-GUIDE.zh-CN.md) | 所有用户——安装、界面与命令行用法、右键集成、密码文档处理、常见问题 |
| [更新日志](CHANGELOG.md) | 所有用户——版本历史 |
| [开发指南](docs/DEVELOPMENT.md) | 贡献者——架构、约定、打包、维护者备忘 |
| [测试指南](docs/TESTING.md) | 贡献者 / 测试——测试体系、质量门禁、模糊与变异测试 |

## 技术栈

Rust 引擎（`lopdf`、`jpeg-encoder`、`fax`、`subsetter`、SIMD 缩放）· Vue 3 + TypeScript · Tauri 2 · MIT 协议

输出行为、已知限制与故障排查见用户指南。
