"""CLAP 音频-文本模型（Xenova/clap-htsat-unfused，固定 revision）本地桥。

stdin 一次 JSON / stdout 一次 JSON，返回 {"ok":true,...} 或 {"ok":false,"error":"..."}。
支持：文件契约核验、ONNX 张量契约自检、文本塔嵌入工程接口、本地音频内容编码
（FFmpeg 解码 + Slaney log-mel 预处理 + 音频塔 ONNX 推理）。
只读取本地音频文件，不打开声卡、不联网、不写用户素材库、不修改音频源文件；
不做语义检索或中文相关性评测，不宣称检索质量。

许可边界：上游 laion/clap-htsat-unfused 为 Apache-2.0；Xenova ONNX 转换产物的
再分发条款尚未单独核验，本桥仅按固定 revision 使用本地文件，不对许可作保证。

注入点（模块级变量，可用函数参数覆盖）：SESSION_FACTORY / TOKENIZER_FACTORY /
ENCODER_FACTORY / DECODER_FACTORY / AUDIO_ENCODER_FACTORY。测试注入假
decoder/session/encoder 即可覆盖全部逻辑，无需真实 ONNX 或 FFmpeg。未知 op 一律报错。
"""
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import time

MODEL_ID = "Xenova/clap-htsat-unfused"
REVISION = "c28f2883575e590e04d3146ff0713c2448d691ba"
CAPABILITY = "audio_text"
MAX_REQUEST = 2_000_000
MANIFEST_LIMIT = 128 * 1024
MAX_SYNTH_ELEMENTS = 2_000_000
AUDIO_ONNX = "onnx/audio_model_quantized.onnx"
TEXT_ONNX = "onnx/text_model_quantized.onnx"
TOKENIZER = "tokenizer.json"
REQUIRED_FILES = (AUDIO_ONNX, TEXT_ONNX, TOKENIZER)
OPS = ("status", "selftest", "encode_text", "encode_audio", "encode_audio_batch")
PATH_KEYS = ("model_dir", "index", "db", "progress_path", "cancel_path", "output_path", "audio_path")
# 官方 LFS 期望值；selftest(verify_hashes=True) 对这两个文件逐块校验，status 只报存在与长度。
EXPECTED_SIZES = {AUDIO_ONNX: 34301667, TEXT_ONNX: 126603263}
EXPECTED_SHA256 = {
    AUDIO_ONNX: "3fcff2c8824e7bcb83a983f2a49edab3b60cbcf4872ac70efee517355173bd1f",
    TEXT_ONNX: "1a3df8b197e249816e08415fd040434c44762b2eea7eb7bf8a48a0f0bf3c14e5",
}
# preprocessor_config.json：nb_max_frames=1000（+1 帧）、feature_size=64、单通道。
# 实测该固定 revision 的 ONNX 音频塔只接受 [batch, 1, 1001, 64]；把动态维度全填 1
# 会让 /audio_model/audio_encoder/batch_norm/BatchNormalization 的 scale 校验失败。
AUDIO_FEED_SHAPE = (1, 1, 1001, 64)

STATUS_NOTE = (
    "文件齐备仅代表 CLAP 音频-文本模型的文件契约可核验，不等于语义检索可用；"
    "已支持本地音频内容编码（确定性 rand_trunc 近似），但未做中文相关性评测、"
    "未宣称语义检索质量、未与 HuggingFace 官方 ClapFeatureExtractor 逐帧对齐验证；"
    "上游 laion/clap-htsat-unfused 为 Apache-2.0，ONNX 转换产物再分发条款尚未单独核验。"
)
SELFTEST_NOTE = "这是 ONNX 张量契约自检，未做音频内容语义推理或中文相关性评测。"
ENCODE_NOTE = (
    "encode_text 仅返回归一化文本塔嵌入的工程接口，为后续中文检索准备，"
    "不宣称检索质量或相关性。"
)
ENCODE_AUDIO_NOTE = (
    "encode_audio 返回归一化音频塔嵌入，仅供工程链路使用，不宣称语义检索质量；"
    "mel 预处理按 HuggingFace 官方 Slaney 公式复刻，但无 transformers/torch 做逐帧对照验证；"
    "rand_trunc 长音频裁剪由官方随机裁剪改为确定性取开头（waveform[:480000]），可复现但与官方不完全一致；"
    "未做中文相关性评测。"
)

