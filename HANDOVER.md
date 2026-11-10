# SFX Tools 交接文档（供新 Agent 接手）

> 生成：2026-11-05，最后更新 2026-11-10。当前版本 **0.1.27**（SFX Tools；Cargo/npm 包名 `sfx-tools`）。0.1.22–0.1.27 均为仅 EXE 交付（成品区 `D:/Ai-Files/Tools/Exe/07-音效管理软件/sfx-tools-v0.1.27/`）；0.1.21 及更早的 debug/安装包记录作为沿革保留在下方各节，完整交付记录见知识库 `Documentation.md`。
> 本文件自包含，新 agent 无需依赖历史对话或 WorkBuddy Skill 机制即可接手。

## 1. 项目定位与关键路径

| 项 | 路径 |
|---|---|
| 工程仓库 | `D:/Ai-Files/Repos/sfx-tools`（2026-11 迁至现路径，旧 `Repos/GitHub/sfx-tools` 已不存在） |
| 知识库项目目录 | `D:/1-个人知识库_Vault/5-领域-A/06-软件制作/54-音效管理软件/02-音效管理软件重写/` |
| **唯一当前状态入口** | 知识库 `Documentation.md`（不是本文件、不是聊天记录） |
| 路线 | 知识库 `Plan.md`；执行约定 `Implement.md`；复盘 `24-市面音效库工具对标复盘与改进路线.md` |
| 许可审计 | 知识库 `25-模型与FFmpeg再分发许可审计.md`；仓库 `THIRD-PARTY-NOTICES.md`（初版，待补全文） |
| 元数据适配 | 知识库 `26-BWF与iXML元数据适配.md` |
| 评测方法 | 知识库 `27-中文相关性评测方法.md`；模板 `tests/cn-relevance-eval.template.json` |
| 活待办看板 | `D:/1-个人知识库_Vault/6-资产-R/01-待办事项/待办清单.md` |
| 交付 EXE | 成品区 `D:/Ai-Files/Tools/Exe/07-音效管理软件/sfx-tools-v0.1.27/SFX Tools.exe`（0.1.27，18,495,488 字节，SHA256 `107d5643…93139eb`）；桌面 `SFX Tools.lnk` 指向它。开发调试用 `src-tauri/target/debug/sfx-tools.exe` |
| 交接 Skill（可选） | `C:/Users/Administrator/.workbuddy-ai/skills/sound-workbench-handover/`（SKILL.md + references/handover-state.md）；zip 同目录 |

技术栈：Tauri 2 + React/TypeScript + Rust + SQLite/FTS5 + Python ONNX Runtime 桥（独立 CPU 推理）。

## 2. 不可妥协的约定

1. **唯一当前状态入口是知识库 `Documentation.md`**；不得另建第二份当前进度。每次交付后更新它。
2. 四文件规则：`Prompt.md` / `Plan.md` / `Implement.md` / `Documentation.md` 分工明确。
3. 构建用 `verify.ps1 -Action build`：**debug / --no-bundle，不自动打安装包**。更新前按授权后台结束本项目 `sfx-tools.exe`（Implement.md 授权；0.1.13 起已更名，`sound-workbench.exe` 是废弃旧名）。
4. **语义边界不可伪造**：文件齐备 / ONNX 张量自检 / `encode_text` / `encode_audio` 跑通，都不等于「音频语义检索可用」，也不等于「中文相关性评测通过」。评测工具已就位但**指标仍是零产出**；真实万条是正式发布闸门；十万条专项后置。
5. 只调用本机独立 FFmpeg，不捆绑分发；不操作用户素材库内容（只读；元数据写回仅在用户点击保存时发生）。
6. 提交必带身份：仓库与全局都没有配 `user.name/user.email`，`git commit` 需 `-c user.name=… -c user.email=…`；推送走代理 `-c http.proxy=http://127.0.0.1:7897`。
7. **元数据检索不等于语义检索**：0.1.21 的 `meta_search` 只是 bext／iXML 字段拼接后的字符串索引，精确匹配、不分词、不接 CLAP 向量；不得借它宣称「语义检索已补齐」或「中文相关性评测达标」。搜索栏「含元数据」只覆盖本次使用，默认值只在设置页持久化。

## 3. 构建与测试命令

```powershell
# 仓库根目录；verify.ps1 内置工具链环境与强制结束 EXE
& 'D:\Ai-Files\Repos\sfx-tools\verify.ps1' -Action test    # Rust 全量测试
& 'D:\Ai-Files\Repos\sfx-tools\verify.ps1' -Action build   # debug 构建 + 更新 EXE
npm run build                                                          # 仅前端 tsc + Vite

# Python 桥单测（须用 AI venv 解释器）
& 'D:\Ai-Files\Tools\BuildEnv\sfx-tools-ai\Scripts\python.exe' tests/audio_text_bridge_test.py

# 前端波形调度器单测
node --experimental-strip-types --test tests/waveform-data.test.mjs

# 真实 FFmpeg BWF 往返测试（默认 ignored）
& "$env:CARGO_HOME/bin/cargo.exe" test --manifest-path src-tauri/Cargo.toml real_ffmpeg_bwf -- --ignored
```

工具链环境（verify.ps1 已设）：
- `CARGO_HOME=D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/cargo`
- `RUSTUP_HOME=D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/rustup`
- `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`（Rust 1.99.1 MSVC）
- Python venv：`D:/Ai-Files/Tools/BuildEnv/sfx-tools-ai/Scripts/python.exe`（3.12.10, numpy 2.3.4, onnxruntime 1.23.2, tokenizers 0.22.2）

注意：`verify.ps1 -Action build` 在 PowerShell 工具里会因 node 写 stderr 抛 `NativeCommandError` 打断输出，但构建本身通常已完成；判定标准看 EXE 的 `LastWriteTime` 与 `ProductVersion`，不要只看命令是否报错。

