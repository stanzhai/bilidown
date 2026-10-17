<p align="center">
    <img src="assets/app.svg" alt="bilidown" width="120"/>
</p>

<h1 align="center">bilidown</h1>

<p align="center">
    A cross-platform video downloader for Bilibili (Rust rewrite)
</p>

<p align="center">
    <img src="https://img.shields.io/github/v/release/stanzhai/bilidown?style=flat-square" alt="Release"/>
    <img src="https://img.shields.io/github/license/stanzhai/bilidown?style=flat-square" alt="License"/>
    <img src="https://img.shields.io/github/downloads/stanzhai/bilidown/total?style=flat-square" alt="Downloads"/>
    <img src="https://img.shields.io/github/stars/stanzhai/bilidown?style=flat-square" alt="Stars"/>
    <img src="https://img.shields.io/github/actions/workflow/status/stanzhai/bilidown/rust.yml?style=flat-square" alt="Build"/>
</p>

<p align="center">
    <a href="https://github.com/stanzhai/bilidown/releases">Downloads</a> ·
    <a href="README.md">简体中文</a>
</p>

## 📖 Introduction

**bilidown** is a cross-platform desktop application for downloading videos from Bilibili. It is a community fork of [Bili23 Downloader](https://github.com/ScottSloan/Bili23-Downloader), rewritten from scratch in Rust with a [gpui-kit](https://github.com/longbridge/gpui-kit) native UI. All user data from the Python build (config, task database, download history) carries over seamlessly.

Parse a shared link or video ID, then pick exactly which episodes, quality and extra content to save.

## ⚡ Features

| Feature | Details |
| :--- | :--- |
| 🖥️ **Cross-platform** | **Windows 10 1809+**, **Linux** (x64) and **macOS 12+**. |
| 🎨 **Native UI** | GPU-rendered native interface built on [gpui-kit](https://github.com/longbridge/gpui-kit); light/dark themes, HiDPI-native, remembers window position and size across launches. |
| 🚀 **Multi-threaded** | Parallel multi-threaded downloads, resume support, automatic retry on network errors, and a global speed limit. |
| 🔗 **Rich link types** | Videos, bangumi, movies, courses, interactive videos, audio (au/am), user spaces, favourites, weekly picks, collections, watch progress, watch later and history. |
| 🔍 **Batch & search** | Parse multiple video links at once; keyword search across spaces, favourites, watch later and history (current page or everything); auto-select items by preset rules after parsing. |
| ⚙️ **A/V options** | **Video**: up to `8K`, `4K`, `HDR`, Dolby Vision <br>**Audio**: `Hi-Res`, Dolby Atmos <br>**Codec**: `AVC`, `HEVC`, `AV1`<br>Preview exactly what will be downloaded before starting. |
| 💬 **Danmaku & subtitles** | **Danmaku**: `xml`, `ass`, `json`<br>**Subtitles**: `srt`, `lrc`, `txt`, `ass`, `json`<br>Custom styles, subtitle language selection, and `ass` embedding into `mkv`. |
| 🖼️ **Covers & chapters** | Lossless covers (`jpg`, `png`, `avif`, `webp`), optional cover embedding into the final video (with original deletion), and chapter metadata. |
| 🧩 **NFO metadata** | Scrapes and generates NFO metadata for **Kodi**, **Jellyfin**, **Emby** and other media centers. |
| 📁 **Naming rules** | A rule engine with per-type naming templates and multi-level directory layouts for every supported link type. |
| 📦 **Container muxing** | Smart stream extraction and muxing to `mp4` or `mkv`. |
| 🚫 **Duplicate detection** | Download history warns about and can skip already-downloaded content. |
| 🌐 **Network & proxy** | No proxy / system proxy / manual proxy, CDN selection by region, custom User-Agent and Host. |
| 📋 **Clipboard watch** | Detects Bilibili links copied to the clipboard and offers to parse them. |
| 🤖 **MCP server** | Built-in MCP (Model Context Protocol) server so AI clients can parse links, inspect episodes and create download tasks. |
| 🌍 **i18n** | Simplified Chinese, Traditional Chinese and English out of the box. |
| 🔒 **Secure login** | QR code, SMS verification code and cookie login. |
| 📖 **Free & open source** | **GPL-3.0** licensed, no ads, no in-app purchases. |

## 📥 Downloads

- [**GitHub Releases**](https://github.com/stanzhai/bilidown/releases/latest) — Windows installer / portable, macOS dmg (Intel & Apple Silicon), Linux AppImage.

The Windows portable build and the AppImage bundle ffmpeg; the installer includes it too.

## 🛠️ Building from source

```bash
git clone https://github.com/stanzhai/bilidown.git
cd bilidown
cargo build --release -p app --manifest-path crates/Cargo.toml
```

Release artifacts are produced by `crates/packaging/src/bin/packager.py`; see `.github/workflows/rust-release.yml` for the full CI pipeline.

## 🪧 Terms of use
This project is for personal study and research only. Downloaded content is **limited to personal, non-commercial use; commercial use, public redistribution or resale is strictly prohibited**.  
The software operates only within the permissions of the user's own account and **does not bypass any paywall or platform DRM**. Do not use it for bulk scraping or anything that violates the platform's terms of service.  

**Disclaimer**: users assume all risk arising from the use of this project (including but not limited to account suspension and copyright disputes). The developers accept no liability for any direct or indirect damages caused by using or being unable to use this software.

Continuing to use the software means you have read and accepted all of the above.

## 🌱 Fork origin

This project is forked from [ScottSloan/Bili23-Downloader](https://github.com/ScottSloan/Bili23-Downloader) — full credit to the original project and its community. API documentation and signing implementations reference [SocialSisterYi/bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect).

## 🔑 License
Released under the **GPLv3 License**.

wbi signing, several API endpoints and buvid3 generation reference [SocialSisterYi/bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect)  

## 🛠️ Contributing
Issues and pull requests are welcome.

<a href="https://github.com/stanzhai/bilidown/graphs/contributors" target="_blank">
    <img src="https://contrib.rocks/image?repo=stanzhai/bilidown" alt="Contributors" style="width: 300px;"/>
</a>

Made with [contrib.rocks](https://contrib.rocks).
