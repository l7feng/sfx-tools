# Sound Workbench 交接文档（供新 Agent 接手）

> 生成：2026-11-05。当前版本 **0.1.12**（debug/no-bundle，已构建并提交）。
> 本文件自包含，新 agent 无需依赖历史对话或 WorkBuddy Skill 机制即可接手。

## 1. 项目定位与关键路径

| 项 | 路径 |
|---|---|
| 工程仓库 | `D:/Ai-Files/Repos/GitHub/sound-workbench` |
| 知识库项目目录 | `D:/1-个人知识库_Vault/5-领域-A/06-软件制作/54-音效管理软件/02-音效管理软件重写/` |
| **唯一当前状态入口** | 知识库 `Documentation.md`（不是本文件、不是聊天记录） |
| 路线 | 知识库 `Plan.md`；执行约定 `Implement.md`；复盘 `24-市面音效库工具对标复盘与改进路线.md` |
| 许可审计 | 知识库 `25-模型与FFmpeg再分发许可审计.md`；仓库 `THIRD-PARTY-NOTICES.md`（初版，待补全文） |
| 元数据适配 | 知识库 `26-BWF与iXML元数据适配.md` |
| 评测方法 | 知识库 `27-中文相关性评测方法.md`；模板 `tests/cn-relevance-eval.template.json` |
| 活待办看板 | `D:/1-个人知识库_Vault/6-资产-R/01-待办事项/待办清单.md` |
| 交付 EXE | `src-tauri/target/debug/sound-workbench.exe`（0.1.12，24943616 字节，2026/11/05 15:08:14） |
| 交接 Skill（可选） | `C:/Users/Administrator/.workbuddy-ai/skills/sound-workbench-handover/`（SKILL.md + references/handover-state.md）；zip 同目录 |

技术栈：Tauri 2 + React/TypeScript + Rust + SQLite/FTS5 + Python ONNX Runtime 桥（独立 CPU 推理）。

## 2. 不可妥协的约定

1. **唯一当前状态入口是知识库 `Documentation.md`**；不得另建第二份当前进度。每次交付后更新它。
2. 四文件规则：`Prompt.md` / `Plan.md` / `Implement.md` / `Documentation.md` 分工明确。
3. 构建用 `verify.ps1 -Action build`：**debug / --no-bundle，不自动打安装包**。更新前按授权后台结束本项目 `sound-workbench.exe`（Implement.md 授权）。
4. **语义边界不可伪造**：文件齐备 / ONNX 张量自检 / `encode_text` / `encode_audio` 跑通，都不等于「音频语义检索可用」，也不等于「中文相关性评测通过」。评测工具已就位但**指标仍是零产出**；真实万条是正式发布闸门；十万条专项后置。
5. 只调用本机独立 FFmpeg，不捆绑分发；不操作用户素材库内容（只读；元数据写回仅在用户点击保存时发生）。
6. 提交必带身份：仓库与全局都没有配 `user.name/user.email`，`git commit` 需 `-c user.name=… -c user.email=…`；推送走代理 `-c http.proxy=http://127.0.0.1:7897`。

## 3. 构建与测试命令

```powershell
# 仓库根目录；verify.ps1 内置工具链环境与强制结束 EXE
& 'D:\Ai-Files\Repos\GitHub\sound-workbench\verify.ps1' -Action test    # Rust 全量测试
& 'D:\Ai-Files\Repos\GitHub\sound-workbench\verify.ps1' -Action build   # debug 构建 + 更新 EXE
npm run build                                                          # 仅前端 tsc + Vite

# Python 桥单测（须用 AI venv 解释器）
& 'D:\Ai-Files\Tools\BuildEnv\sound-workbench-ai\Scripts\python.exe' tests/audio_text_bridge_test.py

# 前端波形调度器单测
node --experimental-strip-types --test tests/waveform-data.test.mjs

# 真实 FFmpeg BWF 往返测试（默认 ignored）
& "$env:CARGO_HOME/bin/cargo.exe" test --manifest-path src-tauri/Cargo.toml real_ffmpeg_bwf -- --ignored
```

工具链环境（verify.ps1 已设）：
- `CARGO_HOME=D:/Ai-Files/Tools/BuildEnv/sound-workbench-rust/cargo`
- `RUSTUP_HOME=D:/Ai-Files/Tools/BuildEnv/sound-workbench-rust/rustup`
- `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`（Rust 1.99.1 MSVC）
- Python venv：`D:/Ai-Files/Tools/BuildEnv/sound-workbench-ai/Scripts/python.exe`（3.12.10, numpy 2.3.4, onnxruntime 1.23.2, tokenizers 0.22.2）

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

## 5. 0.1.12 已交付（本批）

