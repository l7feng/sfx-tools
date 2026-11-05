# Sound Workbench 交接文档（供新 Agent 接手）

> 生成：2026-11-05。当前版本 **0.1.11**（debug/no-bundle，已构建）。
> 本文件自包含，新 agent 无需依赖历史对话或 WorkBuddy Skill 机制即可接手。

## 1. 项目定位与关键路径

| 项 | 路径 |
|---|---|
| 工程仓库 | `D:/Ai-Files/Repos/GitHub/sound-workbench` |
| 知识库项目目录 | `D:/1-个人知识库_Vault/5-领域-A/06-软件制作/54-音效管理软件/02-音效管理软件重写/` |
| **唯一当前状态入口** | 知识库 `Documentation.md`（不是本文件、不是聊天记录） |
| 路线 | 知识库 `Plan.md`；执行约定 `Implement.md`；复盘 `24-市面音效库工具对标复盘与改进路线.md` |
| 活待办看板 | `D:/1-个人知识库_Vault/6-资产-R/01-待办事项/待办清单.md` |
| 交付 EXE | `src-tauri/target/debug/sound-workbench.exe`（0.1.11，24618496 字节） |
| 交接 Skill（可选） | `C:/Users/Administrator/.workbuddy-ai/skills/sound-workbench-handover/`（SKILL.md + references/handover-state.md）；zip 同目录 |

技术栈：Tauri 2 + React/TypeScript + Rust + SQLite/FTS5 + Python ONNX Runtime 桥（独立 CPU 推理）。

## 2. 不可妥协的约定

1. **唯一当前状态入口是知识库 `Documentation.md`**；不得另建第二份当前进度。每次交付后更新它。
2. 四文件规则：`Prompt.md` / `Plan.md` / `Implement.md` / `Documentation.md` 分工明确。
3. 构建用 `verify.ps1 -Action build`：**debug / --no-bundle，不自动打安装包**。更新前按授权后台结束本项目 `sound-workbench.exe`（Implement.md 授权）。
4. **语义边界不可伪造**：文件齐备 / ONNX 张量自检 / `encode_text` / `encode_audio` 跑通，都不等于「音频语义检索可用」或「中文相关性评测通过」。真实万条是正式发布闸门；十万条专项后置。
5. 只调用本机独立 FFmpeg，不捆绑分发；不操作用户素材库。

## 3. 构建与测试命令

```powershell
# 仓库根目录；verify.ps1 内置工具链环境与强制结束 EXE
& 'D:\Ai-Files\Repos\GitHub\sound-workbench\verify.ps1' -Action test    # Rust 全量测试
& 'D:\Ai-Files\Repos\GitHub\sound-workbench\verify.ps1' -Action build   # debug 构建 + 更新 EXE

# Python 桥单测（须用 AI venv 解释器）
& 'D:\Ai-Files\Tools\BuildEnv\sound-workbench-ai\Scripts\python.exe' tests/audio_text_bridge_test.py

# 前端波形调度器单测
node --experimental-strip-types --test tests/waveform-data.test.mjs
```

工具链环境（verify.ps1 已设）：
- `CARGO_HOME=D:/Ai-Files/Tools/BuildEnv/sound-workbench-rust/cargo`
- `RUSTUP_HOME=D:/Ai-Files/Tools/BuildEnv/sound-workbench-rust/rustup`
- `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`（Rust 1.99.1 MSVC）
- Python venv：`D:/Ai-Files/Tools/BuildEnv/sound-workbench-ai/Scripts/python.exe`（3.12.10, numpy 2.3.4, onnxruntime 1.23.2, tokenizers 0.22.2）

## 4. 模型与 ONNX 契约（固定版本，勿随意升级）

| 模型 | model_id | revision |
|---|---|---|
| 文本 E5 | `intfloat/multilingual-e5-small` | `614241f622f53c4eeff9890bdc4f31cfecc418b3` |
| 音频-文本 CLAP | `Xenova/clap-htsat-unfused` | `c28f2883575e590e04d3146ff0713c2448d691ba` |

- CLAP ONNX SHA256：audio `3fcff2c8824e7bcb83a983f2a49edab3b60cbcf4872ac70efee517355173bd1f`；text `1a3df8b197e249816e08415fd040434c44762b2eea7eb7bf8a48a0f0bf3c14e5`。
- 音频塔输入实测 `[1, 1, 1001, 64]`（`preprocessor_config.json`：`nb_max_frames=1000`+1、`feature_size=64`）。
- 双塔各输出 **512 维**；`encode_audio` 返回 L2 归一化有限向量。
- 下载代理 `http://127.0.0.1:7897`（Hugging Face 直连超时，必须走代理）。
- 上游 `laion/clap-htsat-unfused` 为 Apache-2.0；ONNX 转换产物再分发条款**尚未单独核验**。
- 长音频确定性取开头 480000 samples（10s），未与官方 `ClapFeatureExtractor` 逐帧对照。