## 4. 模型与 ONNX 契约（固定版本，勿随意升级）

| 模型 | model_id | revision |
|---|---|---|
| 文本 E5 | `intfloat/multilingual-e5-small` | `614241f622f53c4eeff9890bdc4f31cfecc418b3` |
| 音频-文本 CLAP | `Xenova/clap-htsat-unfused` | `c28f2883575e590e04d3146ff0713c2448d691ba` |

- CLAP ONNX SHA256：audio `3fcff2c8824e7bcb83a983f2a49edab3b60cbcf4872ac70efee517355173bd1f`；text `1a3df8b197e249816e08415fd040434c44762b2eea7eb7bf8a48a0f0bf3c14e5`。
- 音频塔输入实测 `[1, 1, 1001, 64]`（`preprocessor_config.json`：`nb_max_frames=1000`+1、`feature_size=64`）。
- 双塔各输出 **512 维**；`encode_audio` 返回 L2 归一化有限向量。
- 下载代理 `http://127.0.0.1:7897`（Hugging Face 直连超时，必须走代理）。
- 上游 `laion/clap-htsat-unfused` 为 Apache-2.0；`Xenova` 仓库**无 LICENSE、无 license 字段**，再分发需我们自行补齐声明（见 25 号审计「路线甲」）。
- 长音频确定性取开头 480000 samples（10s），未与官方 `ClapFeatureExtractor` 逐帧对照。

## 5. 已交付记录

### 0.1.26（当前）

按人类三项界面反馈落地（原话与三项裁决见知识库 `Documentation.md` 同名小节），沿用「实现 → 前端测试 → 类型检查 → Rust 测试 → release 构建 → 只交付 EXE」口径，未生成安装包。

- **筛选维度**：音乐库常驻筛选条改手风琴——同一时刻只展开一个下拉，点选值后不自动收起；筛选预置模板改存偏好（`sound-workbench-preferences` 内 `filterTemplates`），可在设置页增删改并持久化。
- **任务窗口**：右下角导入任务面板缩至 268×158（约原尺寸 12%），保留任务清单/进度滚动与关闭。
- **音频工具交互**：反转／单声道分离改为点击即按默认参数生成（无对话框、不自动播放，显示处理进度与「试听／导出…」，试听需手动点）；播放循环点击直接切换 `aria-pressed`；距离／效果器／叠层改为底部工具栏上方向上展开的侧边栏（`audio-side-panel`，三标签切换，叠层内嵌原图层工具，不再跳页）；变速改为音量右侧滑杆，松手按倍速生成并自动试听结果（`audio_tools_start`＋`audio_tools_preview`）。
- **验证与产出**：`tsc` 通过；前端 60/60（新增筛选手风琴/模板与变速滑杆断言）；`cargo test` 183 通过 / 0 失败 / 6 忽略（本批未改音频工具模块）；release EXE 0.1.26／18,285,568 字节，SHA256 `07e97151…87b05b0`，成品区 4 文件（EXE／ico／README／SHA256SUMS）；桌面快捷方式已改指 0.1.26。CDP 逐项核对三项（筛选一次只开一个、模板往返生效并还原、任务面板 268×158、反转/单声道分离无对话框且进度→就绪、循环直接切换、向上侧栏 380×545 三标签、变速 1.25×/1.50× 生成并自动试听），截图 `D:/My-Temporary/sw-debug/v26-*.png`。
- **边界**：鼠标／滑杆手感与听感、原生对话框待人工；跨机与万条素材未验；`THIRD-PARTY-NOTICES.md` 仍为初稿；工程仓未提交。

### 0.1.25

按人类十三项界面反馈＋四项裁决落地（原话与裁决表见知识库 `Documentation.md` 同名小节），沿用「实现 → 前端测试 → 类型检查 → Rust 测试 → release 构建 → 只交付 EXE」口径，未生成安装包。

- **布局与样式**：窄窗侧栏去 vw 封顶可拖拽（双击复位 220）；删除列表密度选项（基线即紧凑）；默认强调色改蓝 `#6aa9e0`；按钮统一 32px 高、4/6px 圆角 token；设置弹窗锁定 690×560 内容内滚。
- **检索联动**：关「含元数据」=仅匹配文件名（`catalog.rs` 检索列只取 `a.name`）；搜索栏开关与设置默认值双向写回偏好，取代 0.1.21「临时开关不写回」语义；占位文案随开关切换。
- **右键菜单**：14 项单行化，解释改 `title` 悬停提示，删占行小字与「查找相似」。
- **底部播放区**：传输行与工具行同排；左下六按钮默认中文；播放／导出／生成等重点按钮强调色底。
- **Logo**：蓝色声波柱内联 SVG＋新 `sfx-tools.ico`/png，顶栏／EXE／任务栏／快捷方式同源。
- **收藏分组**：侧栏单层文件夹树（全部收藏／各分组／未分组），沿用 `favorite_group` 字符串不迁移。
- **工具页**：不渲染侧栏／详情栏；页内拆「工具（3)／管理（6)」两个 DockFrame 应用内浮窗（非 OS 多窗），可浮动／停靠；UI 层 AI／模型／语义检索入口与文案移除，后端休眠保留。第 11 项改名经裁决取消，名称保留。
- **账号菜单**：新增 `account-menu.tsx`，顶栏「账号」下拉（状态行＋账号与授权管理…／刷新授权／退出登录），不再切页；菜单骨架参考 SoundSeeker 1.1.8 静态实证（激活／升级／刷新授权／个人中心／退出登录形态），动作仍走本项目 licensing 命令与本地开发发行端，不接真实支付，未读取其用户数据、未运行其程序。
- **验证与产出**：`tsc` 通过；前端 58/58；`cargo test` 183 通过 / 0 失败 / 6 忽略（`model_download` 一例并行负载下环境性抖动，隔离重跑通过）；release EXE 0.1.25／18,284,032 字节，SHA256 `896c9d5a…a5d20624`，成品区 4 文件（EXE／ico／README／SHA256SUMS）；桌面快捷方式已改指 0.1.25。CDP 逐项核对 13 项（620px 窄窗 246→256→220、元数据开关同词 0 vs 48044 条、两窗口 `aiTraces=[]`、账号点击保持 library 页等），截图 `D:/My-Temporary/sw-debug/v25-*.png`；素材库 60367 条未变。
- **边界**：鼠标手感／目视、原生对话框、跨机、万条素材与听感待人工；两窗口为应用内浮窗；`THIRD-PARTY-NOTICES.md` 仍为初稿；工程仓未提交。

