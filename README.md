# SFX Tools（音效与音乐素材工作台）

当前同步 0.1.21 开发源码，仅用于私有开发。可运行版本入口：`D:/Ai-Files/Repos/GitHub/sfx-tools/src-tauri/target/debug/sfx-tools.exe`（debug/no-bundle，非安装包，0.1.21／25,329,152 字节／2026-11-06 14:28）。源码构建及验证入口见下文。首次 Git 提交从本次源码快照开始，不包含本机素材库、账号数据及构建缓存。

音效管理软件独立重写，开发代号。当前推进二三期功能及0.1.5底部工具/完整波形，实际验证与当前状态以知识库Documentation为准；下文旧阶段内容保留为历史记录。

计划与唯一进度：D:/1-个人知识库_Vault/5-领域-A/06-软件制作/54-音效管理软件/02-音效管理软件重写/Documentation.md。

验证工程不扫描用户音效库、不修改素材源。测试使用生成的合成样本；S1.0合成音与普通拖出已有人工反馈及散列核验，不代表产品音频链完成。

## 公司电脑验证入口

运行 `powershell -ExecutionPolicy Bypass -File ./verify.ps1 -Action check` 检查；`-Action build` 构建不打包的调试程序；`-Action dev` 启动原生窗口。Rust安装必须先完成。脚本仅为当前公司电脑环境指定D盘工具链，不代表其他设备已配置。

`build/s1-samples/` 是FFmpeg生成的0.5秒合成音，包含WAV PCM16、AIFF PCM16、FLAC和MP3，未使用用户素材；不进入Git。该目录存在不代表解码或真实播放已通过。全部实际验收记录仍以知识库Documentation为准。

## Rust就绪前的验证

- `powershell -ExecutionPolicy Bypass -File ./generate-fixtures.ps1`：生成缺失合成样本，核验已有样本，不覆盖；包含四种格式和5秒长WAV。
- `node --experimental-strip-types --test tests/probe-session.test.mjs`：运行前端状态与防重入测试，使用模拟IPC，不代表真实Rust或声卡结果。
- `npm run build`：检查TypeScript并构建验证窗口。

原生测试使用`verify.ps1 -Action test`。脚本通过进程级RUSTUP_TOOLCHAIN复用实际安装的stable，但必须严格匹配1.99.1及MSVC host；不匹配即停止，且禁用自动安装。项目rust-toolchain.toml保留预期版本，不宣称其他机器已配置。

真实验收步骤见知识库项目`05-S1.0真实验收矩阵.md`；唯一当前进度仍是Documentation.md。库外`build/s1-validation/`保留本阶段具体检查证据。

## 查看Rust实时下载进度

双击本目录`查看Rust下载进度.cmd`即可，每秒显示百分比、下载大小和速度，按Ctrl+C退出。主脚本为`watch-rust-download.ps1`，只读观察本项目Rust 1.99.1下载缓存，不启动或停止安装；关闭窗口不影响安装。下载100%后仍区分解压与编译器可运行检查。

## S1.1界面样稿

在工程运行 `npm run dev`，浏览器打开 `http://127.0.0.1:5173/`。保留 `?probe` 为S1.0验证页面；浏览器不提供Tauri原生命令，需要在原生壳内使用。

12条内置演示素材支持分类、搜索、格式筛选、排序、收藏、详情折叠及模拟播放进度。不会读取磁盘、播放音频或写入收藏；刷新重置。添加素材库采用“选择演示目录→预览→完成演示”，选择变化使预览失效。

本轮没有重建或替换此前S1.0 EXE；当前样稿交付为Vite页面。`node --experimental-strip-types --test tests/demo-data.test.mjs tests/probe-session.test.mjs`检查过滤、时间边界及既有探针状态逻辑。自动浏览器访问未获授权，视觉和真实点击尚待人类验收。
## S1.2真实目录

启动 `src-tauri/target/debug/sound-workbench.exe`。原生主入口提供SQLite目录与后台扫描；浏览器默认仍显示演示样稿。`?catalog`是目录页，`?demo`是样稿，`?probe`保留技术探针。SQLite内嵌编译，无需另装服务。

添加素材库：粘贴绝对文件夹路径，检查目录后启动。目录分页100条，筛选仅当前页。任务面板支持取消、继续与重新扫描；不修改源音频、不跟随目录联接。数据库由Tauri app_local_data_dir提供，应用标识local.soundworkbench.desktop，首次启动创建catalog.sqlite；同库只允许一个进程。元数据未知时长按未知显示，播放器本阶段未接入。

12项前端逻辑测试与16个Rust用例分别通过；原生调试构建通过。测试证据build/s1.2-validation。万条测试为46字节合成WAV，不代表真实库吞吐。新版窗口及IPC仍待用户手动验收，知识库08提供步骤。
## 导入体验增强

支持把单个文件夹拖到原生窗口、原生选择文件夹、手动输入；均检查目录后再导入。最近12条有效路径保存在本机数据库，重启保留，取消或无效路径不写入历史。左侧原路径文件树来自已登记记录，点击目录查看该子树，全部素材取消范围；树不会改动磁盘目录。

本轮按用户要求清空正式库2187条素材索引和3条任务，音频仍在原位。新版13项前端测试、19项Rust测试及构建通过；原生选择器、真实拖入和树操作仍待人工验收。知识项目09说明SoundSeeker静态模块依据及下一批改进，未声称实测原产品UI。