# 默认 None 表示使用真实 onnxruntime / tokenizers / FFmpeg；测试可注入以跳过真实加载与解码。
SESSION_FACTORY = None
TOKENIZER_FACTORY = None
ENCODER_FACTORY = None
DECODER_FACTORY = None
AUDIO_ENCODER_FACTORY = None


def _check_abs_path(value, key):
    if not isinstance(value, str) or not value or len(value) > 32768:
        raise ValueError(key + " 必须是长度有界的路径字符串")
    path = Path(value)
    if not path.is_absolute():
        raise ValueError(key + " 必须是绝对路径")
    if any(part == ".." for part in path.parts):
        raise ValueError(key + " 不允许 .. 组件（禁止路径越界）")


def validate(request):
    if not isinstance(request, dict) or request.get("op") not in OPS:
        raise ValueError("op 必须是 status/selftest/encode_text/encode_audio/encode_audio_batch")
    for key in PATH_KEYS:
        if key in request:
            _check_abs_path(request[key], key)
    if "model_dir" not in request:
        raise ValueError("model_dir 缺失")
    revision = request.get("revision")
    if (not isinstance(revision, str) or len(revision) != 40
            or any(c not in "0123456789abcdef" for c in revision)
            or revision != REVISION):
        raise ValueError("revision 必须是固定的 40 位小写提交标识 " + REVISION)
    if request["op"] == "encode_text":
        text = request.get("text")
        if not isinstance(text, str) or not text.strip() or len(text) > 1024:
            raise ValueError("text 必须为 1..1024 个非空字符")
    if request["op"] == "encode_audio":
        audio_path = request.get("audio_path")
        if not isinstance(audio_path, str) or not audio_path or len(audio_path) > 32768:
            raise ValueError("audio_path 必须为 1..32768 个非空字符")
    if request["op"] == "encode_audio_batch":
        audio_paths = request.get("audio_paths")
        if not isinstance(audio_paths, list) or not 1 <= len(audio_paths) <= 32:
            raise ValueError("audio_paths 必须为 1..32 个路径的列表")
        for item in audio_paths:
            _check_abs_path(item, "audio_paths 元素")
    return request


def decode(raw):
    if len(raw) > MAX_REQUEST:
        raise ValueError("request too large（超过 2MB 上限）")
    return validate(json.loads(raw))


def _check_relative(relative):
    if not isinstance(relative, str) or not relative or len(relative) > 512:
        raise ValueError("模型相对路径无效")
    if relative.startswith(("/", "\\")) or ":" in relative or "\\" in relative or ".." in relative:
        raise ValueError("模型相对路径禁止绝对路径、.. 或转义：" + relative)
    if any(part in ("", ".", "..") for part in Path(relative).parts):
        raise ValueError("模型相对路径含越界组件：" + relative)


def _assert_no_link(path, root):
    try:
        relative = path.relative_to(root)
    except ValueError:
        raise ValueError("模型路径越出根目录：" + str(path))
    current = root
    for part in relative.parts:
        current = current / part
        try:
            info = os.lstat(current)
        except OSError:
            continue
        if stat.S_ISLNK(info.st_mode) or (getattr(info, "st_file_attributes", 0) & 0x400):
            raise ValueError("模型路径含符号链接或目录联接，拒绝逃逸：" + str(current))


def _model_file(request, relative):
    root = Path(request["model_dir"]).resolve()
    _check_relative(relative)
    raw = root.joinpath(*Path(relative).parts)
    _assert_no_link(raw, root)
    path = raw.resolve()
    if path != root and root not in path.parents:
        raise ValueError("模型文件经解析后越出根目录：" + relative)
    if not path.is_file():
        raise ValueError("模型文件缺失或不是普通文件：" + relative)
    return path