- `src-tauri/src/bwf_metadata.rs`（新）：RIFF/WAVE 容器读写。解析 `bext`（描述/制作者/参考号/日期时间/timeReference）与 `iXML`（PROJECT/SCENE/TAKE/TAPE/NOTE + 私有 `SW_` 字段）；命令 `bwf_read_metadata` / `bwf_write_metadata` 只接受素材 id（经 `catalog.resolve_asset` 解析），不接受前端传任意路径。
- 回写策略：只替换 bext 与 iXML，其余 chunk 连头带数据逐字节复制（含奇数补位）；缺 bext 时插在 `fmt ` 后；重算 RIFF 长度；同目录临时文件 → 回读校验 → 原子替换，失败保留原文件。RF64 只读、非 RIFF 拒绝、超长 bext 字段报错不截断。
- `src/bwf-metadata.tsx`（新）：详情栏「BWF / iXML 元数据」面板，9 个字段可读可写，显示字节变化与「目录需重扫」提示。
- `src-tauri/src/semantic_eval.rs`（新）：中文相关性评测。评测集 JSON（查询 + 期望文件名片段），指标 hit@1/3/5/10 + MRR，未标注查询不进分母；命令 `semantic_vector_eval`（绝对路径、1MB 上限）与 `semantic_vector_eval_template`；索引为空直接报错，不出假指标。
- `src/cn-eval.tsx`（新）：CLAP 面板底部「中文相关性评测」区块。
- `semantic_vector::search_text` 由私有改 `pub(crate)` 供评测复用，行为未变。
- `tests/cn-relevance-eval.template.json`（新）：12 条中文查询模板，`expected` 全空待人工标注。
- `THIRD-PARTY-NOTICES.md`（新）：第三方组件与许可初版工作稿，打包前需补全文。

上一批 0.1.11（同样已在仓库）：CLAP 双塔语义检索（`semantic_vector.rs`）、波形「暂不可用」修复（`player.rs` Mutex 排队 + `waveform-data.ts` 退避重试）、模型下载队列与桥契约（`model_download.rs` / `audio_text.rs` / `audio_text_bridge.py`）。

## 6. 验证结果（0.1.12）

| 项 | 结果 |
|---|---|
| Rust | 全量 158 项：**151 通过 / 6 忽略 / 1 失败**。唯一失败 `catalog::tests::real_worker_cancel_resume_and_single_task_guard` 为**既有偶发超时**（本轮另一轮全量跑通过，0.1.11 记录同样漂移），与本次无关。`bwf_metadata` 10 项、`semantic_eval` 6 项全绿 |
| 真实文件 | `real_ffmpeg_bwf_roundtrip`（`#[ignore]`，已实跑）：FFmpeg 生成带 bext 的 BWF → 写回 iXML → 音频 data chunk **逐字节不变**，bext 与 iXML 并存 |
| 前端 | `npm run build`（tsc + Vite）通过 |
| EXE | ProductVersion/FileVersion **0.1.12**，24943616 字节，写入 2026/11/05 15:08:14；未生成安装包 |
| Git | 0.1.10/0.1.11/0.1.12 均已提交并推送，工作区 clean |

## 7. 未完成 / 风险（勿当已完成）

1. **中文相关性评测指标仍是零**：工具与模板就位，评测集 `expected` 全空，需人工标注后运行；`search_text` 跑通 ≠ 检索质量达标。
2. **BWF 面板未做真实 GUI 手测**；只测了本机 FFmpeg 生成的 BWF，Soundminer / Basehead / Pro Tools 导出文件未逐个对照；`axml`/`chna`/`cart`/`ds64` 只透传不解析。
3. **向量索引真实万条**耗时/失败隔离/断点续建未实测；真实万条是发布闸门，十万条专项后置。
4. **许可**：`THIRD-PARTY-NOTICES.md` 为初版，多个组件缺许可全文；`bundle.active` 仍为 false；ONNX 转换产物的声明义务需我们补齐。
5. 真实 GUI/听审、PT 真实工程导入、生产支付、S1.4 安装升级实测仍未完成；AAF/OMF 解析与 PT Spotting 未开始。

## 8. 续作顺序（0.1.12 之后）

1. 标注 `cn-relevance-eval` 评测集并跑出第一组 hit@k / MRR，按「模型 revision + 索引条数 + 评测集版本 + 日期」记入 `Documentation.md`。
2. 在授权素材库上跑真实万条索引，记录耗时与失败分布（不伪造结果）。
3. BWF 面板真实 GUI 手测，并用 Soundminer/PT 导出的真实文件对照读写。
4. 制作适配器续项：AAF/OMF 解析、PT Spotting、线索表/许可证字段导出（24 号复盘中期项，iXML/BWAV 已完成）。
5. 许可收尾：补齐 `THIRD-PARTY-NOTICES.md` 全文、模型落盘补 `LICENSE`/`NOTICE`/`MODIFICATIONS.md`、评估从 laion 原始权重自行转换（路线乙）。
6. 每次交付后固定流程：`verify.ps1 -Action test`（除既有 flaky）→ `verify.ps1 -Action build` → 升版本号 → 同步 `Documentation.md`/`README.md` → 更新本文件与交接 Skill → 提交推送。

## 9. 当前 git 状态

工作区 clean，无未提交改动。最近提交：

| commit | 内容 |
|---|---|
| `d3b5537` | 0.1.11：接入 CLAP 音频语义检索并修复波形不可用（含 0.1.10 模型下载队列） |
| 其后一条 | 0.1.12：BWF/iXML 元数据适配与中文评测工具 |

> 接手第一步建议：先读知识库 `Documentation.md` 最新章节与 `Plan.md` 的 2026-11-05 节，再跑 `verify.ps1 -Action test` 确认环境，然后按第 8 节推进。
