# 品牌源文件

Logo 设计源文件。配色：便签黄 `#FFC53D` · 折角琥珀 `#E8A02B` · 墨色 `#2B2419`。

## 文件清单

| 文件 | 说明 | 用途 |
|---|---|---|
| `symbol-color.svg` | 主形：方形便签 + 右下折角 + 两行字迹 | 品牌符号母版 |
| `symbol-tilt-color.svg` | 主形微倾 4° 表情变体 | About / README 等大尺寸物料 |
| `symbol-on-tile.svg` | 便签黄圆角砖 + 白便签 | 应用图标（ico 48–256px 来源） |
| `tray-16.svg` | 16px 优化稿：折角加大、笔画像素对齐 | 托盘图标（ico 16–32px 来源） |
| `wordmark.svg` | 字标：手工构建字形（圆头笔画 26/110 网格），非字体 | 单独使用名字的场合，**不可打字重排** |
| `lockup-horizontal.svg` + `.png` | 横版组合 | README 头图、文档页眉 |
| `lockup-stacked.svg` | 竖版组合 | 正方形场合 |
| `lockup-on-dark.svg` + `.png` | 暗底全白横版 | GitHub 深色模式（`<picture>` 切换）、深色背景 |

## 重新生成 assets/icon.ico

`icon.ico` 内含 8 个尺寸，两种设计按场景拆分：

- **16 / 20 / 24 / 32 px** ← 渲染 `tray-16.svg`（托盘 / 资源管理器小图标）
- **48 / 64 / 128 / 256 px** ← 渲染 `symbol-on-tile.svg`（大图标场景）

各尺寸渲染为透明 PNG 后按 PNG-in-ICO 格式拼装（ICONDIR 头 + 16 字节目录项 + PNG 数据），任意 ICO 工具或十几行脚本均可完成。

## 使用规范速览

- 净空：四周 ≥ 便签高度的四分之一，随尺寸缩放
- 最小尺寸：符号 16px（托盘/favicon 必须用 `tray-16` 世代图形）；横版 120px 宽
- 彩色版只放白/浅灰底；深色底用 `lockup-on-dark`；不要放在黄色底上
- 不要拉伸、改配色、加特效、旋转主形（微倾表情除外）、打字重排字标