## 0.1.1顶栏与波形

音效库、音乐库、音频工具在顶栏切换；素材位置是原路径筛选。音乐库可添加文件夹或接收音效库所选引用，源音频不移动，相同文件共用ID与标注。工具页的AI/PT检索与预览作用于所有登记素材。

设置支持密度、启动页、默认音量、配色与布局记忆。四边折叠保留恢复按钮；列表小波形悬停显示时间，点击或Enter按位置试听，左右键预选位置。波形是真实峰值连续包络，最多前60秒，不是完整频谱。

按人类当前约定，只运行 `verify.ps1 -Action build` 更新 `src-tauri/target/debug/sound-workbench.exe`（debug/no-bundle）；未明确要求时不生成安装包。PT实际工程导入测试最后进行。


## 0.1.2右键菜单与导航

文件夹与素材支持右键菜单（列表也可Shift+F10）；收藏、标签、试听历史、播放列表与常用路径按库持久化。搜索有输入时默认覆盖当前库全部位置，可在搜索栏旁切回当前文件夹；清空搜索恢复原目录。

新建空子文件夹会登记并显示。实际改名/同盘移动先预览，再确认；保留UUID和标注，目标存在时拒绝覆盖。出库移除两库中的实体与关联，保留磁盘；磁盘目录回收包含未入库文件，不能回收时停止，当前仅支持本地固定NTFS卷。

相似入口当前是文本语义检索。叠层/散布可保存可编辑时间轴编排，尚无混音试听和渲染。实际回收站、桌面交互与新菜单听审留人类验收；Agent只用专用临时目录验证非删除磁盘操作。


## 0.1.3叠层混音渲染

音频工具页的叠层可渲染为48kHz/双声道/PCM16 WAV，完成后显式试听或选择目录导出；不自动播放、不改源文件、不自动入库、不覆盖已有导出。支持≤48kHz单/双声道输入，最多200片段、32同时重叠、60分钟；高采样率和多声道明确拒绝，带限重采样留后续。

后台任务有进度和取消，修改编排后旧结果标为过期；会话内按任务ID绑定受控缓存并核验源及输出身份。缓存限制1.5GiB、保留当前会话成功结果供试听；超限停止，需停止试听并关闭应用后清理缓存。关闭应用不保存任务状态，下次需重新渲染，已导出WAV保留。

界面/听审与真实导出由人类验收；离线测试不操作声卡或真实素材。


## 0.1.4紧凑导航与列设置

左侧七个入口改为紧凑按钮，标签/播放列表/常用路径按需浮出选择，路径树单独占据剩余高度滚动。可在浮出列表中筛选，常用路径仍支持右键菜单。

在列表表头右键（或聚焦表头后Shift+F10）打开列设置，勾选显示/隐藏、上下移动调整从左到右的顺序。默认显示素材名称/波形试听/时长/格式，采样率和状态可开启；至少保留一列。宽度按字段身份保存，重排不交换数据或宽度；恢复默认列不重置其他面板。遵守设置中的布局记忆开关。


## 0.1.5底部音频工具与完整波形

播放器下方新增反转、单声道合并/拆分、距离、效果器、叠层与变速。叠层打开已有编排页，其余工具生成独立受控WAV后显式试听/导出，原文件不改；距离是电平/低通近似，效果器为内置增益/低通/高通/单次回声，变速随速变调，不声称保调或外部插件宿主。

底部选中素材先显示快速预览，同时后台完整分析，完成后用全长概览替换，可跨60秒定位；列表小波形保留前段以避免整页扫描长音频。完整峰值流式聚合至最多8192点，有进度、取消、源身份及有界磁盘缓存；概览不是样本级编辑。播放器位置来自真实后端，RAF插值只平滑显示，不触发播放，失联最多前进0.5秒。

音频处理限原素材10分钟、192kHz单/双声道；速度.5–2倍，回声保留一次延迟尾音，慢速可使输出长于10分钟。保持原采样率PCM16（变速调整输出rate），缓存1.5GiB；完整波形不受该处理时长限制。详细原软件静态依据与验收见知识库20计划。

## 0.1.6多阶段框架

“音频工具”页新增真实声学特征索引/以声搜声、模型完整性核验登记、UCS元数据词项建议、SHA256完全重复候选、批量副本导出、SoXR重采样、atempo保调变速与插件文件候选扫描。先在素材库选择条目，再进入工具页；原文件不改，目标文件不覆盖。转换依赖本机独立FFmpeg，构建时记录检测到的路径，也可通过SOUND_WORKBENCH_FFMPEG显式指定；不是EXE自带运行时。新转换入口输出PCM24 WAV，旧底部工具的PCM16/声道限制保持原实现。

底部播放器可切换峰值、峰值+RMS、频谱、频谱+波形、按频率着色。频谱使用最多256个真实时间窗、64个对数频带的概览，可能漏掉窗间短瞬态；不是密集频谱编辑器。声学索引最多读取前30秒/200万帧，不等于音频语义理解。音频-文本模型推理、模型下载队列、AAF/OMF解析和插件宿主尚未实现；界面明确区分不可用能力。

账号/双设备/模拟订单与退款、签名离线凭证仅为本地开发框架，服务须手动启动，流程见[本地开发说明](tools/license-dev-service.md)。不真实收费，基础素材读取与导出不锁定。中文语义评测脚手架evaluate-audio-retrieval.py尚待真实标注/模型结果，未运行时不填命中率。