def _check_hash(path, expected):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    if digest.hexdigest() != expected.lower():
        raise ValueError("模型文件 SHA256 与官方 LFS 期望值不符（下载不完整或被替换）：" + path.name)


def manifest(request, verify=False):
    """只读核验清单与三个文件。verify=True 时对两个官方 ONNX 逐块校验 SHA256。"""
    directory = Path(request["model_dir"]).resolve()
    raw = directory / "model-manifest.json"
    _assert_no_link(raw, directory)
    if not raw.is_file():
        raise ValueError("缺少 model-manifest.json")
    if raw.stat().st_size > MANIFEST_LIMIT:
        raise ValueError("model-manifest.json 超出允许大小")
    data = json.loads(raw.read_text(encoding="utf-8-sig"))
    if not isinstance(data, dict):
        raise ValueError("清单必须是 JSON 对象")
    if data.get("model_id") != MODEL_ID:
        raise ValueError("清单 model_id 与 " + MODEL_ID + " 不一致")
    if data.get("revision") != REVISION or request.get("revision") != REVISION:
        raise ValueError("清单/请求 revision 与固定值不一致")
    license_ = data.get("license")
    if not isinstance(license_, str) or not license_.strip():
        raise ValueError("清单缺少 license 声明")
    files = data.get("files")
    if not isinstance(files, list):
        raise ValueError("清单缺少 files 列表")
    entries = {}
    for item in files:
        if not isinstance(item, dict) or not isinstance(item.get("path"), str):
            raise ValueError("清单 files 项无效")
        _check_relative(item["path"])
        entries[item["path"]] = item
    for relative in REQUIRED_FILES:
        entry = entries.get(relative)
        if entry is None:
            raise ValueError("清单缺少文件记录：" + relative)
        path = _model_file(request, relative)
        size = entry.get("size")
        if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
            raise ValueError("清单文件大小无效：" + relative)
        if path.stat().st_size != size:
            raise ValueError("模型文件长度与清单不匹配：" + relative)
        if verify and relative in EXPECTED_SHA256:
            _check_hash(path, EXPECTED_SHA256[relative])
    return data


def status(request):
    """只读：status 不加载模型、不逐块校验；只报 files_present 与 manifest_ok。"""
    files_present = False
    manifest_ok = False
    detail = ""
    try:
        manifest(request)
        manifest_ok = True
        files_present = True
    except (OSError, ValueError, KeyError, TypeError) as error:
        detail = "（当前文件契约未通过：" + str(error) + "）"
    available = files_present and manifest_ok
    return dict(ok=True, available=available, model_ready=available,
                files_present=files_present, manifest_ok=manifest_ok,
                capability=CAPABILITY, revision=REVISION, dim=None,
                note=STATUS_NOTE + detail)


def _shape_of(shape):
    dims = []
    for value in (shape or []):
        if isinstance(value, int) and not isinstance(value, bool) and value > 0:
            dims.append(value)
        else:
            dims.append(1)  # 动态维度（None / 字符串 / 0 / 负数）用 1
    total = 1
    for value in dims:
        total *= value
    if total > MAX_SYNTH_ELEMENTS:
        dims = [1 for _ in dims]  # 超大声明形状退化为全 1，防止构造超大张量
    return tuple(dims) or (1,)


def _dtype(np, onnx_type):
    name = (onnx_type or "").lower()
    if "float16" in name:
        return np.float16
    if "double" in name:
        return np.float64
    if "float" in name:
        return np.float32
    if "int64" in name:
        return np.int64
    if "int32" in name:
        return np.int32
    if "int16" in name:
        return np.int16
    if "int8" in name:
        return np.int8
    if "uint8" in name:
        return np.uint8
    if "bool" in name:
        return np.bool_
    return np.float32


def _synth(np, onnx_type, shape):
    dtype = _dtype(np, onnx_type)
    if dtype == np.bool_:
        return np.ones(shape, dtype=np.bool_)
    if np.issubdtype(dtype, np.floating):
        return np.full(shape, 0.01, dtype=dtype)  # 小的确定性浮点值
    return np.ones(shape, dtype=dtype)  # 整型用合法 id（1）