### 0.1.24

按人类六项需求落地（原话见知识库 `Documentation.md` 同名小节）：「1、收藏可以分组 2、十字架鼠标更改 3、波形截取片段，可十字架 4、六窗口可折叠成图标 5、信息弹窗优化 6、logo图标优化」，沿用「实现 → 前端测试 → 类型检查 → Rust 测试 → release 构建 → 只交付 EXE」口径，未生成安装包。

- **收藏分组**：`catalog.rs` 搜索／列表参数增 `favorite_group`／`favorite_ungrouped`；设组复用 `catalog_set_favorite_group`（设组同时置收藏，清除分组只清分组、不动收藏位）；前端收藏视图加分组筛选条、选择工具条加「收藏分组」批量入口，详情面板可内联改组。
- **波形片段**：列表迷你波形改手型光标、主波形保留十字光标；主波形拖选生成选区，可试听（到片段末尾自动暂停）与导出 WAV（`audio_clip_export`，单段上限 600 秒，不覆盖同名文件，只读源素材）。
- **折叠成图标**：左侧六个快捷入口与底部六个音频工具都可折叠为纯图标；偏好存 `sound-workbench-sidebar-shortcuts-compact-v1`／`sound-workbench-audio-toolbar-compact-v1`，重启保持。修复折叠后图标被基础样式 `display:none` 隐藏导致按钮空白的问题。
- **详情面板**：新增摘要卡（名称／格式／时长／采样率／大小／分组徽标），分组去重、去掉空的 AI 分组，保留复制与定位操作。
- **Logo**：顶部「∿」字符换成与 EXE 图标同款的内联 SVG（`brand-mark.tsx`，深色圆角底＋金色竖条）。
- **验证与产出**：`npx tsc --noEmit` 通过；前端 `node --experimental-strip-types --test tests/*.test.mjs` 57 项通过；`cargo test` 182 通过 / 6 忽略（排除依赖外网的 `model_download` 用例）；release EXE 0.1.24／18,285,056 字节，成品区含 `README.md`、`SHA256SUMS.txt`、`sfx-tools.ico`；桌面快捷方式已改指 0.1.24。GUI 经 WebView2 调试协议实证六项（含 25%–68% 选区、试听 2.15 秒自动暂停、导出 1.21 秒 WAV、素材总数回落 60367）。
- **边界**：本远程会话鼠标注入被拦截，原生「选择文件夹」对话框与导出完成提示未人工点选；未做跨机与万条素材验收；片段只导出 WAV；收藏分组为单层；工程仓未提交。

### 0.1.22 / 0.1.23（仅 EXE，摘要）

- 0.1.22：界面改版——空状态分层排版、通用浮窗框架（边缘吸附预览＋可调尺寸并持久化）、播放器底部／左栏／浮窗三态；仅 EXE 交付，明细见 `Documentation.md` 同名小节。
- 0.1.23：logo 与名称统一「SFX TOOLS」、拖出再拖回按取消处理、新增 M4A（AAC）导入／播放／波形、波形与时长一致的重复素材合并（只整理目录记录，不动磁盘源文件）；另修列宽偏好重置与旧版占用素材库时的静默崩溃。

### 0.1.21（历史，元数据检索）

按人类「搜索栏跨语言按钮旁加『含元数据』开关，可在设置更改默认开启、可手动关」落地。事实核查先确认 0.1.20 既无按钮、也无 BWF/iXML 检索索引（关键词只匹配 `a.name || a.path || COALESCE(m.tags_search,'')`），因此做的是完整闭环，不是 UI 假开关。

