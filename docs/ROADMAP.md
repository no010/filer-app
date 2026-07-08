# filer ROADMAP

设计 review 后的可做功能点与改进清单。✅ = 已做；其余按价值/工作量分档，按需取用。

## A. 健壮性 / 正确性

- ✅ **A1+A2 安全删除**：`delete_filed_file` / `delete_source_file` / replace 改送 OS 回收站（`trash` crate），不再永久删。
- ✅ **A3 错误事件到 UI**：watcher/scan 失败 emit `process-error`，UI 弹 toast，不再静默。
- ✅ **A6 卸载清右键菜单**：NSIS `POSTUNINSTALL` 钩子删 `HKCU\*\shell\filer` + 设置里"移除右键菜单"按钮。
- ⬜ **同路径重下载内容变了要重新入箱**：`exists_for_path` 命中后比 mtime/size，变了当新文件（watcher.rs）。
- ⬜ **`scan_now` 超时按文件大小动态算**：现在固定 60s，50GB+ 文件会被误跳（watcher.rs:322）。
- ⬜ **跨卷 move 的 copy+delete 非原子**：源删除失败时回滚删目标副本（filer.rs `move_file`）。
- ⬜ **`mark_replaced` 保留审计**：加 `replaced_by` 列，不清 filed_path，历史可见"被 #N 替换"。
- ⬜ **DB schema 版本表**：现在只有 `ensure_column` 加列；以后改类型/改名需要正规迁移（`refinery` 或手写 version 表）。
- ⬜ **路径存 UTF-8 字节**：`to_string_lossy` 对罕见字符路径丢码，改 `os_str_bytes` 存原始字节（watcher.rs:199）。

## B. 高价值功能

- ✅ **B9 自动更新**：`tauri-plugin-updater` + 签名密钥对 + `latest.json` manifest，启动时检查并提示下载重启。
- ✅ **B10 拖拽导入**：窗口拖放文件 → `import_path` 入箱。
- ✅ **B15 收件箱键盘三连**：j/k 移动焦点、f 归档、i 跳过、e 编辑。
- ⬜ **B11 多 watch 目录**：`watch_dir: String` → `watch_dirs: Vec<String>`，同时监控 Downloads + Desktop + 项目目录。
- ⬜ **B12 内容检索（FTS5）**：SQLite FTS5 + PDF/文本内容抽取，搜"datasheet 里提到 CAN"。区别于手动整理的核心卖点。
- ⬜ **B13 暂停/恢复监听**：临时别自动入箱（批量下临时文件时）。
- ⬜ **B14 预览/dry-run**：改规则后先看"会归到哪"再提交。
- ⬜ **B16 规则导入/导出**：分享规则集（如"硬件工程师 datasheet 规则包"）。
- ⬜ **B17 托盘图标 + 最小化到托盘**：后台 watcher 有托盘存在感，点托盘唤起/暂停。
- ⬜ **B18 定时清理动作**：回顾 tab 加"超过 N 天自动移 cold 子目录 / 送回收站"可选策略。
- ⬜ **B19 CLI 模式**：`filer scan` / `filer file <path>` 给脚本/无头用。
- ⬜ **B20 标签自动建议**：按文件名/分类建议标签（datasheet→"mcu"，invoice→"finance"）。

## C. UX 打磨

- ⬜ **C1 暗色模式**（Tailwind 现浅色 only）。
- ⬜ **C2 i18n**：UI 全中文，加 en/zh 切换扩受众。
- ⬜ **C3 History/Review 筛选排序**：按分类/日期/大小筛，不只搜索。
- ⬜ **C4 Stats 时间序列**："每周归档数"折线，不只计数卡。
- ⬜ **C5 首次向导自动探测 Downloads**：Windows `~/Downloads` 预填。
- ⬜ **C6 ReviewModal dest_dir 文件夹选择器**（现在纯手打）。
- ⬜ **C7 空状态引导**：首次空收件箱"拖个文件试试"。

## D. 规模化（量大才要）

- ⬜ **D1 `list_inbox` 分页**：现在全量加载，inbox 上千条会卡（store.rs）。
- ⬜ **D2 search 加 FTS5 或 filename 索引**：LIKE 全表扫，>10k 条会慢。
- ⬜ **D3 大 PDF 用 lopdf 流式加载**：现在 >50MB 跳过提取，大 datasheet 拿不到厂商建议。

## E. 分发 / 生态

- ✅ **E1 代码签名**：macOS Developer ID（申请中）+ Windows SignPath Foundation（申请中）。
- ⬜ **E2 包管理器**：Winget / Scoop / Chocolatey（Windows）、Homebrew tap（macOS）、Flathub（Linux）。
- ⬜ **E3 portable 模式**：config+db 放 exe 旁边，USB 可携。
- ⬜ **E4 `#![allow(dead_code)]` 清理**：`now_ym`/`fmt_in_tz`/`set_suggestion` 等留而未用，接上或删。
- ⬜ **E5 跨平台右键**：仅 Windows；macOS Quick Action / Linux 文件管理器集成。

## 测试覆盖

- ⬜ **watcher 集成测试**：notify→process 流程（现在只 `drain_stable` 单测）。
- ⬜ **UI 测试**。
- ⬜ **跨卷 move 实测**：`move_file` 的 EXDEV 回退无实测（需两卷）。