def _build_feed(np, session):
    feed = {}
    for spec in session.get_inputs():
        declared = _shape_of(getattr(spec, "shape", None))
        shape = AUDIO_FEED_SHAPE if spec.name == "input_features" and len(declared) == 4 else declared
        feed[spec.name] = _synth(np, getattr(spec, "type", None), shape)
    return feed


def _output_names(session):
    return [item.name for item in session.get_outputs()]


def _real_session_factory(path, providers):
    import onnxruntime as ort
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    return ort.InferenceSession(str(path), sess_options=options, providers=providers)


def _real_tokenizer_factory(path):
    from tokenizers import Tokenizer
    tokenizer = Tokenizer.from_file(str(path))
    tokenizer.enable_truncation(max_length=256)
    return tokenizer


def selftest(request, session_factory=None, verify_hashes=True):
    """两个 ONNX session 各跑一次确定性合成张量推理，仅验证张量契约与有限性。"""
    manifest(request, verify=verify_hashes)  # 全量逐块校验两个官方 SHA256（真实运行默认开启）
    import numpy as np
    factory = session_factory or SESSION_FACTORY or _real_session_factory
    started = time.perf_counter()
    audio = factory(_model_file(request, AUDIO_ONNX), ["CPUExecutionProvider"])
    text = factory(_model_file(request, TEXT_ONNX), ["CPUExecutionProvider"])
    audio_feed = _build_feed(np, audio)
    text_feed = _build_feed(np, text)
    audio_out = list(audio.run(_output_names(audio), audio_feed))
    text_out = list(text.run(_output_names(text), text_feed))
    if not audio_out or not text_out:
        raise ValueError("ONNX 会话没有输出张量，无法完成张量契约自检")
    if not all(bool(np.isfinite(np.asarray(item)).all()) for item in audio_out + text_out):
        raise ValueError("ONNX 张量契约自检失败：输出包含非有限值（NaN/Inf），拒绝返回假结果")

    def dim_of(array):
        return int(array.shape[-1]) if array.ndim >= 1 else int(array.size)

    audio_primary = np.asarray(audio_out[0])
    text_primary = np.asarray(text_out[0])
    return dict(ok=True,
                audio_output_shape=list(audio_primary.shape),
                text_output_shape=list(text_primary.shape),
                audio_dim=dim_of(audio_primary), text_dim=dim_of(text_primary),
                audio_input_shapes={key: list(np.asarray(value).shape) for key, value in audio_feed.items()},
                text_input_shapes={key: list(np.asarray(value).shape) for key, value in text_feed.items()},
                finite=True,
                elapsed_ms=round((time.perf_counter() - started) * 1000, 2),
                capability=CAPABILITY, revision=REVISION, note=SELFTEST_NOTE)


def _flatten(values):
    if hasattr(values, "tolist"):
        values = values.tolist()
    result = []
    stack = [values]
    while stack:
        current = stack.pop()
        if isinstance(current, (list, tuple)):
            stack.extend(reversed(current))
        else:
            result.append(current)
    return result


def _real_encode_text(request, session_factory, tokenizer_factory):
    import numpy as np
    session_factory = session_factory or SESSION_FACTORY or _real_session_factory
    tokenizer_factory = tokenizer_factory or TOKENIZER_FACTORY or _real_tokenizer_factory
    tokenizer = tokenizer_factory(_model_file(request, TOKENIZER))
    session = session_factory(_model_file(request, TEXT_ONNX), ["CPUExecutionProvider"])
    encoded = tokenizer.encode(request["text"])
    ids = list(encoded.ids)
    mask = list(getattr(encoded, "attention_mask", [1] * len(ids)))
    feed = {}
    for spec in session.get_inputs():
        name = spec.name
        if name == "input_ids":
            feed[name] = np.asarray([ids], dtype=np.int64)
        elif name == "attention_mask":
            feed[name] = np.asarray([mask], dtype=np.int64)
        elif name == "token_type_ids":
            feed[name] = np.zeros((1, len(ids)), dtype=np.int64)
        else:
            feed[name] = _synth(np, getattr(spec, "type", None), _shape_of(getattr(spec, "shape", None)))
    outputs = session.run(_output_names(session), feed)
    if not outputs:
        raise ValueError("文本塔没有输出张量")
    return np.asarray(outputs[0]).reshape(-1).tolist()


