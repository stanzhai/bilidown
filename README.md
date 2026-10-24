<p align="center">
    <img src="assets/app.svg" alt="bilidown" width="120"/>
</p>

<h1 align="center">bilidown</h1>

<p align="center">
    跨平台 Bilibili 视频下载工具（Rust 重写版）
</p>

<p align="center">
    <img src="https://img.shields.io/github/v/release/stanzhai/bilidown?style=flat-square" alt="Release"/>
    <img src="https://img.shields.io/github/license/stanzhai/bilidown?style=flat-square" alt="License"/>
    <img src="https://img.shields.io/github/downloads/stanzhai/bilidown/total?style=flat-square" alt="Downloads"/>
    <img src="https://img.shields.io/github/stars/stanzhai/bilidown?style=flat-square" alt="Stars"/>
    <img src="https://img.shields.io/github/actions/workflow/status/stanzhai/bilidown/rust.yml?style=flat-square" alt="Build"/>
</p>

<p align="center">
    <a href="https://github.com/stanzhai/bilidown/releases">下载发布版本</a> ·
    <a href="README_en.md">English</a>
</p>

## 📖 项目简介

**bilidown** 是一个跨平台桌面应用程序，用于下载 Bilibili 上的视频资源。它是 [Bili23 Downloader](https://github.com/ScottSloan/Bili23-Downloader) 的社区分支，使用 Rust 与 [gpui-kit](https://github.com/longbridge/gpui-kit) 从零重写了整个 GUI 与下载内核，保留原项目的全部数据（配置、任务数据库、下载历史可与 Python 版无缝衔接）。

支持解析分享链接或视频 ID，自由选择要下载的分 P、画质与附加内容（弹幕、字幕、封面、元数据等）。

## ⚡ 程序特性

| 特性 | 详细说明 |
| :--- | :--- |
| 🖥️ **跨平台支持** | 支持 **Windows 10 1809+**、**Linux**（x64）和 **macOS 12+** 三大桌面操作系统。 |
| 🎨 **原生 UI 设计** | 基于 [gpui-kit](https://github.com/longbridge/gpui-kit) 构建的原生 GPU 渲染界面，支持浅色 / 深色主题无缝切换，原生适配高分屏，并会记住窗口位置与大小，下次启动自动恢复。 |
| 🚀 **多线程与加速**| 原生集成多线程并行下载、断点续传及网络异常自动重试机制，提供极致的下载速率，同时支持全局下载限速。 |
| 🔗 **多类型解析** | 全面支持：`投稿视频`、`番剧`、`电影`、`课程`、`互动视频`、`音乐（au/am）`、`UP主空间`、`收藏夹`、`每周必看`、`订阅合集`、`追番追剧`、`稍后再看`、`历史记录`等。 |
| 🔍 **批量与检索** | 支持一次粘贴多条投稿视频链接**批量解析**；个人空间、收藏夹、历史记录与稍后再看支持**关键词搜索**（可选择只筛选当前页或搜索全部内容）；解析完成后还可按预设条件**自动勾选**下载项。 |
| ⚙️ **音视频自定义**| **画质**：`8K`、`4K`、`HDR`、`杜比视界`等 <br>**音质**：`Hi-Res 无损`、`杜比全景声`等 <br>**编码**：`AVC`、`HEVC`、`AV1`<br>下载前可实时预览本次将要下载的媒体内容。 |
| 💬 **弹幕与字幕** | **弹幕**：`xml`、`ass`、`json`<br>**字幕**：`srt`、`lrc`、`txt`、`ass`、`json`<br>支持自定义弹幕与字幕样式、指定字幕语言，并可将 `ass` 格式的弹幕和字幕嵌入 `mkv` 容器中。 |
| 🖼️ **封面与章节**| 无损保存原图质量（`jpg`、`png`、`avif`、`webp`），原生支持将封面嵌入最终的视频文件（可选嵌入后删除原图），并支持写入视频的**章节信息**。 |
| 🧩 **NFO 元数据** | 自动刮削并生成符合 **Kodi**、**Jellyfin**、**Emby** 等媒体中心标准格式的本地媒体元数据。 |
| 📁 **分类与命名** | 内置强大规则引擎，可为投稿视频、番剧、课程、互动视频、收藏夹、个人空间、历史记录、稍后再看、每周必看、音乐等类型分别设置命名模板与多级目录分类存储模式。 |
| 📦 **封装格式转化**| 智能音视频流混合提取，支持封装输出为 `mp4` 或 `mkv`，充分满足不同播放设备的兼容需求。 |
| 🚫 **重复下载检测**| 自动记录下载历史，再次下载相同内容时给出提示并可直接跳过，避免重复占用带宽与磁盘空间。 |
| 🌐 **网络与代理** | 支持**不使用代理**、**使用系统代理**、**手动设置**三种代理模式，可根据地理位置选择 CDN 节点以改善下载速度，并支持自定义 User-Agent 与 Host。 |
| 📋 **剪贴板监控** | 自动识别复制到剪贴板中的 B 站链接，确认后即可直接进入解析下载流程。 |
| 🤖 **MCP 服务器** | 内置 MCP（Model Context Protocol）服务器，可供 AI 客户端解析链接、查询分 P 并创建下载任务。 |
| 🌍 **国际化支持** | 内置多语言界面，开箱可用：简体中文、繁体中文、English。 |
| 🔒 **账号安全登录**| 支持快捷安全的**扫码登录**、**短信验证登录**与 **Cookie 登录**。 |
| 📖 **完全开源免费**| 基于 **GPL-3.0** 协议发布，代码完全开源、无内购、无广告，拥抱社区共建。 |

## 📥 下载地址

- [**GitHub Releases**](https://github.com/stanzhai/bilidown/releases/latest) - Windows 安装包 / 便携版、macOS dmg（Intel & Apple Silicon）、Linux AppImage。

Windows 便携版与 AppImage 内置 ffmpeg；安装版亦自动附带，无需单独配置。

## 🛠️ 从源码构建

```bash
git clone https://github.com/stanzhai/bilidown.git
cd bilidown
cargo build --release -p app --manifest-path crates/Cargo.toml
```

打包产物（安装包 / dmg / AppImage）由 `crates/packaging/src/bin/packager.py` 生成，CI 全流程见 `.github/workflows/rust-release.yml`。Windows 安装包安装到 `Program Files\bilidown`（独立于旧版 Bili23 Downloader，升级互不干扰）。

平台图标说明：

- **Windows**：`crates/app/build.rs` 通过 [winresource](https://crates.io/crates/winresource) 把 `crates/app/assets/app.ico`（资源 ID 1，gpui 窗口类加载处）和 DPI 感知清单嵌入 `bilidown.exe`，标题栏 / 任务栏图标即来自该资源；release 构建以 `windows` 子系统链接，启动不再弹出控制台窗口（debug 构建保留控制台便于看日志）。
- **macOS**：`assets/app.icns` 经 packager 写入 `Resources/app.icns` 并登记 `CFBundleIconFile`。
- **Linux**：X11 窗口图标内嵌自 `crates/app/assets/tray-icon.png`（与 `assets/app.svg` 同源）。

`app.ico` / 各尺寸 PNG 由 `assets/app.svg` 重新生成（需要 `rsvg-convert` 与 Python Pillow）：

```bash
for s in 256 128 64 48 32 16; do rsvg-convert -w $s -h $s assets/app.svg -o app_$s.png; done
python3 -c "from PIL import Image; imgs=[Image.open(f'app_{s}.png').convert('RGBA') for s in (256,128,64,48,32,16)]; imgs[0].save('crates/app/assets/app.ico', format='ICO', append_images=imgs[1:], sizes=[(i.width,i.height) for i in imgs])"
```

## 🪧 使用协议
本项目仅供个人学习与研究用途，下载内容**仅限于个人非商业使用，严禁用于任何形式的商业目的、公开传播或分发**。  
本软件仅基于用户账号的合法访问权限操作，**不会绕过任何付费墙或平台知识产权保护措施**。请勿将本软件用于批量抓取或任何违反目标平台服务条款的行为。  

**免责声明**：用户需完全自行承担使用本项目可能带来的所有风险（包括但不限于账号封禁、版权纠纷等）。项目开发者不对任何人因使用或无法使用本软件所引发的任何直接或间接法律纠纷、损害承担责任。  

继续使用即表示您已充分理解并同意遵守上述全部条款。

## 🌱 分支来源

本项目 fork 自 [ScottSloan/Bili23-Downloader](https://github.com/ScottSloan/Bili23-Downloader)，并向原项目及其社区致敬；接口文档与签名实现参考 [SocialSisterYi/bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect)。

## 🔑 开源许可
本项目在 **GPLv3 License** 许可协议下进行发布。

wbi 签名、部分接口以及 buvid3 等参数生成参考 [SocialSisterYi/bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect)  

## 🛠️ 参与贡献
欢迎提出 Issue 与 Pull Request。

<a href="https://github.com/stanzhai/bilidown/graphs/contributors" target="_blank">
    <img src="https://contrib.rocks/image?repo=stanzhai/bilidown" alt="Contributors" style="width: 300px;"/>
</a>

Made with [contrib.rocks](https://contrib.rocks).