- **后端索引**：`catalog.rs` 增 `assets.meta_search TEXT` 兼容列（`NULL` = 旧库未索引、空串 = 已索引但无内嵌字段）；`metadata_search_text(path, format)` 只对 WAV 调 `bwf_metadata::read`，抽 bext 描述／制作者／参考号／日期时间与 iXML 的 PROJECT／SCENE／TAKE／TAPE／NOTE 及 `SW_` 私有字段，去控制字符、拼成一行、截到 `META_SEARCH_LIMIT = 4096` 字符。`record_file` 在新增／更新／旧库补索引时写入，尽量不重复解码；新增 `CatalogState::refresh_metadata_index(&self, id)`。
- **检索开关**：`SearchFilter` 增 `metadata:bool`，`catalog_list` 增 `metadata:Option<bool>`；`search_page` 在打开时把 `COALESCE(a.meta_search,'')` 拼进关键词匹配式。跨语言扩展与标签／分类／格式／时长／收藏／播放列表筛选不受影响，用户输入仍全走 SQL 绑定参数。
- **回写联动**：`bwf_write_metadata` 写盘成功后调 `refresh_metadata_index`，刷新失败不回滚音频、不掩盖写入成功，`WriteReport.rescan_required` 提示保留。
- **前端两层状态**：`preferences.metadataSearch` 默认 `true`（旧存档缺字段也默认开，显式 `false` 才关）；`catalog.tsx` 的 `metadataOn` 是本次使用临时态，手动关不写 localStorage，设置改默认值时 `useEffect` 同步、重启按默认值恢复。搜索栏新增第二个 `.search-toggle` 按钮「含元数据」，`scopeKey`／`refresh` 纳入该状态；设置页「素材库」分类新增「检索时默认包含内嵌元数据」复选框与边界说明。
- **验证**：`cargo check --all-targets --offline` 通过；全量 `cargo test --bin sfx-tools --offline` **176 通过 / 0 失败 / 6 忽略**（历史 flaky 用例本轮通过，仅增量编译锁目录删除告警）；新增 3 个 Rust 元数据单测；`npx tsc --noEmit` 通过，`tests/preferences.test.mjs` 2/2、波形单测 10/10；`verify.ps1 -Action build` 通过，EXE 0.1.21／25,329,152 字节／2026-11-06 14:28。CDP 实测 `app_version`=v0.1.21、两个搜索按钮初始都按下、手动关不改 localStorage 偏好、设置里关默认值后 reload 保持、`catalog_list` 接受 `metadata:true/false`；暗／亮共 7 张 `v21-*.png`。
- **边界**：旧版本建立的素材库需重扫一次补齐 `meta_search`；只覆盖 WAV/BWF 的 bext／iXML 已知字段，MP3／FLAC／AIFF 无此内嵌字段（用户标签仍可搜）；`axml`／`chna`／`cart`／`ds64` 未解析；拼接文本不分词、不接语义向量。版本四处同步 0.1.21；工程仓本批未提交。

### 0.1.20（上一批）

按人类六张参考图与本批文字需求落地，中途人类追加裁决「波形显示全部，不能只显示片段」。

- **全长波形（最高优先裁决）**：撤销本批早期「简化小波形只解码前 12 秒」，并取消历史「列表最多前 60 秒」截断。`player.rs` 按文件头时长算读取上限、分桶数超 2×目标峰值时两两归并并翻倍桶宽（内存恒定、末尾瞬态不丢）；`WAVE_CACHE_VERSION` 5→6 使旧片段缓存整批失效；点击试听定位上界改按真实时长。简化规格仍是**整条音频**，只是 512 峰／单条单色包络／不分声道；完整规格 4096 峰并保留分轨。单线单色只省渲染，提速靠缓存复用、流式降采样与 `WAVE_DECODE_LOCK` 串行解码。
- **三个事实性回答**：小窗口只画一条可行但只省渲染；深浅叠加统一成一个颜色不加快解码；波形**确实占磁盘**（本机实测 323 个文件／16MB，单条 ≤128KB、总上限 16MB／256 条，设置页可查看／改路径／关闭／清空）。
- **工具页折叠**：进音频工具自动收起左素材栏、右详情栏、底部播放器，离开时恢复；偏好可关。
- **跨语言搜索**：`multilingual.rs` 本地中／英／日静态词表，词内 OR 词间 AND，候选全走绑定参数；公式／标签／分类／格式／时长／收藏／播放列表筛选不受影响。搜索条有开关，设置里可关。
- **设置页**：分类 5→7。新增输出设备下拉（`cpal = "=0.15.3"`，按名称选设备、拔掉回落系统默认）、三个试听行为开关、图 #4「缓存波形图」（开关／自定义路径／分组统计／清空／预生成进度与取消）、图 #5 临时文件清理、图 #6 数据备份（独立目录永不覆盖，`VACUUM INTO` + SHA256 + `integrity_check` + manifest + 设置快照）。`settings_ops.rs` 统一治理缓存根，清理／备份只碰白名单目录。
- **验证**：`cargo check --all-targets` 通过；全量 `cargo test --bin sfx-tools` 172 通过 / 1 失败 / 6 忽略（唯一失败为既有时间敏感用例，用 0.1.19 HEAD 版对照重跑同样超时，非本批回归）；`player::tests` 22/22；`tsc --noEmit` 通过、前端波形 10/10、debug 构建通过。EXE 0.1.20，25,324,544 字节，2026/11/06 13:01。CDP 实测：库中 3534 秒素材的简化／完整波形都 `analyzed_duration=3534`、`truncated=false`；跨语言 `rain` 2→4 条、`雨音` 0→4 条；输出设备枚举出 UR22C 等真实设备；数据备份实产出 2,150,400 字节一致性快照 `integrity=ok`。暗／亮两色 12 张截图核对四个设置面板与工具页折叠／恢复，无裁切。

未收口：长音频首次解码比旧的截断方案慢（靠缓存与预生成摊薄）；文件头时长缺失的异常输入仍退回 2400 万采样安全上限并标「前段」；旧版片段缓存成为孤儿文件，可在设置页一次清空；「按频谱着色」下线、图标定版、安装包仍待拍板。

### 0.1.19 列表列重构与音乐库（配乐）窗口

按人类三条反馈落地：列设置删「状态」、参考图 1 重排列表并压缩行高、参考图 2 优化配乐窗口。裁决为「列结构＋行高紧凑」与「筛选维度条＋列＋音乐详情组」。