def encode_text(request, encoder_factory=None, session_factory=None, tokenizer_factory=None):
    """文本塔嵌入工程接口：输出做 L2 归一化，返回有限向量；不宣称检索质量。"""
    manifest(request)  # 契约齐备（不逐块），未就位返回明确错误而非假数据
    factory = encoder_factory or ENCODER_FACTORY
    vector = (factory(request).encode_text(request["text"]) if factory is not None
              else _real_encode_text(request, session_factory, tokenizer_factory))
    flat = _flatten(vector)
    if not flat or len(flat) > 4096:
        raise ValueError("文本嵌入维度越界")
    try:
        values = [float(item) for item in flat]
    except (TypeError, ValueError):
        raise ValueError("文本嵌入包含非数值元素，拒绝返回")
    if not all(math.isfinite(value) for value in values):
        raise ValueError("文本嵌入包含非有限值（NaN/Inf），拒绝返回假向量")
    norm = math.sqrt(sum(value * value for value in values))
    if norm <= 0 or not math.isfinite(norm):
        raise ValueError("文本嵌入范数无效（零向量或非有限值）")
    return dict(ok=True, dim=len(values), vector=[value / norm for value in values],
                capability=CAPABILITY, revision=REVISION, note=ENCODE_NOTE)


def _ffmpeg_path():
    """解析 FFmpeg 可执行文件：SW_FFMPEG_PATH 优先，其次 PATH。"""
    override = os.environ.get("SW_FFMPEG_PATH")
    if override:
        return override
    found = shutil.which("ffmpeg")
    if not found:
        raise ValueError("未找到 FFmpeg：请设置 SW_FFMPEG_PATH 或将 ffmpeg 加入 PATH")
    return found


def _real_decode_audio(path):
    """FFmpeg 把任意音频解码为 48kHz 单声道 f32le（最多 30 秒），返回一维 float32。"""
    import numpy as np
    command = [
        _ffmpeg_path(),
        "-v", "error",
        "-nostdin",
        "-i", str(path),
        "-ac", "1",
        "-ar", "48000",
        "-f", "f32le",
        "-acodec", "pcm_f32le",
        "-t", "30",
        "-",
    ]
    try:
        result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                timeout=60, check=False)
    except subprocess.TimeoutExpired:
        raise ValueError("FFmpeg 解码超时（超过 60 秒）")
    except OSError as error:
        raise ValueError("无法启动 FFmpeg：" + str(error))
    if result.returncode != 0:
        detail = result.stderr.decode("utf-8", "replace").strip()[:512]
        raise ValueError("FFmpeg 解码失败（returncode=" + str(result.returncode) + "）：" + detail)
    waveform = np.frombuffer(result.stdout, dtype="<f4").astype(np.float32)
    if waveform.size == 0:
        raise ValueError("FFmpeg 未解码出音频样本：" + str(path))
    if not bool(np.isfinite(waveform).any()):
        raise ValueError("FFmpeg 解码结果全为非有限值（NaN/Inf）：" + str(path))
    return waveform


def _hertz_to_mel_slaney(freq):
    """Slaney hertz->mel；np.where 按 mask 分段，兼容标量与 ndarray。"""
    import numpy as np
    freq = np.asarray(freq, dtype=np.float64)
    logstep = 27.0 / np.log(6.4)
    return np.where(freq < 1000.0, 3.0 * freq / 200.0,
                    15.0 + np.log(np.maximum(freq, 1000.0) / 1000.0) * logstep)