本批验证：39项前端、16项Python、117项Rust通过；单独真实FFmpeg转换测试通过，默认忽略的其余4项真实环境测试仍未执行。GUI/听审、PT实际写入、真实万条/十万条与正式发布后置。只更新debug EXE，不生成安装包；verify.ps1构建前按已授权范围核对路径并结束本工程进程。

## 0.1.7工具入口复盘

点击顶端“音频工具”始终回到十分类总览，并清除工具搜索，不再因曾打开叠层而只剩叠层模块。搜索只筛选分类入口，不会卸载已打开的面板或中断任务。工具页显示当前库、选择条数和单条名称，可返回同库保留选择与分页。

素材列表上方新增“以声搜声”“UCS建议”“导出 / 转换”，素材右键“查找相似音效”直达声学搜声；设置新增“模型与账号”直达入口。分类打开后展开对应操作；无选择/超过500条、无声学索引、非法保调倍速会给出使用条件。账号入口直接展开开发配置，不会自动启动服务或真实收费。

本批44项前端测试（含离线组件静态渲染）及debug/no-bundle构建通过；没有修改原生处理算法、用户素材或安装包。浏览器验收受既有权限阻止，未把自动组件检查当成真实GUI通过。音频语义推理/模型下载、AAF/OMF解析、外部插件宿主和生产支付仍按上版记录待开发。


## 0.1.9同页工具与分类

底部六工具在当前播放器上方非模态展开，叠层同页，不再跳工具页；仍先生成派生结果再显式试听/导出。目录右键“设为常用并置顶”，树顶部显示常用入口。音乐库展开“音乐分类标签”，按流派/情绪/用途/乐器添加或筛选，支持自定义，原标签保留。

音频工具页只保留第三期制作转换、插件/AAF-OMF状态和PT预览；其余通过顶栏“检索 / 管理”“账号”和设置进入库内面板。邮箱登录/注册已分离，真实邮件验证尚未配置。49项前端与debug构建通过，GUI/听审未实测；桌面快捷方式指向0.1.9，工具出口0.1.8包未重打。

## 0.1.10 模型下载队列与音频-文本契约适配

按21号计划下一批顺序推进二期本地AI线，不把人类操作/真实万条/十万条/PT实测作为编码前置。

- 新增可恢复模型下载：Range 4MiB分块、`.part`断点续传、SHA256/长度校验、原子落盘、单任务互斥、取消、主机白名单与相对路径安全；预置 E5/CLAP 固定 revision 清单，并生成 `model-manifest.json`。Tauri命令 model_download_list/start/status/cancel。
- 新增音频-文本契约适配：Rust模块 `audio_text.rs` 提供 contract_ready 与 status/selftest 命令；Python 桥 `audio_text_bridge.py` 加载固定 revision 的 CLAP ONNX（Xenova/clap-htsat-unfused，revision c28f2883575e590e04d3146ff0713c2448d691ba），做文件契约核验与ONNX张量自检，音频塔输入实测 [1,1,1001,64]，双塔各输出512维向量；encode_text 做文本塔编码。桥严格区分“文件契约/张量自检”与“真实音频语义推理”：未做音频内容语义推理，也不做中文相关性评测。
- model_registry 原硬编码的 audio_text_ready/download_supported 改为真实能力判定（manifest存在且SHA256通过、音频文本契约就绪、下载模块启用的组合）。
- 前端 audio-intelligence.tsx 接入下载队列进度/取消与ONNX自检；tools-page.tsx 文案改为“模型下载管理已实现、语义推理未实现”。
- 修复：text_model_quantized.onnx SHA256 抄写多一位已更正（64位）；已存在但校验失败的损坏文件现仅在下载校验通过后才替换。
- 验证：model_download 8项、audio_text 2项、model_registry 2项 Rust 测试通过；audio_text_bridge_test.py 8项通过且真实 status/selftest/encode_text 跑通；前端与debug构建通过，未生成安装包。catalog::tests::real_worker_cancel_resume_and_single_task_guard 仍为既有超时问题，与本批无关。

边界：音频内容语义推理、中文相关性评测、联网下载切换、ONNX再分发许可审计、真实GUI/听审、万条/十万条、PT真实工程导入与生产支付均未完成；真实万条发布闸门与S1.4安装升级实测保持。

## 0.1.11 CLAP 语义检索接线与波形修复

接续 0.1.10，落地 24 号复盘“CLAP 接入检索”快赢，并修复列表波形“暂不可用”。

- 新增 `src-tauri/src/semantic_vector.rs`：CLAP 双塔向量索引，独立 SQLite 按固定 revision 隔离；命令 `semantic_vector_build/status/cancel/search_text/search_audio`；批量 `encode_audio_batch` 一次加载 audio ONNX 处理多条，余弦 Top-K，带进度/取消/失败隔离，以音找音排除自身。
- `audio_text.rs` 暴露 `run_bridge_op` 供复用；`audio_text_bridge.py` 新增 `encode_audio`/`encode_audio_batch`，真实解码 → log-mel → 512 维 L2 归一化向量（FFmpeg 48k 单声道，长音频确定性取开头 10 秒）。
- 前端 `audio-intelligence.tsx` 新增“CLAP 语义检索”面板：构建/取消索引、中文文本检索、以音找音；文案改为“已接入本地离线检索，中文相关性评测未执行”。
- 波形修复：`player.rs` 的 AtomicBool 单槽互斥改 `Mutex<()>` 在 `spawn_blocking` 内排队；`waveform-data.ts` 队列 24→64，新增 `withWaveformRetry` 仅对瞬时竞争错误退避，永久错误立即上抛。消除“波形暂不可用·点击重试”。
- 验证：Rust 135 通过 / 1 既有超时失败 / 5 忽略（`catalog::tests::real_worker_cancel_resume_and_single_task_guard`，与本次无关），`semantic_vector` 6 项、`audio_text` 2 项、波形锁测试通过；Python 21 项、前端 9 项通过；debug 构建通过，未生成安装包。
- 版本升至 0.1.11；EXE ProductVersion 0.1.11，24618496 字节，写入时间 2026/11/05 12:56:18。

