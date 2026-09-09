# PDF 压缩器

[![CI](https://github.com/cosct/pdf-compressor/actions/workflows/ci.yml/badge.svg)](https://github.com/cosct/pdf-compressor/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/cosct/pdf-compressor.svg)](https://github.com/cosct/pdf-compressor/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

[English](README.md) | 简体中文

把 PDF 变小，但**不弄坏它**：文字仍是可搜索、可复制的文字，图片按你的质量要求重新编码，其余内容原样保留。所有处理都在你自己的电脑上完成——没有上传，没有账号，没有联网要求。

![主界面](docs/screenshots/app-main.png)

## 它能做什么

- **只动该动的部分**：压缩前先分析每个文件，判断体积花在哪里（图片？字体？冗余数据？），只优化真正占体积的内容；文字和线条永远不会被转成图片
- **图片压缩有真功夫**：按目标质量重新编码 JPEG、自动缩小超大图片的分辨率、彩色转灰度；黑白扫描件使用专为传真设计的无损压缩（G4），体积通常只有原来的零头
- **给邮件附件定预算**：目标大小模式直接说"压到 5MB 以内"，引擎自动在质量和分辨率之间找到能达标的组合，并聪明地分配——文字页少花预算、照片页多保细节
- **清理看不见的赘肉**：完全相同的重复数据只保留一份，编辑器留下的无用资源顺手清掉，内嵌的大字体裁剪为文档实际用到的字（对中文文档效果显著）
- **不冒险**：压缩结果不会比原文件更大（压不小就原样保留）；带密码的文件有明确的处理流程；原文件永远不会被覆盖
- **完全离线**：文件不出你的电脑，不收集任何数据
- **顺手**：图形界面支持多文件队列；命令行一行搞定批量任务；Windows / macOS / KDE / GNOME 的文件管理器里右键即可压缩

## 安装

| 平台 | 方式 |
| --- | --- |
| Windows | 下载 [Releases](https://github.com/cosct/pdf-compressor/releases) 中的安装包（`.exe`），自带命令行工具与资源管理器右键菜单 |
| macOS | 下载 [Releases](https://github.com/cosct/pdf-compressor/releases) 中的 `.dmg`（Intel / Apple Silicon 通用） |
| Arch Linux | AUR 安装：`yay -S pdf-compressor-bin`（二进制包）或 `yay -S pdf-compressor`（源码包） |
| 其他 Linux | 下载 [Releases](https://github.com/cosct/pdf-compressor/releases) 中的 `.AppImage` 或 `.deb` |
| 源码构建 | `pnpm install && pnpm run tauri build`（需要 Rust 1.93+ 与 Node.js 22+） |

## 快速上手

**图形界面**——把 PDF 拖进窗口，看一眼分析结果（预计能省多少、推荐用什么档位），点"开始压缩"。结果保存在原文件旁边，原名不变、绝不覆盖。

**命令行**——

```bash
pdf-compressor-cli analyze input.pdf                          # 先看看体积花在哪
pdf-compressor-cli compress input.pdf --preset maximum        # 压缩（最激进档）
pdf-compressor-cli compress input.pdf --target-size 5MB       # 压到 5MB 以内
pdf-compressor-cli quick 扫描件/ --no-notify                  # 整个目录批量、后台跑
curl -s https://例.com/big.pdf | pdf-compressor-cli compress - --stdout > small.pdf
                                                              # 接在管道里用，不落中间文件
```

命令行与图形界面共用同一个引擎，参数含义完全一致——详见[用户指南](docs/USER-GUIDE.zh-CN.md)。

## 它是如何工作的（一分钟版）

PDF 文件大，通常是因为图片存得比需要的大、嵌入了整套字体、或积累了多次编辑留下的冗余数据。本工具逐个检查这些内容，在**只改体积、不改外观**的前提下重新组织它们：图片重新编码到目标质量、重复数据合并、用不到的字体部分裁掉。文字、书签、链接和页面结构永远原样保留——所以压缩后的文件仍然可以正常搜索、复制和打印。

想了解每种参数的具体含义和背后的取舍？读[用户指南的参数详解](docs/USER-GUIDE.zh-CN.md#3-压缩参数详解)。

## 文档

| 文档 | 写给谁 |
| --- | --- |
| [用户指南](docs/USER-GUIDE.zh-CN.md) | 所有用户——安装、界面与命令行用法、右键集成、密码文档、常见问题、术语速查 |
| [更新日志](CHANGELOG.md) | 所有人——每个版本改了什么 |
| [开发指南](docs/DEVELOPMENT.md) | 贡献者——架构、代码约定、打包与维护备忘 |
| [测试指南](docs/TESTING.md) | 贡献者 / 测试——测试体系与质量门禁 |

## 参与贡献

欢迎提交 Issue 与 Pull Request。开发环境搭建、代码结构与提交规范见[开发指南](docs/DEVELOPMENT.md)；改动前请先跑一遍本地质量门禁（`pnpm gate`）。

## 许可证

[MIT](LICENSE)