def _mel_to_hertz_slaney(mels):
    """Slaney mel->hertz；np.where 按 mask 分段赋值，避免 ndarray 用 Python 分支。"""
    import numpy as np
    mels = np.asarray(mels, dtype=np.float64)
    logstep = np.log(6.4) / 27.0
    return np.where(mels < 15.0, 200.0 * mels / 3.0,
                    1000.0 * np.exp(logstep * (mels - 15.0)))


def _mel_features(waveform):
    """复刻 ClapFeatureExtractor（rand_trunc / slaney / repeatpad）的 log-mel，返回 (1001,64) float32。

    与官方差异：官方 rand_trunc 对超长波形随机裁剪，本项目为确定性可复现改为取开头 waveform[:N]；
    mel 公式逐式复刻，但无 transformers/torch 做逐帧对照验证。
    """
    import numpy as np
    n_samples = 480000
    n_fft = 1024
    hop_length = 480
    w = np.asarray(waveform, dtype=np.float64).reshape(-1)
    if w.size == 0:
        raise ValueError("音频波形为空，无法提取 mel 特征")
    if w.size > n_samples:
        # 官方 rand_trunc: idx = np.random.randint(0, overflow + 1); w = w[idx:idx + n_samples]
        # 本项目要求确定性可复现，改为取开头（与官方随机裁剪不完全一致）。
        w = w[:n_samples]
    elif w.size < n_samples:
        tiled = np.tile(w, int(n_samples / w.size))  # repeatpad
        w = np.pad(tiled, (0, n_samples - tiled.size), mode="constant", constant_values=0.0)
    window = np.hanning(n_fft + 1)[:-1].astype(np.float64)
    padded = np.pad(w, (n_fft // 2, n_fft // 2), mode="reflect")
    n_frames = int(1 + np.floor((padded.size - n_fft) / hop_length))
    frames = np.lib.stride_tricks.sliding_window_view(padded, n_fft)[::hop_length][:n_frames]
    spec = np.fft.rfft(frames * window, n=n_fft, axis=1)
    power = (np.abs(spec) ** 2.0).T  # (513, n_frames)

    fft_freqs = np.linspace(0, 48000 // 2, 513)
    mel_freqs = np.linspace(_hertz_to_mel_slaney(50.0).item(),
                            _hertz_to_mel_slaney(14000.0).item(), 66)
    filter_freqs = _mel_to_hertz_slaney(mel_freqs)
    filter_diff = np.diff(filter_freqs)
    slopes = filter_freqs[None, :] - fft_freqs[:, None]
    down = -slopes[:, :-2] / filter_diff[:-1]
    up = slopes[:, 2:] / filter_diff[1:]
    fb = np.maximum(0.0, np.minimum(down, up))  # (513, 64)
    enorm = 2.0 / (filter_freqs[2:66] - filter_freqs[0:64])
    fb *= enorm[None, :]

    mel = np.maximum(1e-10, fb.T @ power)  # (64, n_frames)
    db = 10.0 * (np.log10(np.clip(mel, 1e-10, None))
                 - np.log10(max(1e-10, 1.0)))  # power=2.0 / reference=1.0 / top_db=null
    features = np.asarray(db, dtype=np.float32).T  # (1001, 64)
    if features.shape != AUDIO_FEED_SHAPE[2:] or not bool(np.isfinite(features).all()):
        raise ValueError("mel 特征形状或非有限值校验失败：" + str(features.shape))
    return features


def _real_encode_audio(request, decoder_factory, session_factory):
    import numpy as np
    decoder_factory = decoder_factory or DECODER_FACTORY or _real_decode_audio
    session_factory = session_factory or SESSION_FACTORY or _real_session_factory
    waveform = decoder_factory(request["audio_path"])
    features = _mel_features(waveform)
    session = session_factory(_model_file(request, AUDIO_ONNX), ["CPUExecutionProvider"])
    outputs = session.run(_output_names(session),
                          {"input_features": features[None, None, :, :]})
    if not outputs:
        raise ValueError("音频塔没有输出张量")
    return np.asarray(outputs[0]).reshape(-1).tolist()


def encode_audio(request, decoder_factory=None, session_factory=None, encoder_factory=None):
    """音频塔嵌入工程接口：L2 归一化后返回有限向量；确定性 rand_trunc 近似，不宣称检索质量。"""
    manifest(request)  # 契约齐备（不逐块），未就位返回明确错误而非假数据
    factory = encoder_factory or AUDIO_ENCODER_FACTORY
    vector = (factory(request).encode_audio(request["audio_path"]) if factory is not None
              else _real_encode_audio(request, decoder_factory, session_factory))
    flat = _flatten(vector)
    if len(flat) != 512:
        raise ValueError("音频嵌入维度必须为 512，实际为 " + str(len(flat)))
    try:
        values = [float(item) for item in flat]
    except (TypeError, ValueError):
        raise ValueError("音频嵌入包含非数值元素，拒绝返回")
    if not all(math.isfinite(value) for value in values):
        raise ValueError("音频嵌入包含非有限值（NaN/Inf），拒绝返回假向量")
    norm = math.sqrt(sum(value * value for value in values))
    if norm <= 0 or not math.isfinite(norm):
        raise ValueError("音频嵌入范数无效（零向量或非有限值）")
    return dict(ok=True, dim=512, vector=[value / norm for value in values],
                capability=CAPABILITY, revision=REVISION, note=ENCODE_AUDIO_NOTE)


def encode_audio_batch(request, decoder_factory=None, session_factory=None):
    """批量音频编码：一次加载 audio ONNX session，逐文件编码；单文件失败隔离，不影响整批。"""
    manifest(request)  # 契约齐备（不逐块），整批只校验一次
    decoder_factory = decoder_factory or DECODER_FACTORY or _real_decode_audio
    session_factory = session_factory or SESSION_FACTORY or _real_session_factory
    session = session_factory(_model_file(request, AUDIO_ONNX), ["CPUExecutionProvider"])
    output_names = _output_names(session)
    vectors = []
    failures = []
    count = 0
    for path in request["audio_paths"]:
        try:
            features = _mel_features(decoder_factory(path))
            outputs = session.run(output_names,
                                  {"input_features": features[None, None, :, :]})
            if not outputs:
                raise ValueError("音频塔没有输出张量")
            flat = _flatten(outputs[0])
            if len(flat) != 512:
                raise ValueError("音频嵌入维度必须为 512，实际为 " + str(len(flat)))
            try:
                values = [float(item) for item in flat]
            except (TypeError, ValueError):
                raise ValueError("音频嵌入包含非数值元素，拒绝返回")
            if not all(math.isfinite(value) for value in values):
                raise ValueError("音频嵌入包含非有限值（NaN/Inf），拒绝返回假向量")
            norm = math.sqrt(sum(value * value for value in values))
            if norm <= 0 or not math.isfinite(norm):
                raise ValueError("音频嵌入范数无效（零向量或非有限值）")
            vectors.append([value / norm for value in values])
            count += 1
        except Exception as error:
            vectors.append(None)
            failures.append(dict(path=path, error=str(error)[:500]))
    # 32 x 512 个原生 float 约 350KB，加 failures 仍远小于 1MB；保留 float 原生序列化以免影响余弦排序。
    return dict(ok=True, count=count, dim=512, vectors=vectors, failures=failures,
                capability=CAPABILITY, revision=REVISION, note=ENCODE_AUDIO_NOTE)


def main():
    try:
        raw = sys.stdin.buffer.read(MAX_REQUEST + 1)
        request = decode(raw)
        op = request["op"]
        if op == "status":
            result = status(request)
        elif op == "selftest":
            result = selftest(request)
        elif op == "encode_audio":
            result = encode_audio(request)
        elif op == "encode_audio_batch":
            result = encode_audio_batch(request)
        else:
            result = encode_text(request)
    except Exception as error:
        print(str(error), file=sys.stderr)
        result = dict(ok=False, error=str(error))
    print(json.dumps(result, ensure_ascii=True, allow_nan=False))


if __name__ == "__main__":
    main()