- 列模型：`ColumnId` 6→12 项，删 `status`，新增 `#`／分类／声道／收藏／播放列表，以及 `musicOnly` 的 BPM／调性；`visibleColumns(layout,library)` 按库过滤。旧列宽按列名回填，纯旧列序与「旧列在前＋新列追加」两种存档整体回到新默认列序，避免新列全挤在表尾。
- 列表：行高 53→38px、名称单行去扩展名、完整名与路径进悬停提示、小波形 34→28px；状态异常行以名称前的红色小圆点提示，不占列。表头三列可点击切换升降序（后端补 `*_desc` 白名单，NULL 时长仍最后）。
- 行内操作：新增 `playlist-picker.tsx`，♡ 即时收藏、≡ 选歌单或新建后加入；均只写本机目录。
- 筛选维度条：`music-categories.tsx` 删除，新建 `music-filters.tsx`／`music-filters.css`，常驻一行 8 个下拉（含时长）；值带计数、多选取交集（后端 `tags:Vec<String>`，上限 8）、chip 可撤销；每个下拉可给所选素材打 `维度:值` 标签。曲风兼容早期 `曲风:` 前缀。
- 后端：`catalog_list` 增加 `tags`／`min_duration`／`max_duration` 三个参数；`SearchFilter` 增加同名字段；时间区间按真实 `duration` 过滤，未知时长排除；排序白名单补三个降序表达式。BPM／调性没有自动分析，只从标签读，未标注显示「—」。
- 详情：`asset-detail.tsx` 增 `library` 参数，音乐库多出「音乐信息」组（艺术家／专辑／BPM／调性／年份／曲风），编辑写回标签不动原音频。
- 测试：全量 `cargo test --bin sfx-tools` 156 通过 / 0 失败 / 6 忽略，含新增的 `sorts_descending_and_filters_by_duration_range`、`multiple_dimension_tag_filters_intersect`；`tsc --noEmit` 与 debug 构建通过；EXE 0.1.19，25026560 字节，2026/11/06 11:27。CDP 模拟 0.1.18 存档升级后列序正确、行高 38px；排序箭头、播放列表浮层、维度 chip、时长「3 分以上」2169→180 条逐项截图核对。

未收口：窄窗口仍需横向滚动；「按频谱着色」是否下线待定；三版候选图标未选定；安装包未生成。

### 0.1.18 波形声道分轨与详情窗口重排

按人类三条反馈落地：波形配色只留蓝/白、波形要区分立体声与单声道、详情窗口按参考图整理并让标签与元数据可折叠。

- 波形配色：`Preferences.waveColor` 由 `amber|mint` 改为 `blue|white`，`workspace-shell.css` 用 `.wave-blue` / `.wave-white` 两套令牌，浅色主题另给墨色版本（白底上白色波形不可见）；旧偏好值 `safePreferences` 一律回落 `blue`。
- 声道分轨：`Waveform` 新增 `channels` 与 `channel_peaks`（player.rs / full_wave.rs），`VisualData` 新增 `channels` / `channel_peaks` / `channel_rms`（visual_analysis.rs）。前端 `envelopePath(peaks,lanes,lane)` 按轨道分区镜像，`Envelope` 传 `lanes` 即上下双轨，`channelLanes()` 只认「两条真包络」，多声道与旧缓存回落单轨。
- 缓存：`WAVE_CACHE_VERSION` 3→4、`full_wave::VERSION` 1→2、`visual_analysis::VERSION` 1→2，旧记录整批失效重算。
- 详情窗口：新建 `asset-detail.tsx` / `asset-detail.css`，五段分组（文件信息 / 我的标签 / 内嵌元数据 / AI 分类标签 / 原始文件位置），每段可折叠；文件信息为行式左标签右值；标签折叠态显示圆角标签或「还没有标签」，点「编辑」展开 textarea；元数据折叠态右侧为「编辑」，展开后显示 bext/iXML 状态与单列输入框（面板只有两百余像素宽，双列会被裁）。
- 元数据读取合并：`useBwfMeta` 提升到详情层，`BwfMetadataPanel` 支持 `injected` / `useInjected` 注入，一个素材只读一次；写回后 `onSaved` 触发 detail reload。`asset-annotations.tsx` 已删除。
- 测试：新增 `stereo_waveform_keeps_per_channel_lanes`；`player::tests` 19 项、`visual_analysis` 4 项、`full_wave` 3 项通过。全量 `cargo test --bin sfx-tools` 152 通过 / 6 忽略 / 2 失败，失败项与改动无关——`catalog::tests::real_worker_cancel_resume_and_single_task_guard` 是 `Instant::now() < deadline` 超时断言（本机沙箱慢），`model_download::tests::single_file_full_download_is_verified` 需要外网。
- 实测：合成立体声（左前段满、右后段满）与单声道各一条，导入临时目录后 `channel_peaks` 前后半段均值 0.393 / 0.035 与信号一致，单声道为空；验收后临时条目已从素材库移除，临时音频留在 `D://My-Temporary//sw-audio`。

未收口：列表小波形 34px 高、立体声每轨约 16px，只够看「上下两条」形态；「按频谱着色」模式未下线；频谱画布配色未接新配色。

### 0.1.17 界面主题化与工具页图标


按人类六条界面反馈落地，全部改动在前端，Rust 侧未动。

- 目录树：`folder-tree.tsx` 用内联 SVG `FolderGlyph` 替换 `▱` 字符，闭合／展开两套造型，选中态跟随 `--accent`；侧栏标题「原路径目录树」→「路径目录」。
- 播放条：`.player-heading`、`.wave-view-controls` 改右对齐，「待机 尚未播放 波形:…」「波形视图 峰值」两行靠右；删除播放器下方长提示。
- 二级标签栏：素材库顶栏新增 `.tool-tabs`，横向切换 11 个工具页；`openTool` 在音频工具页内不再跳回素材库；`.tool-directory` 只在总览／搜索时显示。
- 工具图标：新增 `src/tool-glyphs.tsx`（11 枚工具图标 + `grid`/`back` 两枚动作图标，24×24 线性，`stroke:currentColor`）。
- 主题系统：新增 `src/theme.css` + `src/theme.ts`，`data-theme`（dark/light）× `data-accent`（gold/blue/teal/violet）= 8 组；强调色阶由 `color-mix` 派生；`localStorage` 键 `sfx-tools-theme-v1`；`index.html` 内联首帧脚本防浅色启动闪暗。`workbench.css` 的 `:root` 只留字体与尺寸，77 处写死色值改为变量。
- 设置面板：`显示与布局` 顶部新增强调色四色点 + 底色深色/浅色下拉。
- 验收：`tsc` 与 debug 构建通过；像素审计确认暗紫主题 285 个高饱和像素落在 220–300°、金色仅 6 个（暗金主题金 61／紫 19）；文字对比度浅色 5.39–5.73:1、暗色 7.08–7.53:1，12px 正文过 WCAG AA。
- 未收编：`spectral-view.css` 12 处频谱画布配色（刻意深底）、素材标签 6 种分类色、`#0006/#0008/#0009` 阴影；波形配色仍是「暖金／青绿」偏好项，未与强调色联动。