边界：CLAP 中文相关性评测未执行，不宣称检索质量达标；向量索引真实万条稳定性未实测；ONNX 再分发许可未审；长音频未与官方逐帧对照。真实 GUI/听审、PT 实测、生产支付、十万条专项仍保持未完成。
## 0.1.12 BWF/iXML 元数据适配与中文评测工具

接续 0.1.11，落地 24 号复盘中期项中的「iXML/BWAV 读取+回写」，并把「中文相关性评测」从口头闸门变成可复现工具。

- 新增 `src-tauri/src/bwf_metadata.rs`：RIFF/WAVE 容器读写，解析 `bext`（描述/制作者/参考号/日期时间/timeReference）与 `iXML`（PROJECT/SCENE/TAKE/TAPE/NOTE + 私有 `SW_` 字段：分类/关键词/许可证/作者/来源/UCS）；命令 `bwf_read_metadata`、`bwf_write_metadata` 只接受素材 id，不接受前端传任意路径。
- 回写只替换 bext 与 iXML，其余 chunk 连头带数据逐字节复制（含奇数补位）；缺 bext 时插在 `fmt ` 后；重算 RIFF 长度；同目录临时文件 → 回读校验 → 原子替换，失败保留原文件。RF64 只读、非 RIFF 拒绝、超长 bext 字段报错不截断、未知 iXML 元素原样保留。
- 前端新增 `src/bwf-metadata.tsx`：详情栏「BWF / iXML 元数据」面板，9 个字段可读可写，写后提示目录需重扫。
- 新增 `src-tauri/src/semantic_eval.rs` 与 `src/cn-eval.tsx`：中文相关性评测，指标 hit@1/3/5/10 与 MRR；评测集为 JSON（查询 + 期望文件名片段），未标注查询不进分母；索引为空直接报错，不出假指标。模板见 `tests/cn-relevance-eval.template.json`。
- 验证：Rust 全量 151 通过 / 6 忽略 / 1 既有偶发失败；`bwf_metadata` 10 项、`semantic_eval` 6 项全绿；`real_ffmpeg_bwf_roundtrip`（ignored）已实跑，写回后音频 data chunk 逐字节不变；前端 tsc + Vite 通过；未生成安装包。
- 新增 `THIRD-PARTY-NOTICES.md` 初版（第三方组件许可，待补全文）。
- 版本升至 0.1.12；EXE ProductVersion 0.1.12，24943616 字节，写入时间 2026/11/05 15:08:14。

边界：中文评测指标仍为零产出（评测集 expected 全空，需人工标注）；BWF 面板未做真实 GUI 手测，仅用本机 FFmpeg 生成的 BWF 验证，Soundminer/PT 导出文件未对照；只透传 `axml`/`chna`/`cart`/`ds64` 不解析语义；AAF/OMF 与 PT Spotting 未开始；许可声明仍为工作稿，`bundle.active` 未开启。
## 0.1.13 更名 SFX Tools 与 EXE 版本信息补全

按人类指令「改名为 SFX Tools，exe 不管在哪都要有版本信息，更新后检查波形」。

- 更名范围到文件名：`productName` → `SFX Tools`，Cargo 包名与 npm 包名 → `sfx-tools`，产物由 `sound-workbench.exe` 变为 **`sfx-tools.exe`**；窗口标题、侧栏品牌（`SFX TOOLS`）、关于页、`document.title`、`index.html` 标题、单实例窗口匹配串、`verify.ps1` 的结束进程路径同步更新。桌面快捷方式已重建为 `SFX Tools.lnk` 指向新 exe，旧的 `Sound Workbench.lnk` 已移入回收站。
- EXE 版本资源补齐（来源：`tauri.conf.json` 的 `bundle.publisher` / `bundle.copyright` + `productName`）：`ProductName=SFX Tools`、`FileDescription=SFX Tools`、`CompanyName=SFX Tools`、`LegalCopyright=© 2026 SFX Tools`、`FileVersion/ProductVersion=0.1.13`。此前 CompanyName 取自 identifier 第二段（soundworkbench）、版权为空。
- 数据目录与目录库**刻意保持不变**：identifier 仍为 `local.soundworkbench.desktop`，`%LOCALAPPDATA%` 下的目录库、向量索引、模型登记不受更名影响，避免既有素材索引丢失。
- 波形：0.1.11 的修复保持原样（`player.rs` 用 `Mutex<()>` 在 `spawn_blocking` 内排队、`waveform-data.ts` 队列 64 + `withWaveformRetry` 仅对瞬时竞争退避），本轮未改波形代码；Rust 152 项与前端波形 9 项测试通过。真实 GUI 表现待人类手测。
- 验证：Rust 152 通过 / 6 忽略 / 0 失败（既有偶发超时本轮通过）；前端 tsc + Vite 通过；EXE ProductVersion 0.1.13，24943616 字节，写入时间 2026/11/05 15:51:18；未生成安装包。

