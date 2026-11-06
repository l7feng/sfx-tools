---
handoff: "task-context"
version: "1.0"
language: "zh-Hans"
coverage: "FULL"
---

# 任务交接 / Task Handoff — sound-workbench (音效/音乐管理软件)

## 继续位置 / Resume

### 当前目标 / Current goal

继续 sound-workbench 开发（Tauri 2 + React 19 + TypeScript + Rust + SQLite，debug 产物 `sfx-tools.exe`）。**0.1.20 已完整交付**：全长波形、跨语言搜索、音频工具页默认折叠、设置页新增试听行为／输出设备／缓存／临时文件／数据备份。下一批按 24 号复盘的中期／战略项推进（见下「建议下一步」）。

### 停止位置 / Stopped at

0.1.20 交付于 2026-11-06。基线提交 `90d60bd`（0.1.19）；本批主提交 `b00d1af` 已推送 `origin/main`（知识库仓按约定不自动提交），并构建出 debug EXE `sfx-tools.exe` = 25,324,544 字节、ProductVersion/FileVersion 0.1.20、mtime 2026/11/06 13:01。README / HANDOVER / 知识库项目 54 `Documentation.md` / `Plan.md` 均已同步 0.1.20 章节。

验证：`cargo check --all-targets --offline` 通过；`cargo test --bin sfx-tools --offline` 172 通过 / 1 失败 / 6 忽略（唯一失败为既有时间敏感测试，见下）；`player::tests` 22/22；`tsc --noEmit` 通过；`node --test tests/waveform-data.test.mjs` 10/10；`verify.ps1 -Action build` 通过。CDP 暗／亮两色 12 张截图在 `D:/My-Temporary/sw-debug/v20-*.png`。

### 建议下一步 / Recommended next action

按 24 号复盘（`24-市面音效库工具对标复盘与改进路线.md`）的中期项，确定性从高到低：
1. **AAF/OMF 真正解析与 PT Spotting** —— 已有只读预览与状态文案，按既定「最后提供测试工程」顺序，不提前伪造工程写入。
2. **感知近重复检测升级 / UCS 结构化管理 / 线索表（Cue Sheet）导出 + 许可证字段** —— 复用现有标签与列模型。
3. **中文相关性评测** —— 工具已在 0.1.12 交付（hit@k/MRR、评测集模板、面板入口），缺的是人工标注评测集，指标仍为零产出，不得宣称「已达标」。

### 完成标准 / Completion criteria

- 每条需求落地并有 CDP 截图验收（暗／亮两色各一组）。
- 版本号四处同步（`package.json` / `Cargo.toml` / `Cargo.lock` / `tauri.conf.json`）。
- `cargo test --bin sfx-tools` 与 `tsc --noEmit` 全绿；已知既有 flaky 失败需单独说明并做 HEAD 对照。
- 本轮**不自动提交知识库仓**（存在大量并行会话未提交中间态）。

## 不可丢失 / Keep

### 有效决定 / Active decisions