### 0.1.16 字号与字体可读性

- 字号可读性：按人类「字号有点小，看着累」的反馈，全库 246 处字号整体上移一档（`8→10 9→10 10→11 11→12 12→13 13→14`），最小字号从 8px 提到 10px、正文基线 13→14px。只在 `font-size` 与 `font:` 简写的字号上动手，高度/边距/行高未批量改。
- 阅读面二次对齐（`workspace-shell.css` 末尾「可读性基线」块）：表头 12px/30px、表体 12px/53px、`.asset-name strong` 13px、`.asset-name small` 11px、状态栏 11px/28px、导航项 35px，行高同步放大。列表每屏少约 1 行。
- 字体栈：`--mono` 补中文回退并前置 Cascadia Mono（`'Cascadia Mono',Consolas,'Microsoft YaHei UI',monospace`），修掉路径列中文掉宋体的问题；`--body` 挂 `Segoe UI` 兜底。
- 0.1.15 一并提交：`capabilities/main.json` 补 `core:app:allow-version` 并改用自有命令 `app_version()`；波形新增 `isMissingSourceError` 区分文件缺失与瞬时错误；`pinned_paths` 增 `color` 列、新增 `library_pin_color` 命令与右键六色板、常用路径移到侧栏底部常驻并默认展开。`.sidebar-pins` 本轮由 `column-reverse` 改回 `column`，标题回到列表上方。
- 构建口径更正：`npm run tauri -- build` 必须带 `RUSTUP_HOME=D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/rustup` + `CARGO_HOME=.../cargo` + PATH `.../cargo/bin`，否则报 toolchain 未安装（默认 `RUSTUP_HOME` 是空的 `C:\Users\<用户>\.rustup`）。只改前端时增量构建 15–25 秒。
- 提交：`1ced5ad`，33 个文件，已推送 `origin/main`。`src-tauri/icons/icon.{ico,png}` 的候选图标仍在试验，未纳入提交。

### 0.1.13 更名与版本信息

- 更名 SFX Tools：`productName`、Cargo 包名与 npm 包名改为 `sfx-tools`，产物由 `sound-workbench.exe` 变为 `sfx-tools.exe`；窗口标题、侧栏品牌、关于页、`document.title`、`index.html`、单实例窗口匹配串、`verify.ps1` 结束进程路径同步改。桌面快捷方式重建为 `SFX Tools.lnk`（旧 `Sound Workbench.lnk` 已入回收站）。
- EXE 版本资源补齐（tauri-build 取自 `productName` + `bundle.publisher` + `bundle.copyright`）：`ProductName`/`FileDescription`/`CompanyName` = SFX Tools，`LegalCopyright` = © 2026 SFX Tools，`FileVersion`/`ProductVersion` = 0.1.13。`OriginalFilename` 该字段 tauri-build 不写，属性里为空。
- identifier 保持 `local.soundworkbench.desktop`，`%LOCALAPPDATA%` 下目录库、向量索引、模型登记不受更名影响。
- 波形代码本轮未改：`player.rs` 的 `Mutex<()>` 排队与 `waveform-data.ts` 的队列 64 + 退避重试保持 0.1.11 状态；Rust 152 项、前端波形 9 项通过，真实 GUI 待人类手测。

### 0.1.12 上一批

- `src-tauri/src/bwf_metadata.rs`（新）：RIFF/WAVE 容器读写。解析 `bext`（描述/制作者/参考号/日期时间/timeReference）与 `iXML`（PROJECT/SCENE/TAKE/TAPE/NOTE + 私有 `SW_` 字段）；命令 `bwf_read_metadata` / `bwf_write_metadata` 只接受素材 id（经 `catalog.resolve_asset` 解析），不接受前端传任意路径。
- 回写策略：只替换 bext 与 iXML，其余 chunk 连头带数据逐字节复制（含奇数补位）；缺 bext 时插在 `fmt ` 后；重算 RIFF 长度；同目录临时文件 → 回读校验 → 原子替换，失败保留原文件。RF64 只读、非 RIFF 拒绝、超长 bext 字段报错不截断。
- `src/bwf-metadata.tsx`（新）：详情栏「BWF / iXML 元数据」面板，9 个字段可读可写，显示字节变化与「目录需重扫」提示。
- `src-tauri/src/semantic_eval.rs`（新）：中文相关性评测。评测集 JSON（查询 + 期望文件名片段），指标 hit@1/3/5/10 + MRR，未标注查询不进分母；命令 `semantic_vector_eval`（绝对路径、1MB 上限）与 `semantic_vector_eval_template`；索引为空直接报错，不出假指标。
- `src/cn-eval.tsx`（新）：CLAP 面板底部「中文相关性评测」区块。
- `semantic_vector::search_text` 由私有改 `pub(crate)` 供评测复用，行为未变。
- `tests/cn-relevance-eval.template.json`（新）：12 条中文查询模板，`expected` 全空待人工标注。
- `THIRD-PARTY-NOTICES.md`（新）：第三方组件与许可初版工作稿，打包前需补全文。