边界：更名只覆盖工程与应用界面、快捷方式；知识库历史文档中的旧名按存证保留。`OriginalFilename` 字段 tauri-build 不写入，属性里为空。未做真实 GUI 手测（人类安排后测）。

## 0.1.16 字号与字体可读性调整（含 0.1.15）

按人类反馈「字号有点小，字体优化，这个字体看着累」调整；0.1.15 的版本号修复与常用路径颜色标记一并提交（0.1.14 为过渡版本号，无独立交付）。

- 诊断：渲染环境 2560×1440、系统缩放 150%（`innerWidth 1440`、`devicePixelRatio 1.5`），12px CSS 约合 18 物理像素，属可读区间；问题在档位本身——全库 216 处字号声明最小值到 8px，表头/表体 10px、副路径 9px、状态栏 10px，中文在该区间笔画糊连。
- 字号：246 处整体上移一档，映射单调不跨档 `8→10 9→10 10→11 11→12 12→13 13→14`，14px 以上（标题、时间码、播放键、图标字号）不动；脚本只匹配 `font-size:Npx` 与 `font:` 简写中的字号，不触碰高度、边距、行高。
- 阅读面：在 `workspace-shell.css` 末尾追加「可读性基线」，`th` 12px/30px、`td` 12px/53px、`.asset-name strong` 13px/1.5、`.asset-name small` 11px/1.45、`.asset-name` 最小高 42px、状态栏 11px/28px、导航项 35px、`.pins-select` 26px、分页按钮 30px。
- 字体：`--mono` 由 `Consolas,'Cascadia Mono',monospace` 改为 `'Cascadia Mono',Consolas,'Microsoft YaHei UI',monospace`——原栈无中文回退，路径列中文掉宋体、与西文笔画割裂；`--body` 保持雅黑优先并挂 `Segoe UI` 兜底。
- 布局修正：`.sidebar-pins` 由 `column-reverse` 改回 `column`，常用路径标题回到列表上方。
- 验证：`tsc` 与 `npm run tauri -- build --debug --no-bundle` 通过；EXE `FileVersion`/`ProductVersion` = 0.1.16，24963072 字节，写入时间 2026/11/05 18:18；五屏（音效库/音乐库/音频工具/目录右键/色板菜单）经 WebView2 调试端口截图核对，无裁切与重叠。

边界：列表行高 49→53px，每屏少显示约 1 行；`src-tauri/icons/*` 的候选图标仍在试验中，未纳入本次提交；E 盘素材路径未恢复，波形列仍显示「文件缺失」，属数据问题。
## 0.1.17 界面主题化与工具页图标

按人类六条界面反馈落地：目录图标与标题、播放条两行右对齐、删长提示、工具页入口、主题色彩、按钮参考图优化。

- 目录树：`folder-tree.tsx` 用内联 SVG `FolderGlyph` 替换 `▱` 字符，闭合／展开两套造型（有子项且展开时换成开盖形状，选中态跟随 `--accent`）；侧栏标题「原路径目录树」改为「路径目录」。
- 播放条右对齐：`.player-heading` 与 `.wave-view-controls` 改 `justify-content:flex-end`，「待机 尚未播放 波形:…」「波形视图 峰值」两行靠右；删除播放器下方长提示，只保留 `Space 播放／暂停`。
- 二级标签栏：素材库顶栏在库切换右侧新增 `.tool-tabs`，工具页与库内工具抽屉都能横向切换全部 11 个工具页；`openTool` 同步调整——在音频工具页内点库类工具不再跳回素材库；`.tool-directory` 改为只在总览或搜索时出现，打开具体工具后让位给内容区。
- 工具图标：新增 `src/tool-glyphs.tsx`，11 个工具页各一枚 24×24 线性图标（放大镜内电平条／标签／双矩形／双向箭头／插头／三层堆叠／文本行+放大镜／导入托盘／人像／健康曲线／信息圈），另含 `grid`、`back` 两枚动作图标；标签栏 13px、卡片徽章 15px、动作按钮 14px。首版「放大镜内正弦波」在 13px 下糊成一团，改为三条电平条后小尺寸可辨。
- 主题系统：新增 `src/theme.css` 与 `src/theme.ts`——`<html data-theme="dark|light" data-accent="gold|blue|teal|violet">` 共 8 组组合（4 主色 × 亮暗）；`:root` 保留暗色令牌，`[data-theme=light]` 整体覆盖亮色；强调色阶用 `color-mix(in srgb,var(--accent) …)` 派生 `--accent-wash`／`--accent-line`／`--accent-line-strong`／`--accent-hover`／`--wave-fill`／`--wave-line`，一处改主色即全站联动。选择存 `localStorage` 键 `sfx-tools-theme-v1`；`index.html` head 内联首帧脚本先写 `dataset`，避免浅色启动时闪一帧暗底。
- 颜色收编：`workbench.css` 的 `:root` 只留字体与尺寸，颜色全部移交 theme.css；脚本批量把 77 处写死色值映射为变量（含 `#151719ee` → `color-mix(in srgb,var(--bg) 93%,transparent)`、`#818885` → `var(--muted)`）。
- 设置：`显示与布局` 顶部新增「强调色」四个色点（选中态双层描边）+「底色」深色／浅色下拉，与列表密度、波形配色并列。
- 验收：`tsc --noEmit` 与 `npm run tauri -- build --debug --no-bundle` 通过；EXE 0.1.17，24966656 字节，2026/11/06 09:47。像素审计——暗紫主题高饱和像素 285 个落在 220–300° 色相、金色仅 6 个，暗金主题反之（金 61／紫 19），证明强调色真正生效；文字对比度浅色 5.39–5.73:1、暗色 7.08–7.53:1，12px 正文均过 WCAG AA。逐屏截图覆盖工具总览、素材库、设置对话框、目录右键在亮暗两种主题下的表现。

