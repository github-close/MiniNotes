<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/lockup-on-dark.png">
  <img src="assets/brand/lockup-horizontal.png" alt="MiniNotes" width="420">
</picture>

**托盘常驻的极简便签 · 单 exe 便携 · 零运行时依赖**

[![Rust](https://img.shields.io/badge/Rust-1.97%2B-FFC53D?logo=rust&logoColor=000)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-FFC53D?logo=windows&logoColor=000)](#)
[![exe](https://img.shields.io/badge/single%20exe-~220KB-FFC53D)](#)
[![Memory](https://img.shields.io/badge/RAM-~4MB-FFC53D)](#)

</div>

---

全局热键一键唤起，失焦自动隐藏。两张页面：「**待办**」勾选清单 + 「**笔记**」左列表右编辑器。数据是 exe 旁的纯文件——拷走目录即完成备份，删掉目录即完全卸载。

## 特性

- 🖇️ **托盘常驻**：点击托盘图标或全局热键（默认 `Ctrl+Alt+N`，可在设置中改）唤起；失焦 / `Esc` 自动隐藏，不打扰
- ✅ **待办页**：勾选清单，空格或点击切换完成状态
- 📝 **笔记页**：Markdown 纯文本笔记，600ms 防抖自动保存，重命名 / 新建 / 删除
- 🎨 **三档主题**：浅色 / 暗色 / 跟随系统，无边框自绘窗口；暗色下列表、滚动条、光标、右键/托盘菜单全套跟随
- 🧳 **完全便携**：单 exe（约 220KB），数据在 `data/` 下（`notes/*.md`、`todos.json`、`config.ini`），无数据库、无 WebView、无外部 DLL
- ⚙️ **可调**：热键捕获修改、开机自启（注册表 `HKCU\...\Run`）、失败自动降级备选热键并气泡提示
- 🔁 **单实例**：重复启动自动聚焦已有窗口

## 构建

```bash
cargo build --release   # 产物: target/release/mininotes.exe
```

需要 Rust 1.97+（`x86_64-pc-windows-msvc`）与 MSVC 链接器，无其他依赖。

## 使用

1. 运行 `mininotes.exe`，托盘出现便签图标
2. `Ctrl+Alt+N`（或点击托盘图标）唤起窗口
3. 顶栏切换「待办 / 笔记」；右上角 ⚙ 打开设置（热键、主题、失焦隐藏、开机自启）
4. 数据都在 exe 旁的 `data/` 目录，随目录走

## 架构

设计文档见 [docs/架构说明.md](docs/架构说明.md)：无边框自绘窗口、`static` 全局状态的消息重入安全、GDI+ 绘制、防抖保存等实现细节与关键设计决策。

品牌源文件在 [assets/brand/](assets/brand/README.md)。