上一批 0.1.11（同样已在仓库）：CLAP 双塔语义检索（`semantic_vector.rs`）、波形「暂不可用」修复（`player.rs` Mutex 排队 + `waveform-data.ts` 退避重试）、模型下载队列与桥契约（`model_download.rs` / `audio_text.rs` / `audio_text_bridge.py`）。

## 6. 验证结果（0.1.21 本批 / 0.1.20 历史）

**0.1.21 本批**

| 项 | 结果 |
|---|---|
| Rust | `cargo check --all-targets --offline` 通过；全量 `cargo test --bin sfx-tools --offline` **176 通过 / 0 失败 / 6 忽略**；新增 `metadata_toggle_only_adds_embedded_fields_to_text_search` / `metadata_text_extracts_bwf_fields_and_skips_non_wav` / `write_back_refreshes_metadata_index_for_search` 三项。历史 flaky 用例 `real_worker_cancel_resume_and_single_task_guard` 本轮显示 `ok`；「删除增量编译锁目录失败」告警不影响结果 |
| 前端 | `npx tsc --noEmit` 通过；`node --experimental-strip-types --test tests/preferences.test.mjs` 2/2；`node --test tests/waveform-data.test.mjs` 10/10 |
| CDP 端到端 | `app_version`=v0.1.21；两个 `.search-toggle`「跨语言」「含元数据」初始 `aria-pressed=true`、placeholder「…名称、路径、标签、元数据…」；手动关后按钮 `false`、placeholder 回退，localStorage `metadataSearch` 仍 `true`；设置页复选框默认勾选，改 `false` 后工具栏同步、reload 保持、恢复后同回 `true`；`catalog_list` 带 `metadata:true/false` 均被接受（空查询总数同为 2169）。暗／亮 7 张截图在 `D:/My-Temporary/sw-debug/v21-*.png` |
| EXE | ProductVersion/FileVersion **0.1.21**，25,329,152 字节，2026/11/06 14:28:26；未生成安装包 |
| 版本 | `package.json` / `src-tauri/Cargo.toml` / `Cargo.lock` / `tauri.conf.json` 四处同步；`package-lock.json` 仍为历史 0.1.16，按约定不动 |

**0.1.20（历史）**

| 项 | 结果 |
|---|---|
| Rust | `cargo check --all-targets --offline` 通过；全量 `cargo test --bin sfx-tools --offline` **172 通过 / 1 失败 / 6 忽略**；`player::tests` 22/22。唯一失败 `catalog::tests::real_worker_cancel_resume_and_single_task_guard` 是既有时间敏感／竞态用例：单独重跑与用 0.1.19 HEAD 版 `catalog.rs` 对照重跑都超时，0.1.18 验收记录也标过「既有偶发」，**不是本批回归，不要为它改产品代码** |
| 前端 | `npx tsc --noEmit` 通过；`node --test tests/waveform-data.test.mjs` 10/10 |
| CDP 端到端 | `app_version`=0.1.20；首屏 100 个小波形全部 `is-simple`、无「前段」标签；库中 3534 秒素材的简化／完整波形都 `analyzed_duration=3534`、`truncated=false`（512／4096 峰）；跨语言 `rain` 2→4、`雨` 2→4、`雨音` 0→4，`thunder`／`雷` 0→0（不伪造命中）；`settings_output_devices` 枚举出系统默认与 UR22C 等真实设备；`settings_cache_info` 返回 root／enabled／四组统计；`settings_backup_create` 实产出 `backup-20261106-130448-25919e3f`，catalog 快照 2,150,400 字节、`integrity=ok`、含 manifest。暗／亮两色 12 张截图在 `D:/My-Temporary/sw-debug/v20-*.png` |
| EXE | ProductVersion/FileVersion **0.1.20**，ProductName = SFX Tools，25324544 字节，写入 2026/11/06 13:01:14；未生成安装包 |
| 历史真实文件 | 0.1.12 批次 `real_ffmpeg_bwf_roundtrip`（`#[ignore]`，已实跑）：FFmpeg 生成带 bext 的 BWF → 写回 iXML → 音频 data chunk 逐字节不变，bext 与 iXML 并存 |

## 7. 未完成 / 风险（勿当已完成）

1. **中文相关性评测指标仍是零**：工具与模板就位，评测集 `expected` 全空，需人工标注后运行；`search_text` 跑通 ≠ 检索质量达标。
2. **BWF 面板未做真实 GUI 手测**；只测了本机 FFmpeg 生成的 BWF，Soundminer / Basehead / Pro Tools 导出文件未逐个对照；`axml`/`chna`/`cart`/`ds64` 只透传不解析。
3. **向量索引真实万条**耗时/失败隔离/断点续建未实测；真实万条是发布闸门，十万条专项后置。
4. **许可**：`THIRD-PARTY-NOTICES.md` 为初版，多个组件缺许可全文；`bundle.active` 仍为 false；ONNX 转换产物的声明义务需我们补齐。
5. 真实 GUI/听审、PT 真实工程导入、生产支付、S1.4 安装升级实测仍未完成；AAF/OMF 真正解析与 PT Spotting 未开始（目前只有只读预览与状态文案，不提前伪造工程写入）。
6. 0.1.20 新增未收口：长音频首次解码比旧的截断方案慢（靠缓存与预生成摊薄，未做 WAV PCM 直读快路径）；文件头时长缺失的异常输入仍退回 2400 万采样安全上限并标「前段」；`WAVE_CACHE_VERSION` 5→6 后旧片段缓存成为孤儿文件，可在设置页一次清空；6 张参考图只作视觉对照，未逐像素复刻。
7. 待人类拍板：「按频谱着色」是否下线、列表小波形是否加高到 44–48px、三版候选图标定版、是否生成安装包。
8. 更名只覆盖工程与应用界面；知识库历史文档里的旧名按存证保留。
9. 0.1.21 新增未收口：旧库 `meta_search` 为 `NULL`，必须重扫一次才能参与检索（BWF/iXML 回写只刷新单条）；索引只覆盖 WAV/BWF 的 bext／iXML 已知字段，`axml`／`chna`／`cart`／`ds64` 与 MP3／FLAC／AIFF 容器标签未纳入；拼接文本不分词、不做字段权重、不接语义向量；CDP 截图尚未逐张人工目视复核。