边界：`spectral-view.css` 仍留 12 处频谱画布配色（刻意保持深底，避免浅色下频谱对比不足）；素材标签的 6 种分类色与 `#0006/#0008/#0009` 阴影未收编为变量；波形配色仍是既有「暖金／青绿」偏好项，未与强调色联动；`src-tauri/icons/*` 三版候选图标仍待人类选定，未纳入提交；安装包未生成。

## 0.1.18 波形声道分轨与详情窗口重排

按人类反馈「波形配色只留蓝、白」「波形要能区分立体声与单声道」「详情窗口按参考图整理、标签与元数据可折叠」重做。

- 波形配色：删除「暖金 / 青绿」两套，改为「蓝 / 白」。浅色主题下白色波形自动转墨色（否则白底上不可见）；旧的 `amber`／`mint` 偏好值一律回落到蓝色，不需要手动重设。
- 声道分轨：三处波形数据源都补了每声道包络——`player_waveform`（列表前段 60 秒）、`full_wave`（播放器全长）、`visual_analysis`（峰值/RMS、频谱、组合模式）。立体声按上下两轨绘制、中间一条分隔线、左侧 L / R 标记；单声道仍是单条居中；多声道（5.1 等）回落单条，不硬拆。
- `visual_analysis` 另记每声道 RMS，立体声下「外层峰值 + 内层 RMS」也能分轨显示，不会出现峰值两条、RMS 一条的错位。
- 缓存口径：`WAVE_CACHE_VERSION` 3→4、完整波形 `VERSION` 1→2、可视化 `VERSION` 1→2。旧记录缺分轨字段，必须整批失效重算，靠版本号比按字段兜底更可靠。
- 详情窗口：新建 `src/asset-detail.tsx` 与 `asset-detail.css`，按参考图重排为「文件信息 / 我的标签 / 内嵌元数据 / AI 分类标签 / 原始文件位置」五段，每段可折叠；文件信息是行式「左标签 + 右对齐值」，超长省略并在悬停时给全文。
- 标签与元数据：折叠态只显示标签小圆点或一句话状态（「还没有标签」／「暂无嵌入元数据」），点「编辑」才展开输入框；元数据折叠时右侧是「编辑」，展开后换成 bext / iXML 状态。
- 读取合并：`useBwfMeta` 提到详情层，`BwfMetadataPanel` 新增 `injected` / `useInjected` 注入参数，同一素材只读一次元数据；写回后刷新详情与面板两处。原 `asset-annotations.tsx` 删除。
- 验收：`player::tests` 19 项、`visual_analysis` 4 项、`full_wave` 3 项通过（新增 `stereo_waveform_keeps_per_channel_lanes`）；全量 `cargo test` 152 通过 / 2 失败（`catalog::real_worker_cancel_resume_and_single_task_guard` 的超时断言、`model_download::single_file_full_download_is_verified` 需外网，均为既有偶发，与本轮无关）；`tsc --noEmit` 与 debug 构建通过；EXE 0.1.18，25010176 字节，2026/11/06 10:43。
- 实测：合成一段「左声道前 1.5 秒满、右声道后 1.5 秒满」的立体声与一段单声道，导入临时目录验证——`channel_peaks` 前后半段均值为 0.393 / 0.035，与信号完全一致；单声道 `channel_peaks` 为空、列表按单轨渲染。验收后临时条目已从素材库移除。

边界：列表小波形只有 34px 高，立体声两轨各约 16px，够看出「上下两条 + 分隔线」，看不清左右电平细节；`spectral-view.css` 的画布配色未接新配色；「按频谱着色」模式仍然保留，是否下线待定。

## 0.1.19 列表列重构与音乐库（配乐）窗口

按人类三条反馈落地：列设置删除「状态」、参考图 1 重排列表、参考图 2 优化配乐窗口。裁决为「列结构＋行高紧凑」与「筛选维度条＋BPM/调性列＋音乐详情组」。

