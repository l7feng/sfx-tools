# 第三方组件与许可声明

本文件汇总 Sound Workbench 运行时依赖的第三方组件及其许可。**当前为初版工作稿**：打包（`bundle.active` 打开）前必须补齐每个组件标记为「待补」的许可全文，否则不具备分发条件。

审计依据与推导过程见知识库项目文档 `25-模型与FFmpeg再分发许可审计.md`（2026-11-05）。

## AI 模型

| 组件 | 固定版本 | 许可 | 全文状态 |
| --- | --- | --- | --- |
| intfloat/multilingual-e5-small（文本向量，文本语义检索） | revision `614241f622f53c4eeff9890bdc4f31cfecc418b3` | MIT | 待补 MIT 全文与版权行 |
| laion/clap-htsat-unfused（音频-文本双塔上游权重） | revision `8fa0f1c6d0433df6e97c127f64b2a1d6c0dcda8a` | Apache-2.0 | 待补 Apache-2.0 全文 + NOTICE |
| Xenova/clap-htsat-unfused（本软件实际使用的 ONNX 产物） | revision `c28f2883575e590e04d3146ff0713c2448d691ba` | 仓库未声明，沿用上游 Apache-2.0 | 待补上述全文 + 修改说明（PyTorch→ONNX 转换、int8 量化、转换方为 Xenova 社区） |

模型由用户端下载到本地，本仓库不携带权重文件（`build/` 已被忽略）。

## Python 运行时（打包时若随包内嵌）

| 组件 | 版本 | 许可 | 全文位置/状态 |
| --- | --- | --- | --- |
| Python 解释器 | 3.12.10 | PSF-2.0 | 待补 |
| ONNX Runtime | 1.23.2 | MIT | 需随附 `onnxruntime/ThirdPartyNotices.txt`（已存在于 venv） |
| HuggingFace tokenizers | 0.22.2 | Apache-2.0 | 待补 |
| NumPy | 2.3.4 | BSD-3-Clause | 待补 |

## 外部可执行文件

| 组件 | 处理方式 | 说明 |
| --- | --- | --- |
| FFmpeg | **不捆绑**，运行时按 `SOUND_WORKBENCH_FFMPEG` / `SW_FFMPEG_PATH` / PATH 解析 | 用户自备，本软件不承担其再分发义务。若将来捆绑，只允许 LGPL 共享构建，禁止 `--enable-gpl` 构建；须附 LGPL 全文与源码获取方式 |

## 应用框架

| 组件 | 许可 | 说明 |
| --- | --- | --- |
| Tauri 2 / Wry / WebView2 绑定 | MIT 或 Apache-2.0 | 待补 |
| React 及前端依赖 | MIT | 打包前用工具统一扫描 `package-lock.json` 后补齐 |

## 打包前检查清单

1. 每个「待补」项写入实际许可全文，不以链接代替。
2. 模型落盘目录同时写入 `LICENSE` / `NOTICE` / `MODIFICATIONS.md`，其 SHA256 纳入 `model-manifest.json`。
3. 应用内「关于 → 开源许可」可查看本文件内容。
4. 分发 FFmpeg 的构建若含 `libsoxr`（LGPL），单独列明。