## 5. 0.1.11 已交付（本批）

- `src-tauri/src/semantic_vector.rs`（新）：CLAP 双塔向量索引，独立 SQLite 按固定 revision 隔离；命令 `semantic_vector_build/status/cancel/search_text/search_audio`；批量 `encode_audio_batch`（一次加载 audio ONNX 处理 1–16 条）、余弦 Top-K、以音找音排除自身、进度/取消/失败隔离。
- `src-tauri/src/audio_text.rs`：暴露 `pub fn run_bridge_op(...)`（`OUTPUT_LIMIT` 4MB），供向量索引复用固定 revision/路径/进程约束。
- `audio_text_bridge.py`：新增 `encode_audio` / `encode_audio_batch`（FFmpeg 48k 单声道 → Slaney log-mel 64×1001 → ONNX → 512 维 L2 归一化）。
- `src/audio-intelligence.tsx`：新增「CLAP 语义检索」面板（构建/取消索引、中文文本检索、以音找音）；文案改为「已接入本地离线检索，未做中文评测」。
- 波形修复：`player.rs` AtomicBool 单槽互斥 → `Mutex<()>` 在 `spawn_blocking` 内排队（消除「波形分析进行中」）；`waveform-data.ts` 队列 24→64 + `withWaveformRetry`（仅瞬时竞争错误退避，永久错误立即上抛）。
- 版本升 0.1.11：`src-tauri/Cargo.toml`、`package.json`、`package-lock.json`、`src-tauri/tauri.conf.json`、`src/settings.tsx`。
- 文档：知识库 `Documentation.md` 追加 `## 2026-11-05 · 0.1.11 …`；`Plan.md` 回填；`README.md` 追加 0.1.11 段。

## 6. 验证结果（0.1.11）

| 项 | 结果 |
|---|---|
| Rust | 141 项：**135 通过 / 1 失败 / 5 忽略**。唯一失败 `catalog::tests::real_worker_cancel_resume_and_single_task_guard` 为**既有超时问题，与本次无关**。`semantic_vector` 6 项、`audio_text` 2 项、波形锁测试通过 |
| Python | `tests/audio_text_bridge_test.py` **21/21** 通过 |
| 前端 | `tests/waveform-data.test.mjs` **9/9** 通过；tsc+Vite+Tauri debug 构建通过 |
| EXE | ProductVersion/FileVersion **0.1.11**，24618496 字节，写入 2026/11/05 12:56:18；未生成安装包 |

## 7. 未完成 / 风险（勿当已完成）

1. CLAP **中文相关性评测未执行**；`encode_audio`/`search_text` 跑通 ≠ 检索质量达标（评测集已预留未填）。
2. 向量索引在**真实万条**上的耗时/失败隔离/断点续建未实测；真实万条仍是正式发布闸门，十万条专项后置。
3. ONNX 再分发许可未审计；长音频未与官方逐帧对照。
4. 真实 GUI/听审、PT 真实工程导入、生产支付、S1.4 安装升级实测仍未完成。

## 8. 续作顺序（0.1.11 之后）

1. 真实万条 / 中文相关性评测：在授权素材库上跑 `semantic_vector_build` + 中文 `search_text`，记录命中率与耗时（不伪造结果）。
2. 制作适配器：iXML/BWAV 读取+回写、AAF/OMF 解析、PT Spotting、线索表/许可证字段（见 24 号复盘）。
3. ONNX/FFmpeg 再分发许可审计，为正式打包做准备。
4. 每次交付后固定流程：`verify.ps1 -Action test` 全绿（除既有 flaky）→ `verify.ps1 -Action build` → 升版本号 → 同步 `Documentation.md`/`README.md` → 更新本文件与交接 Skill。

## 9. 当前 git 状态（未提交，0.1.11 改动）

已修改：`README.md`、`package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/src/main.rs`、`src-tauri/src/model_registry.rs`、`src-tauri/src/player.rs`、`src-tauri/tauri.conf.json`、`src/audio-intelligence.tsx`、`src/audio-intelligence.css`、`src/settings.tsx`、`src/tools-page.tsx`、`src/waveform-data.ts`、`tests/waveform-data.test.mjs`。

新增（未跟踪）：`audio_text_bridge.py`、`src-tauri/src/audio_text.rs`、`src-tauri/src/model_download.rs`、`src-tauri/src/semantic_vector.rs`、`tests/audio_text_bridge_test.py`、`HANDOVER.md`（本文件）。

> 接手第一步建议：先读知识库 `Documentation.md` 最新章节与 `Plan.md` 的 2026-11-05 节，再跑 `verify.ps1 -Action test` 确认环境，然后按第 8 节推进。