## 8. 续作顺序（0.1.21 之后）

按 24 号复盘（`24-市面音效库工具对标复盘与改进路线.md`）的中期／战略项，确定性从高到低：

1. **AAF/OMF 真正解析与 PT Spotting** —— 已有只读预览与状态文案，按既定「最后提供测试工程」顺序，不提前伪造工程写入。
2. **感知近重复检测升级 / UCS 结构化管理 / 线索表（Cue Sheet）导出 + 许可证字段** —— 复用现有标签与列模型。
3. **中文相关性评测** —— 0.1.12 工具已交付（hit@k/MRR、评测集模板、面板入口），缺的是人工标注评测集，指标仍为零产出，不得宣称「已达标」。
4. 真实万条索引实测、BWF 面板真实 GUI 手测、许可全文收尾（历史顺延项，见第 7 节）。
5. 每次交付后固定流程：`verify.ps1 -Action test`（除既有 flaky）→ `verify.ps1 -Action build` → 升版本号四处 → 同步 `Documentation.md`/`Plan.md`/`README.md` → 更新本文件、`handoff.md` 与交接 Skill → 提交推送。

## 9. 当前 git 状态（接手时请重新核验，勿以本文缓存为准）

本文件不缓存 git 状态；每次接手第一步务必自己跑：

```bash
git -C 'D:/Ai-Files/Repos/sfx-tools' status --short
git -C 'D:/Ai-Files/Repos/sfx-tools' log --oneline -5
git -C 'D:/Ai-Files/Repos/sfx-tools' log origin/main --oneline -3
```

- 0.1.20 基线提交是 `90d60bd`（0.1.19 列表列重构与音乐库筛选维度条）；**0.1.20 主提交 `b00d1af` 已推送 `origin/main`**（24 个文件、1837 insertions / 382 deletions），其后另有一笔文档状态补充提交。0.1.19 及更早批次早已推送。
- 0.1.21 曾在工作区停留较久，其改动已随 0.1.27 三批提交一并入库；提交时按涉案路径分组，未用 `add -A`。
- **0.1.21–0.1.27 已提交**：2026-11-10 分三批提交——`746c883`（后端插件只读与 `ready_only`）、`355bde4`（前端七项界面收口）、随后一笔文档提交；0.1.22–0.1.26 期间累积的源码改动一并纳入。推送状态以 `git log origin/main` 实况为准。
- 0.1.20 涉及 21 个已跟踪文件与 5 个新文件（`src-tauri/src/multilingual.rs`、`src-tauri/src/settings_ops.rs`、`src/settings-client.ts`、`src/settings-panels.tsx`、`handoff.md`）。
- `src-tauri/icons/icon.ico` / `icon.png`（5 根蓝色声波柱新图标）已在 0.1.27 批次按人类本轮界面反馈一并提交。
- 提交需带身份 `-c user.name=… -c user.email=…`（仓库与全局都未配置，沿用 `SFX Tools Dev <dev@sxftools.dev>`），推送走代理 `-c http.proxy=http://127.0.0.1:7897`；**不自动提交知识库仓**（存在并行会话未提交中间态）。

> 接手第一步建议：先读知识库 `Documentation.md` 最新章节与 `Plan.md` 的 2026-11-06 节，再按本文件第 9 节核验 git，然后按第 8 节推进。

## 10. 0.1.27 状态补记（2026-11-10）

- 交付：成品区 `sfx-tools-v0.1.27/` 四文件（EXE 18,495,488 字节、ico、README、SHA256SUMS），EXE 与源码 release 产物同哈希 `107d5643157ff7ec51c8abc68abc7e52486b8ed3d447d6f697e7778b893139eb`；桌面快捷方式已改指本包（`0.1.26`→`0.1.27` 等长就地改写，备份 `SFX Tools.lnk.bak-20261110-b`）。
- 本批内容：品牌标与图标更新；收藏红心固定 `#e5484d`；变速移到叠层右侧；收藏分组可调高／折叠／隐藏（方向键上增下减）；隐藏不可用素材接入 `catalog_list.ready_only`；音频工具合并回单窗口九标签；三插件 `read_only` 只读能力。
- 验证：`tsc` 0；前端 72 通过；Rust 全量串行 207 通过／0 失败／6 忽略；插件定向 23 通过；release 构建成功；原生窗口 CDP 逐项核对并截图 `D:/My-Temporary/sw-debug/v27-*.png`。`real_worker_cancel_resume_and_single_task_guard` 本轮通过，仍按偶发项对待。
- 插件闸门：release 默认构建三条 feature 全关，设置页开关打开也只会得到 `PLUGIN_DISABLED`；只读能力需 `--features plugin-aaf-import,plugin-pt-overlay,plugin-pt-bouncer` 构建。三份契约 `requires_test_project=true`，真实 AAF/OMF 与 PT 工程仍未验证。
- 未做：真人鼠标点选与听感；带 feature 的 release 插件实测；真实工程样本；跨机与万条素材；许可全文；万条中文相关性评测指标仍为零产出。
- 下一步建议：按第 8 节顺序，优先补「带 feature 的 release 包插件只读实测」与真实 AAF/OMF 样本；界面侧等待人类对本批截图的反馈再决定是否继续微调。