- 列模型：`workbench-state.ts` 的 `ColumnId` 由 6 项扩到 12 项，删掉 `status`，新增 `#`／分类／声道／收藏／播放列表，以及只在音乐库出现的 BPM／调性（`musicOnly`）。`columnDefinitions` 每项带 `widthIndex` 与 `musicOnly`／`sortable` 标记，`visibleColumns(layout,library)` 按库过滤，列设置里音效库看不到 BPM／调性。
- 旧布局迁移：6 项／5 项旧列宽数组按列名回填到新索引；纯旧列序、或「旧五列在前＋新列被追加在后」的升级中间态，整体回到新默认列序，同时清掉状态列残留的隐藏项。用户已经调过顺序（存档里出现新列且不是追加型）则继续沿用。
- 紧凑列表：行高 53→38px（舒适密度 48px），素材名单行显示并去掉音频扩展名，完整文件名与路径移到悬停提示，小波形 34→28px。状态信息不丢——文件缺失／已变化／读取失败的行在名称前加红色小圆点，悬停看全文，不再占一列。
- 表头排序：文件名／时长／采样率三列可点击切换升降序并显示 ▲▼，`aria-sort` 同步；后端 `sort` 白名单补 `name_desc`／`duration_desc`／`rate_desc`，NULL 时长仍排最后；顶部「排序方式」下拉显示当前字段而非空值。
- 行内操作：♡ 即时收藏／取消（`catalog_set_favorite`），≡ 弹出播放列表浮层，可选现有歌单或新建后直接加入（`library_playlist_create` + `library_playlist_add`）。新建 `playlist-picker.tsx`／`playlist-picker.css`，所有写入只改本机目录、不动原音频。
- 音乐库筛选维度条：删除折叠式的 `music-categories.tsx`，改为 `music-filters.tsx`／`music-filters.css` 常驻一行「曲风／情绪／用途场景／乐器／人声／BPM／调性／时长」下拉。维度值来自已有标签（带计数）＋内置建议，可多选并取交集；后端 `catalog_list` 新增 `tags:Vec<String>`，逐项生成 EXISTS 条件，上限 8 项。已选条件以 chip 显示，可单个 × 或「清除全部」。每个下拉底部有「自定义…＋加到素材」，把 `维度:值` 写进所选素材的标签；曲风兼容早期 `曲风:` 前缀。
- 时长维度走真实 `duration` 区间（后端新增 `min_duration`／`max_duration`，未知时长一律排除），不伪造 BPM／调性自动分析；BPM／调性列从 `BPM:`／`调性:` 标签读取，未标注显示「—」。
- 音乐详情组：`asset-detail.tsx` 新增 `library` 参数，音乐库时多出「音乐信息」组（艺术家／专辑／BPM／调性／年份／曲风），编辑表单写回同维度标签并保留其他标签，原音频不改写。

验收：全量 `cargo test --bin sfx-tools` 156 通过 / 0 失败 / 6 忽略（新增 `sorts_descending_and_filters_by_duration_range`、`multiple_dimension_tag_filters_intersect`）；`tsc --noEmit` 与 `npm run tauri -- build --debug --no-bundle` 通过；EXE 0.1.19，25026560 字节，2026/11/06 11:27。CDP 端到端模拟 0.1.18 存档升级：音效库列序变为 `# / 波形 / 文件名 / 分类 / 时长 / 声道 / 采样率 / 收藏 / 播放列表`，音乐库多出 BPM／调性，行高实测 38px；点表头排序箭头切换为降序、播放列表浮层、维度多选 chip、时长「3 分以上」把 2169 条筛到 180 条，逐项截图核对暗色／亮色无裁切。

边界：列多了之后窄窗口仍需横向滚动（1440px 默认窗口基本铺满）；「按频谱着色」模式仍保留，是否下线待定；`src-tauri/icons/*` 三版候选图标仍待选定，未纳入提交；安装包未生成。

## 0.1.20 全长波形、跨语言搜索与设置页扩展

按人类六张参考图与本批文字需求落地；中途人类追加裁决「波形显示全部，不能只显示片段」，据此**推翻**本批早期「简化小波形只解码前 12 秒」的方案，并把历史上「列表波形最多前 60 秒」的截断一并取消。

- **全长波形**：`player.rs` 不再按固定秒数截断，改按文件头时长计算读取上限（多留 1 秒容错）；分桶数超过 2×目标峰值时两两归并、桶宽翻倍，内存恒定且末尾瞬态不丢。`WAVE_CACHE_VERSION` 5→6，旧缓存只含片段峰值，整批失效重算。点击波形试听的位置上界同步改为真实文件时长，不再卡在前 60 秒。
- **简化小波形**（默认开）：仍是**整条音频**，只是压成 512 峰、一条单色包络、不分左右声道；完整规格为 4096 峰且保留分轨。两者缓存目录与 `WaveIdentity.variant` 分离，互不复用。单线／单色只省 SVG 渲染，提速来自缓存复用、流式降采样与全局解码串行锁（`WAVE_DECODE_LOCK`），不靠截断内容。
- **工具页折叠**：进入音频工具自动折叠左素材栏、右详情栏、底部播放器，离开时恢复原布局；可在设置里关闭。
- **跨语言搜索**：`multilingual.rs` 本地静态中／英／日词表，输一种语言同时命中三种写法，同一词的候选 OR、词与词之间 AND；所有候选仍走 SQL 绑定参数，标签／分类／格式／时长／收藏／播放列表筛选不受影响。搜索条有「跨语言」开关，设置里可关。
- **设置页**：分类由 5 个扩到 7 个。新增输出设备下拉（`cpal = "=0.15.3"`，按名称 `try_from_device`，设备拔掉或打开失败静默回落系统默认）、三个试听行为开关（选中即试听／重选从头播／拖到 DAW 时停止）、「缓存波形图」面板（开关、自定义路径、分组统计、清空、预生成进度与取消）、「临时文件」清理、「数据备份」（每次独立目录永不覆盖，`VACUUM INTO` 一致性快照 + SHA256 + `integrity_check` + manifest，附 localStorage 设置快照）。所有可再生缓存统一受设置页缓存根治理，自定义路径下次启动生效；清理与备份只碰白名单目录。
- **波形缓存事实（回答本批提问）**：波形确实占磁盘，峰值 envelope 存 `waveforms-v1` / `waveforms-mini-v1` JSON，单条 ≤128KB、总上限 16MB／256 条，另有完整波形与高级频谱目录。实测本机 323 个文件／16MB（列表波形 226 个／11MB、简化小波形 59 个／349KB、完整波形 35 个／3.1MB、高级频谱 3 个／639KB），可在设置页查看、改路径、关闭或清空。