- **【最高优先，不可回退】波形必须显示全部，不能只显示片段**（人类 2026-11-06 追加裁决）。任何波形性能优化都不许用缩短分析范围冒充提速：必须保持 `analyzed_duration ≈ duration`、`truncated=false`。本批早期「简化小波形只解码前 12 秒」已撤销，历史「列表最多前 60 秒」也取消。
- **全长实现**：`player.rs` 按 `duration::seconds(path)` 算读取上限（多留 1 秒容错）；分桶数超 2×目标峰值时两两归并取 max、桶宽翻倍，内存恒定、末尾瞬态不丢；仅在完全读不到时长时退回 `WAVE_MAX_SAMPLES=24_000_000` 安全上限，再由 `WAVE_ABSOLUTE_MAX_SAMPLES=2_000_000_000` 兜底。`WAVE_CACHE_VERSION=6`（旧版 5 的缓存只含片段，必须整批失效）。点击波形试听上界按真实文件时长，不再卡 60 秒。
- **两种波形规格都覆盖全长**：`simple=true` = 512 峰、单条单色包络、不分声道（`waveforms-mini-v1`，`variant=1`）；`simple=false` = 4096 峰、保留 L/R 分轨（`waveforms-v1`，`variant=0`）。`WAVE_DECODE_LOCK` 全局串行，列表读取与设置页预生成共用。单线／单色只省 SVG 渲染，不省解码。
- **列模型（0.1.19）**：删 `status` 列，新增 `#`／分类／声道／收藏／播放列表；BPM／调性仅音乐库可见。默认列序 `# 波形 文件名 分类 时长 采样率 声道 收藏 播放列表`，`格式` 默认隐藏。行高 38px（comfortable 48px）。
- **音乐库筛选维度条（0.1.19，`src/music-filters.tsx`）**：常驻一行 8 个维度，多选取交集（后端 `tags:Vec<String>` 上限 8），时长走真实 `duration` 区间；BPM／调性只从标签读，未标注显示「—」，不伪造分析。
- **跨语言搜索（0.1.20，`multilingual.rs`）**：本地静态中／英／日词表，`MAX_VARIANTS=6`、`MAX_TOTAL_VARIANTS=64`；词内 OR、词间 AND；所有候选走 SQL 绑定参数；标签／分类／格式／时长／收藏／播放列表条件完全不受影响。离线、不调模型。
- **设置页架构（0.1.20）**：7 个分类。`settings_ops.rs` 在 setup 用 `settings_ops::init(app,&data)` 固定本次进程缓存根（自定义路径下次启动生效）；`safe_dir` 要求绝对路径且祖先无符号链接，清空／清理／备份只碰白名单目录。数据备份每次独立 `backup-…` 目录永不覆盖，`VACUUM INTO` + SHA256 + `integrity_check` + manifest + localStorage 设置快照。
- **输出设备**：`cpal = "=0.15.3"` 显式依赖（与 rodio 0.20.1 传递依赖一致），`OutputStream::try_from_device` 按名称选择，找不到／打开失败静默回落 `try_default()`；需 `cpal::traits::{DeviceTrait,HostTrait}`。
- **波形配色（0.1.18）**：蓝／白（`preferences.waveColor`，浅色下白转墨色）；立体声 L/R 分轨 + 分隔线，单声道单轨，多声道回落。「按频谱着色」仍在，待裁决是否下线。
- **主题**：8 组组合（4 主色 × 亮暗），`<html data-theme data-accent>`；`localStorage` 键 `sfx-tools-theme-v1`、`sound-workbench-preferences-v1`。
- **缓存护栏**：`WAVE_CACHE_VERSION=6`、`full_wave::VERSION=2`、`visual_analysis::VERSION=2`；改波形格式必须升版本使旧缓存失效。

### 约束与权限 / Constraints and authority

- **构建 PATH（硬约束）**：`RUSTUP_HOME=D:/Ai-Files/Tools/BuildEnv/sound-workbench-rust/rustup`、`CARGO_HOME=.../cargo`、Git Bash 的 `PATH=/d/Ai-Files/Tools/BuildEnv/sound-workbench-rust/cargo/bin:$PATH`（**POSIX 形式**；Windows 形式 `D:/...` 在沙箱里解析不到 cargo，会报 `cargo: command not found`）。debug 构建：`npm run tauri -- build --debug --no-bundle`，或 `verify.ps1 -Action build`（会先结束核对路径的本工程 EXE）。
- **CDP 调试**：`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`，连 `http://127.0.0.1:9333/json`；页面内调用用 `window.__TAURI_INTERNALS__.invoke`（不是 `window.__TAURI__`）；Python `urllib` 必须 `build_opener(ProxyHandler({}))` 直连，否则系统代理回 502；websocket 模块在 `D:/My-Temporary/_sw-pylibs313`（`sys.path.insert`），系统 Python 没装。`Runtime.evaluate` 裸箭头函数只返回 `{}`，必须包成 `(...)()`。
- **git 安全**：动 git 先 `git status`；commit 只 `add` 涉案路径，禁 `add -A`；push 走代理 `-c http.proxy=http://127.0.0.1:7897`。不自动提交知识库仓。
- **删除与不可逆操作**：删除走回收站（`SHFileOperationW`），禁永久删除；不可逆批量操作先校验＋等确认。
- **波形缓存事实**：峰值 envelope 存 JSON 文件（`waveforms-v1`／`waveforms-mini-v1`），**确实占磁盘**；单条 ≤128KB、总上限 16MB／256 条。本机实测 323 个／16MB，catalog 2.3MB。改配色不影响已缓存字节（缓存的是峰值不是颜色）。

### 不要复活 / Do not revive

- **片段波形**：`WAVE_SECONDS=60`／`WAVE_MINI_SECONDS=12`／「只解码前 12 秒」等任何按秒截断方案，已被人类明确否决。
- 列设置保留 `status` 列（已删）。
- 旧折叠式「音乐分类标签」面板（已重写为维度条 `music-filters.tsx`）。
- 路径猜测（路径锚点必须查 `D:\1-个人知识库_Vault\USER_PREFS.md`，不拼猜）。

