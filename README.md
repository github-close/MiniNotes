<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/lockup-on-dark.png">
  <img src="assets/brand/lockup-horizontal.png" alt="MiniNotes" width="420">
</picture>

**托盘常驻的极简便签 · 单 exe 便携 · 零运行时依赖**

[![Rust](https://img.shields.io/badge/Rust-1.97%2B-FFC53D?logo=rust&logoColor=000)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-FFC53D?logo=windows&logoColor=000)](#)
[![Memory](https://img.shields.io/badge/RAM-~4MB-FFC53D)](#)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

</div>

---

全局热键一键唤起，失焦自动隐藏。两张页面：「**待办**」勾选清单 + 「**笔记**」左列表右编辑器。数据为纯文本文件（位置见「[下载](#下载)」）——备份就是拷走数据目录，卸载就是删掉它。

## 特性

- 🖇️ **托盘常驻**：点击托盘图标或全局热键（默认 `Ctrl+Alt+N`，可在设置中改）唤起；失焦 / `Esc` 自动隐藏，不打扰
- ✅ **待办页**：勾选清单，空格或点击切换完成状态；右键「填充底色」可给条目标记颜色（红橙黄绿青蓝紫七色跟随主题适配，悬停即弹出色板子菜单，选中项以描边提示）
- 📝 **笔记页**：Markdown 纯文本笔记，600ms 防抖自动保存，重命名 / 新建 / 删除，右键「置顶」固定重要笔记（行尾图钉标识，置顶组内仍可拖拽排序）
- 🎨 **四档主题**：浅色 / 暗色 / 浅黄 / 跟随系统，无边框自绘窗口；暗色下列表、滚动条、光标、右键/托盘菜单全套跟随
- ✏️ **编辑器字体可换**：设置里可为笔记输入区单独选字体族与字号（枚举全部系统字体，暗色下拉全跟随），标题栏 / 列表 / 待办页等其他界面字体不受影响
- 🧳 **零依赖免安装**：单 exe 直接运行，无数据库、无 WebView、无外部 DLL（开机自启仅写 `HKCU\...\Run` 一条）；数据全为纯文件——`notes/*.md`、`notes.order`、`notes.pinned`、`todos.json`、`config.ini`，位置见「[下载](#下载)」
- ⚙️ **可调**：热键捕获修改、开机自启（注册表 `HKCU\...\Run`）、失败自动降级备选热键并气泡提示
- 🔁 **单实例（整机唯一）**：同一 exe 重复启动自动聚焦已有窗口；检测到其他目录的副本在运行时会弹窗拒绝并给出路径，避免热键与数据混乱

## 下载

从 [Releases](https://github.com/github-close/MiniNotes/releases) 下载 `mininotes-vX.Y.Z-x64.zip`，**解压到任意目录后运行** `mininotes.exe`（不要在压缩包里直接双击）。

压缩包里只有 exe 和文档，不含任何数据文件——数据目录在**首次运行后**由程序自动生成，刚解压完看不到是正常的。位置只有一条判定规则：**exe 同目录下存在 `data\` 文件夹 → 数据就在这里（便携模式）；不存在 → 数据存放在 `%APPDATA%\MiniNotes\data`**（`%APPDATA%` 即 `C:\Users\你的用户名\AppData\Roaming`，在资源管理器地址栏输入 `%APPDATA%\MiniNotes` 回车即可直达）。

- **想让数据跟着 exe 走**：在 exe 同目录下新建一个 `data\` 文件夹再运行即可；要把现有数据带过去，先把 `%APPDATA%\MiniNotes\data` 里的内容拷进这个新建的文件夹
- **重置数据**：清空 `data\` 里的内容即可（文件夹本身留着，仍是便携模式）；把文件夹**整个删掉**则视为未启用便携，数据会重新生成在 `%APPDATA%\MiniNotes\data`
- **恢复默认（`%APPDATA%\MiniNotes\data`）**：备份好数据后，删掉 exe 同目录的 `data\` 文件夹再运行

## 构建

```bash
cargo build --release   # 产物: target/release/mininotes.exe
```

需要 Rust 1.97+（`x86_64-pc-windows-msvc`）与 MSVC 链接器，无其他依赖。

## 使用

1. 运行 `mininotes.exe`，托盘出现便签图标
2. `Ctrl+Alt+N`（或点击托盘图标）唤起窗口
3. 顶栏切换「待办 / 笔记」；右上角 ⚙ 打开设置（热键、主题、失焦隐藏、开机自启）
4. 数据目录定位见「[下载](#下载)」：便携模式随目录走，AppData 模式在 `%APPDATA%\MiniNotes\data`

## 架构

设计文档见 [docs/架构说明.md](docs/架构说明.md)：无边框自绘窗口、`static` 全局状态的消息重入安全、GDI+ 绘制、防抖保存等实现细节与关键设计决策。

品牌源文件在 [assets/brand/](assets/brand/README.md)。

## 开发说明

本项目由作者主导设计与验收，ZCode（GLM 编程助手）辅助实现、调试与测试。提交信息中以 `Co-authored-by: ZCode` 标注共同作者。

## 许可证

[MIT](LICENSE) © 2026 github-close