验收：`cargo check --all-targets` 通过；`cargo test --bin sfx-tools` 172 通过 / 1 失败 / 6 忽略，唯一失败为既有时间敏感用例 `catalog::real_worker_cancel_resume_and_single_task_guard`（用 0.1.19 HEAD 版 `catalog.rs` 对照重跑同样超时，0.1.18 验收记录里也标注过它，与本批无关）；`tsc --noEmit` 通过、前端波形单测 10/10、debug 构建通过。EXE 0.1.20，25,324,544 字节，2026/11/06 13:01（全长改动后最终重建）。CDP 实测：`app_version`=0.1.20；对库中 3534 秒（约 59 分钟）的素材分别取简化／完整波形，两者 `analyzed_duration` 均为 3534、`truncated=false`（512／4096 峰）；跨语言 `rain` 2→4 条、`雨` 2→4 条、`雨音` 0→4 条，`thunder`／`雷` 0→0（不伪造命中）；输出设备枚举出系统默认与 UR22C 等真实设备；数据备份实产出 2,150,400 字节 catalog 快照、`integrity=ok`。暗／亮两色 12 张截图核对设置四个新增面板与工具页折叠／恢复，无裁切。

边界：全长解码使首次读取长音频比旧的 60 秒截断慢，靠缓存与设置页预生成摊薄；对包含大量长音频的库，批量预生成会明显耗时，可随时取消。文件头时长缺失的异常输入仍退回 2400 万采样安全上限，此时界面标「前段」。6 张参考图的视觉稿仅作对照，未逐像素复刻；「按频谱着色」是否下线、三版图标定版、安装包仍待人类拍板。

## 0.1.21 内嵌元数据检索开关

按人类「搜索栏跨语言按钮旁加『含元数据』开关，可在设置更改默认开启、可手动关」落地。核查确认 0.1.20 既没有这个按钮，也没把 BWF/iXML 写进检索列（关键词只匹配 `a.name || a.path || COALESCE(m.tags_search,'')`），因此做的是后端到前端的完整闭环，不是 UI 假开关。

- **索引**：`catalog.rs` 增 `assets.meta_search TEXT`（`NULL` = 旧库未索引、空串 = 已索引但无内嵌字段）；`metadata_search_text` 只对 WAV 调 `bwf_metadata::read`，取 bext 描述／制作者／参考号／日期时间，以及 iXML 的 PROJECT／SCENE／TAKE／TAPE／NOTE 与 `SW_` 私有字段（项目、场景、条次、备注、分类、关键词、许可证、创作者、来源、UCS 等），去控制字符后拼成一行、截断 4096 字符。`record_file` 在新增／更新／旧库补索引时写入，尽量不重复解码音频。
- **检索**：`SearchFilter` 增 `metadata:bool`，`catalog_list` 增 `metadata:Option<bool>`；开关打开时把 `COALESCE(a.meta_search,'')` 拼进关键词匹配式。跨语言扩展、标签／分类／格式／时长／收藏／播放列表筛选不受影响，用户输入仍全走 SQL 绑定参数。
- **回写**：`bwf_write_metadata` 写盘成功后调 `CatalogState::refresh_metadata_index` 刷新单条索引；刷新失败不回滚音频、不掩盖写入成功。
- **两层状态**：设置页 `preferences.metadataSearch` 是持久化默认值（默认开，旧存档缺字段也默认开）；搜索栏「含元数据」是本次使用临时开关，手动关不写回偏好，设置改默认值时工具栏同步、重启按默认值恢复。设置页「素材库」新增「检索时默认包含内嵌元数据」复选框，搜索栏第二个按钮复用已有 `.search-toggle` 样式。

验收：`cargo check --all-targets` 通过；全量 `cargo test --bin sfx-tools` 176 通过 / 0 失败 / 6 忽略（新增 3 个元数据单测，历史 flaky 用例本轮通过）；`tsc --noEmit` 通过，`tests/preferences.test.mjs` 2/2、波形单测 10/10；`verify.ps1 -Action build` 通过，EXE 0.1.21／25,329,152 字节／2026-11-06 14:28。CDP 实测：`app_version`=v0.1.21；「跨语言」「含元数据」初始都按下，手动关后 placeholder 回退而 localStorage 默认值仍是 `true`；设置里关掉默认值后 reload 保持关闭；`catalog_list` 的 `metadata:true`／`false` 都被接受。暗／亮共 7 张 `v21-*.png`。版本四处同步 0.1.21。

边界：**0.1.20 及更早建立的素材库需重扫一次**才能补齐 `meta_search`；BWF/iXML 回写只自动刷新当前这一条。只覆盖 WAV/BWF 的 bext／iXML 已知字段，MP3／FLAC／AIFF 没有这些内嵌字段（用户标签仍可搜），`axml`／`chna`／`cart`／`ds64` 未解析；索引是拼接单行文本，不分词、不做字段权重、不接语义向量。工程仓本批未提交，知识库仓按约定不自动提交。