### 失败尝试 / Failed attempts

- `catalog::tests::real_worker_cancel_resume_and_single_task_guard` 是**既有时间敏感／竞态用例**：0.1.20 全量跑失败（30 秒超时断言），单独 `--exact --test-threads=1` 也失败；用 0.1.19 HEAD 版 `catalog.rs` 对照重跑同样 46.84s 超时；0.1.18 README 验收记录里早已把它标为「既有偶发」。本批 `catalog.rs` 只改 `search_page` 与命令参数，未碰扫描 worker。**不要把它当回归去修**，也不要为它改产品代码。
- 构建 PATH 用 Windows 形式 `D:/...` → `cargo: command not found`；改 POSIX `/d/...`。
- CDP `Runtime.evaluate` 裸箭头函数 → 返回 `{}`；包成 IIFE。
- `settings_cache_pregenerate` 调用 `pregenerate_wave_cache` 必须传 3 个参数（第三参 `simple:bool`）；0.1.20 中途曾漏改导致编译失败。

### 已回答问题 / Answered questions

- 小窗口只画一条：**可行，已实现**，但只省渲染不省解码。
- 深浅叠加统一成一个颜色：**不加快解码**，纯渲染层；已实现单色单线。
- 波形是否占磁盘：**占**，见上「波形缓存事实」，设置页可查看／改路径／关闭／清空。
- 「波形显示全部，不能只显示片段」：已按裁决改成全长，见「有效决定」。
- 仍未答：「按频谱着色」是否下线、列表小波形是否加高到 44–48px、图标三版定版、安装包生成。

## 材料与缺口 / Materials and gaps

### 工作区与关键文件 / Workspace and important files

- 仓库 `D:/Ai-Files/Repos/GitHub/sound-workbench`；知识库项目 `D:/1-个人知识库_Vault/5-领域-A/06-软件制作/54-音效管理软件/02-音效管理软件重写/`。
- 0.1.20 关键源码：`src-tauri/src/player.rs`（全长波形）、`multilingual.rs`（跨语言）、`settings_ops.rs`（设置后端）、`main.rs`（setup 缓存根 + 命令注册）、`catalog.rs`（`search_page` / `catalog_list`）、`folder_picker.rs`；前端 `src/catalog.tsx`、`settings.tsx`、`settings-panels.tsx`、`settings-client.ts`、`preferences.ts`、`waveform.tsx`、`waveform-data.ts`、`native-player.tsx`、`workspace-shell.css`、`tests/waveform-data.test.mjs`。
- 六张参考图已找回：`C:/Users/Administrator/.workbuddy-ai/clipboard-images/`（#1 波形配色、#2 试听行为三开关、#3 试听音量＋输出设备、#4 缓存波形图、#5 临时文件、#6 数据备份）。上一版交接里「图片未捕获」的缺口已补齐。
- 文档：`README.md` / `HANDOVER.md` / 知识库项目 54 `Documentation.md`（唯一当前状态入口）/ `Plan.md`。

### 随包附件 / Included attachments

- 无 / None.

### 未决问题 / Open questions

- 「按频谱着色」下线、小波形加高、图标定版、安装包 —— 待拍板。
- `src-tauri/icons/icon.ico`／`icon.png` 工作区有未纳入 0.1.20 提交的改动，保留现状待人类确认其来源与去留。
- `WAVE_CACHE_VERSION` 升级后旧版片段缓存成为孤儿文件（不会被读取但占磁盘），可提示用户在设置页「清空波形图缓存」。

### 已知缺失 / Known omissions

- 全长解码使长音频首次读取变慢，未做 WAV PCM 直读快路径；靠缓存与预生成摊薄。批量预生成对含大量长音频的库会明显耗时（可取消）。
- 6 张参考图只作视觉对照，未逐像素复刻。
- 安装包未生成（仅 debug 产物）；真实万条、十万条、PT 真实工程导入、S1.4 安装升级实测仍未完成。

### 需要重新验证 / Revalidate

- 是否有 0.1.20 之后的后续提交：本批收尾时 `origin/main` = `b00d1af`；接手时跑 `git status --short` 与 `git log origin/main --oneline -3` 确认有无新批次。
- 知识库仓未提交态（并行会话中间态）。
- 备份目录 `C:\Users\Administrator\AppData\Local\local.soundworkbench.desktop\backup\` 已有一次验收产物 `backup-20261106-130448-25919e3f`，属正常产品数据，勿当垃圾删除。
