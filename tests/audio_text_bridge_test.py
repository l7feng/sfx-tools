"""Contract/selftest/encode_audio tests with injected fakes. Never loads real ONNX, network or FFmpeg."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import numpy as np

spec = importlib.util.spec_from_file_location(
    "audio_text_bridge", Path(__file__).resolve().parents[1] / "audio_text_bridge.py")
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)


class Input:
    def __init__(self, name, shape, dtype):
        self.name, self.shape, self.type = name, shape, dtype


class Output:
    def __init__(self, name):
        self.name = name


class FakeSession:
    def __init__(self, inputs, outputs, values):
        self._inputs, self._outputs, self._values = inputs, outputs, values

    def get_inputs(self):
        return self._inputs

    def get_outputs(self):
        return self._outputs

    def run(self, names, feed):
        return self._values


class AudioSession:
    """记录喂给音频塔的 feed，便于断言 input_features 契约。"""

    def __init__(self, values):
        self._values = values
        self.last_names = None
        self.last_feed = None

    def get_inputs(self):
        return [Input("input_features", [None, 1, None, 64], "tensor(float)")]

    def get_outputs(self):
        return [Output("audio_embeds")]

    def run(self, names, feed):
        self.last_names = names
        self.last_feed = feed
        return self._values


def session_factory(audio, text):
    def make(path, providers):
        return audio if Path(path).name == "audio_model_quantized.onnx" else text
    return make


def audio_factory(session):
    def make(path, providers):
        return session
    return make


def fake_decoder(waveform):
    def decode(path):
        return waveform
    return decode


def finite_sessions():
    audio = FakeSession(
        [Input("input_features", [None, 1, None, 64], "tensor(float)")],
        [Output("audio_embeds")],
        [np.full((1, 512), 0.5, dtype=np.float32)])
    text = FakeSession(
        [Input("input_ids", [None, None], "tensor(int64)"),
         Input("attention_mask", [None, None], "tensor(int64)")],
        [Output("text_embeds")],
        [np.ones((1, 512), dtype=np.float32)])
    return audio, text


class SequenceSession:
    """按调用序号返回不同输出，用于批量异常隔离测试。"""

    def __init__(self, values_list):
        self._values_list = values_list
        self._index = 0

    def get_inputs(self):
        return [Input("input_features", [None, 1, None, 64], "tensor(float)")]

    def get_outputs(self):
        return [Output("audio_embeds")]

    def run(self, names, feed):
        values = self._values_list[min(self._index, len(self._values_list) - 1)]
        self._index += 1
        return values


def recording_decoder(waveform, calls):
    def decode(path):
        calls.append(path)
        return waveform
    return decode


def recording_audio_factory(session, calls):
    def make(path, providers):
        calls.append(path)
        return session
    return make


class BridgeTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.model = self.root / "model"
        (self.model / "onnx").mkdir(parents=True)
        self.audio_bytes = b"audio-placeholder"
        self.text_bytes = b"text-placeholder"
        self.token_bytes = b"tokenizer-placeholder"
        (self.model / "onnx" / "audio_model_quantized.onnx").write_bytes(self.audio_bytes)
        (self.model / "onnx" / "text_model_quantized.onnx").write_bytes(self.text_bytes)
        (self.model / "tokenizer.json").write_bytes(self.token_bytes)
        self.write_manifest()
        self.request = dict(op="status", model_dir=str(self.model),
                            revision=bridge.REVISION, text="夜晚安静的雨声")

    def tearDown(self):
        self.temp.cleanup()

    def write_manifest(self, revision=None, bad_path=None, bad_size=False):
        files = [
            dict(path=bridge.AUDIO_ONNX, size=len(self.audio_bytes),
                 sha256=bridge.EXPECTED_SHA256[bridge.AUDIO_ONNX]),
            dict(path=bridge.TEXT_ONNX, size=len(self.text_bytes),
                 sha256=bridge.EXPECTED_SHA256[bridge.TEXT_ONNX]),
            dict(path=bridge.TOKENIZER, size=len(self.token_bytes), sha256="0" * 64),
        ]
        if bad_size:
            files[0]["size"] = len(self.audio_bytes) + 7
        if bad_path:
            files.append(dict(path=bad_path, size=1, sha256="0" * 64))
        data = dict(model_id=bridge.MODEL_ID, revision=revision or bridge.REVISION,
                    license="Apache-2.0", files=files)
        (self.model / "model-manifest.json").write_text(
            json.dumps(data, ensure_ascii=False), encoding="utf-8")

    def official_hashes(self):
        return {
            bridge.AUDIO_ONNX: hashlib.sha256(self.audio_bytes).hexdigest(),
            bridge.TEXT_ONNX: hashlib.sha256(self.text_bytes).hexdigest(),
        }

    def test_status_files_present_and_missing(self):
        result = bridge.status(self.request)
        self.assertTrue(result["ok"])
        self.assertTrue(result["available"])
        self.assertTrue(result["model_ready"])
        self.assertTrue(result["files_present"])
        self.assertTrue(result["manifest_ok"])
        self.assertEqual(result["capability"], "audio_text")
        self.assertEqual(result["revision"], bridge.REVISION)
        self.assertIsNone(result["dim"])
        self.assertIn("不等于语义检索可用", result["note"])
        self.assertIn("已支持本地音频内容编码", result["note"])
        self.assertIn("未与 HuggingFace 官方 ClapFeatureExtractor 逐帧对齐验证", result["note"])
        (self.model / "onnx" / "text_model_quantized.onnx").unlink()
        missing = bridge.status(self.request)
        self.assertFalse(missing["available"])
        self.assertFalse(missing["files_present"])
        self.assertFalse(missing["manifest_ok"])

    def test_selftest_success_finite_dims_and_note(self):
        audio, text = finite_sessions()
        with mock.patch.object(bridge, "EXPECTED_SHA256", self.official_hashes()):
            result = bridge.selftest(dict(self.request, op="selftest"),
                                     session_factory=session_factory(audio, text),
                                     verify_hashes=True)
        self.assertTrue(result["ok"])
        self.assertTrue(result["finite"])
        self.assertEqual(result["audio_output_shape"], [1, 512])
        self.assertEqual(result["text_output_shape"], [1, 512])
        self.assertEqual(result["audio_dim"], 512)
        self.assertEqual(result["text_dim"], 512)
        self.assertGreaterEqual(result["elapsed_ms"], 0)
        self.assertIn("这是 ONNX 张量契约自检，未做音频内容语义推理或中文相关性评测",
                      result["note"])

    def test_selftest_rejects_non_finite_output(self):
        audio = FakeSession([Input("input_features", [None, 1, None, 64], "tensor(float)")],
                            [Output("audio_embeds")],
                            [np.array([[np.nan] * 512], dtype=np.float32)])
        text = FakeSession([Input("input_ids", [None, None], "tensor(int64)")],
                           [Output("text_embeds")],
                           [np.ones((1, 512), dtype=np.float32)])
        with self.assertRaisesRegex(ValueError, "非有限"):
            bridge.selftest(dict(self.request, op="selftest"),
                            session_factory=session_factory(audio, text),
                            verify_hashes=False)

    def test_validation_rejects_invalid_revision_path_escape_and_oversize(self):
        bridge.validate(self.request)
        for revision in ["main", "A" * 40, "a" * 40, "a" * 41]:
            with self.assertRaises(ValueError):
                bridge.validate(dict(self.request, revision=revision))
        for path in ["relative", str(self.model) + "/../escape"]:
            with self.assertRaises(ValueError):
                bridge.validate(dict(self.request, model_dir=path))
        with self.assertRaises(ValueError):
            bridge.validate(dict(self.request, index="relative"))
        with self.assertRaisesRegex(ValueError, "too large"):
            bridge.decode(b"x" * (bridge.MAX_REQUEST + 1))
        self.write_manifest(bad_path="../evil")
        with self.assertRaises(ValueError):
            bridge.manifest(self.request)
        self.assertFalse(bridge.status(self.request)["manifest_ok"])

    def test_encode_text_normalizes_and_bounded(self):
        class Fixed:
            def __init__(self, request):
                pass

            def encode_text(self, text):
                return [3.0, 4.0]

        result = bridge.encode_text(dict(self.request, op="encode_text"), encoder_factory=Fixed)
        self.assertTrue(result["ok"])
        self.assertEqual(result["dim"], 2)
        self.assertAlmostEqual(result["vector"][0], 0.6, places=9)
        self.assertAlmostEqual(sum(v * v for v in result["vector"]), 1.0, places=9)
        self.assertIn("不宣称检索质量", result["note"])

        class NonFinite(Fixed):
            def encode_text(self, text):
                return [1.0, float("nan")]

        class Zero(Fixed):
            def encode_text(self, text):
                return [0.0, 0.0]

        with self.assertRaisesRegex(ValueError, "非有限"):
            bridge.encode_text(dict(self.request, op="encode_text"), encoder_factory=NonFinite)
        with self.assertRaisesRegex(ValueError, "范数"):
            bridge.encode_text(dict(self.request, op="encode_text"), encoder_factory=Zero)

    def test_unknown_and_unimplemented_op_rejected(self):
        # encode_audio 已在 OPS 中实现并注册；未知 op 仍一律报错。
        self.assertIn("encode_audio", bridge.OPS)
        self.assertIn("audio_path", bridge.PATH_KEYS)
        with self.assertRaises(ValueError):
            bridge.validate(dict(self.request, op="nonsense"))
        bridge.validate(dict(self.request, op="encode_audio",
                             audio_path=str(self.root / "tone.wav")))

    def test_selftest_model_not_ready_returns_explicit_error(self):
        (self.model / "onnx" / "text_model_quantized.onnx").unlink()
        audio, text = finite_sessions()
        with self.assertRaisesRegex(ValueError, "模型文件"):
            bridge.selftest(dict(self.request, op="selftest"),
                            session_factory=session_factory(audio, text),
                            verify_hashes=False)

    def test_manifest_verify_detects_tampered_file(self):
        with mock.patch.object(bridge, "EXPECTED_SHA256", self.official_hashes()):
            bridge.manifest(self.request, verify=True)
            tampered = bytes([self.audio_bytes[0] ^ 0xFF]) + self.audio_bytes[1:]
            (self.model / "onnx" / "audio_model_quantized.onnx").write_bytes(tampered)
            with self.assertRaisesRegex(ValueError, "SHA256"):
                bridge.manifest(self.request, verify=True)

    # ---- encode_audio 新增测试 ----

    def test_encode_audio_feed_contract(self):
        tone = (0.2 * np.sin(2 * np.pi * 440 * np.arange(48000) / 48000)).astype(np.float32)
        session = AudioSession([np.full((1, 512), 0.5, dtype=np.float32)])
        request = dict(self.request, op="encode_audio", audio_path=str(self.root / "tone.wav"))
        result = bridge.encode_audio(request, decoder_factory=fake_decoder(tone),
                                     session_factory=audio_factory(session))
        self.assertTrue(result["ok"])
        self.assertEqual(result["dim"], 512)
        self.assertEqual(result["capability"], "audio_text")
        self.assertEqual(result["revision"], bridge.REVISION)
        self.assertEqual(result["note"], bridge.ENCODE_AUDIO_NOTE)
        self.assertAlmostEqual(float(np.linalg.norm(result["vector"])), 1.0, places=9)
        feed = session.last_feed
        self.assertIsInstance(feed, dict)
        self.assertIn("input_features", feed)
        array = feed["input_features"]
        self.assertEqual(array.shape, (1, 1, 1001, 64))
        self.assertEqual(array.dtype, np.float32)
        self.assertTrue(bool(np.isfinite(array).all()))

    def test_mel_features_deterministic_and_distinct(self):
        silence = np.zeros(48000, dtype=np.float32)
        tone = (0.2 * np.sin(2 * np.pi * 440 * np.arange(48000) / 48000)).astype(np.float32)
        a = bridge._mel_features(tone)
        b = bridge._mel_features(tone)
        self.assertEqual(a.shape, (1001, 64))
        self.assertEqual(a.dtype, np.float32)
        np.testing.assert_array_equal(a, b)  # 同一波形两次完全一致
        c = bridge._mel_features(silence)
        self.assertFalse(bool(np.allclose(a, c)))  # 静音 vs 正弦得到不同特征
        long_wave = np.tile(tone, 11)  # 528000 > 480000，触发确定性取开头
        self.assertGreater(long_wave.size, 480000)
        long_features = bridge._mel_features(long_wave)
        np.testing.assert_array_equal(long_features, bridge._mel_features(long_wave))
        np.testing.assert_array_equal(long_features, bridge._mel_features(long_wave[:480000]))
        short = bridge._mel_features(tone[:12000])  # 短音频 repeatpad 分支
        self.assertEqual(short.shape, (1001, 64))
        self.assertTrue(bool(np.isfinite(short).all()))

    def test_encode_audio_rejects_bad_outputs(self):
        silence = np.zeros(48000, dtype=np.float32)
        request = dict(self.request, op="encode_audio", audio_path=str(self.root / "a.wav"))
        cases = [
            ([np.array([[np.nan] * 512], dtype=np.float32)], "非有限"),
            ([np.full((1, 512), float("inf"), dtype=np.float32)], "非有限"),
            ([np.zeros((1, 512), dtype=np.float32)], "范数"),
            ([np.ones((1, 7), dtype=np.float32)], "维度"),
        ]
        for values, keyword in cases:
            session = AudioSession(values)
            with self.assertRaisesRegex(ValueError, keyword):
                bridge.encode_audio(request, decoder_factory=fake_decoder(silence),
                                    session_factory=audio_factory(session))

    def test_encode_audio_encoder_factory_path(self):
        class Fixed:
            def __init__(self, request):
                pass

            def encode_audio(self, path):
                return [3.0, 4.0] + [0.0] * 510

        class NonFinite(Fixed):
            def encode_audio(self, path):
                return [1.0, float("nan")] + [0.0] * 510

        class Zero(Fixed):
            def encode_audio(self, path):
                return [0.0] * 512

        class WrongDim(Fixed):
            def encode_audio(self, path):
                return [1.0, 2.0]

        request = dict(self.request, op="encode_audio", audio_path=str(self.root / "a.wav"))
        result = bridge.encode_audio(request, encoder_factory=Fixed)
        self.assertTrue(result["ok"])
        self.assertEqual(result["dim"], 512)
        self.assertAlmostEqual(float(np.linalg.norm(result["vector"])), 1.0, places=9)
        self.assertAlmostEqual(result["vector"][0], 0.6, places=9)
        self.assertIn("不宣称语义检索质量", result["note"])
        for cls, keyword in [(NonFinite, "非有限"), (Zero, "范数"), (WrongDim, "维度")]:
            with self.assertRaisesRegex(ValueError, keyword):
                bridge.encode_audio(request, encoder_factory=cls)

    def test_encode_audio_validation(self):
        self.assertIn("encode_audio", bridge.OPS)
        self.assertIn("audio_path", bridge.PATH_KEYS)
        base = dict(self.request, op="encode_audio", audio_path=str(self.root / "a.wav"))
        bridge.validate(base)
        for bad in ["relative.wav", str(self.root) + "/../escape.wav", "a" * 32769, 123, None]:
            with self.assertRaises(ValueError):
                bridge.validate(dict(base, audio_path=bad))
        missing = dict(base)
        del missing["audio_path"]
        with self.assertRaises(ValueError):  # 缺少 audio_path
            bridge.validate(missing)
        for revision in ["main", "A" * 40, "a" * 40, "a" * 41]:
            with self.assertRaises(ValueError):
                bridge.validate(dict(base, revision=revision))
        with self.assertRaises(ValueError):  # 未知 op
            bridge.validate(dict(base, op="nonsense"))

    def test_real_decode_audio_ffmpeg_contract(self):
        samples = np.array([0.0, 0.1, -0.2, 0.3], dtype="<f4")
        completed = mock.Mock(returncode=0, stdout=samples.tobytes(), stderr=b"")
        with mock.patch.object(bridge.subprocess, "run", return_value=completed) as run:
            waveform = bridge._real_decode_audio("D:/in.wav")
        self.assertEqual(waveform.dtype, np.float32)
        np.testing.assert_allclose(waveform, samples.astype(np.float32))
        command = run.call_args.args[0]
        self.assertIsInstance(command, list)  # 禁止 shell=True
        self.assertIn("-nostdin", command)
        self.assertIn("-t", command)
        self.assertIn("-", command)
        self.assertFalse(run.call_args.kwargs.get("shell", False))
        self.assertEqual(run.call_args.kwargs.get("timeout"), 60)

        failed = mock.Mock(returncode=1, stdout=b"", stderr=b"boom")
        with mock.patch.object(bridge.subprocess, "run", return_value=failed):
            with self.assertRaisesRegex(ValueError, "FFmpeg 解码失败"):
                bridge._real_decode_audio("D:/in.wav")
        empty = mock.Mock(returncode=0, stdout=b"", stderr=b"")
        with mock.patch.object(bridge.subprocess, "run", return_value=empty):
            with self.assertRaisesRegex(ValueError, "未解码出音频样本"):
                bridge._real_decode_audio("D:/in.wav")

    def test_ffmpeg_path_resolution(self):
        with mock.patch.dict(os.environ, {"SW_FFMPEG_PATH": "D:/custom/ffmpeg.exe"}):
            self.assertEqual(bridge._ffmpeg_path(), "D:/custom/ffmpeg.exe")
        with mock.patch.dict(os.environ, {}, clear=True):
            with mock.patch.object(bridge.shutil, "which", return_value=None):
                with self.assertRaisesRegex(ValueError, "未找到 FFmpeg"):
                    bridge._ffmpeg_path()

    def test_notes_state_unverified_boundaries(self):
        for text in ["确定性 rand_trunc 近似", "未做中文相关性评测",
                     "未宣称语义检索质量", "逐帧对齐验证"]:
            self.assertIn(text, bridge.STATUS_NOTE)
        for text in ["不宣称语义检索质量", "无 transformers/torch 做逐帧对照验证",
                     "确定性取开头", "未做中文相关性评测"]:
            self.assertIn(text, bridge.ENCODE_AUDIO_NOTE)

    # ---- encode_audio_batch 新增测试 ----

    def test_encode_audio_batch_validation(self):
        self.assertIn("encode_audio_batch", bridge.OPS)
        self.assertNotIn("audio_paths", bridge.PATH_KEYS)  # PATH_KEYS 保持不变
        valid = str(self.root / "a.wav")
        base = dict(self.request, op="encode_audio_batch", audio_paths=[valid])
        bridge.validate(base)
        bridge.validate(dict(base, audio_paths=[valid] * 32))
        for bad in [[], [valid] * 33, "not-a-list", {"a": 1}, None,
                    ["relative.wav"], [str(self.root) + "/../escape.wav"],
                    [valid, 123], [valid, None], ["a" * 32769]]:
            with self.assertRaises(ValueError):
                bridge.validate(dict(base, audio_paths=bad))
        missing = dict(base)
        del missing["audio_paths"]
        with self.assertRaises(ValueError):  # 缺少 audio_paths
            bridge.validate(missing)
        with self.assertRaises(ValueError):  # 未知 op 仍拒绝
            bridge.validate(dict(base, op="nonsense"))

    def test_encode_audio_batch_single_session_and_count(self):
        tone = (0.2 * np.sin(2 * np.pi * 440 * np.arange(48000) / 48000)).astype(np.float32)
        session = AudioSession([np.full((1, 512), 0.5, dtype=np.float32)])
        paths = [str(self.root / ("f%d.wav" % i)) for i in range(4)]
        dec_calls, ses_calls = [], []
        request = dict(self.request, op="encode_audio_batch", audio_paths=paths)
        result = bridge.encode_audio_batch(
            request,
            decoder_factory=recording_decoder(tone, dec_calls),
            session_factory=recording_audio_factory(session, ses_calls))
        self.assertTrue(result["ok"])
        self.assertEqual(result["count"], 4)
        self.assertEqual(result["dim"], 512)
        self.assertEqual(result["capability"], "audio_text")
        self.assertEqual(result["revision"], bridge.REVISION)
        self.assertEqual(result["note"], bridge.ENCODE_AUDIO_NOTE)
        self.assertEqual(len(result["vectors"]), 4)
        self.assertEqual(result["failures"], [])
        self.assertEqual(len(ses_calls), 1)  # 只加载一次 ONNX session
        self.assertEqual(dec_calls, paths)  # decoder 调用 N 次且顺序一致
        for vector in result["vectors"]:
            self.assertEqual(len(vector), 512)
            self.assertAlmostEqual(float(np.linalg.norm(vector)), 1.0, places=9)

    def test_encode_audio_batch_failure_isolation(self):
        tone = (0.2 * np.sin(2 * np.pi * 440 * np.arange(48000) / 48000)).astype(np.float32)
        session = AudioSession([np.full((1, 512), 0.5, dtype=np.float32)])
        paths = [str(self.root / ("f%d.wav" % i)) for i in range(3)]
        bad_path = paths[1]

        def flaky_decoder(path):
            if path == bad_path:
                raise ValueError("模拟解码失败")
            return tone

        request = dict(self.request, op="encode_audio_batch", audio_paths=paths)
        result = bridge.encode_audio_batch(request, decoder_factory=flaky_decoder,
                                           session_factory=audio_factory(session))
        self.assertTrue(result["ok"])
        self.assertEqual(result["count"], 2)
        self.assertEqual(len(result["vectors"]), 3)
        self.assertIsNotNone(result["vectors"][0])
        self.assertIsNone(result["vectors"][1])
        self.assertIsNotNone(result["vectors"][2])
        self.assertEqual(len(result["failures"]), 1)
        self.assertEqual(result["failures"][0]["path"], bad_path)
        self.assertIn("模拟解码失败", result["failures"][0]["error"])

    def test_encode_audio_batch_rejects_bad_outputs(self):
        tone = (0.2 * np.sin(2 * np.pi * 440 * np.arange(48000) / 48000)).astype(np.float32)
        values = [
            [np.ones((1, 512), dtype=np.float32)],  # 正常
            [np.array([[np.nan] * 512], dtype=np.float32)],  # 非有限
            [np.ones((1, 7), dtype=np.float32)],  # 维度不对
        ]
        session = SequenceSession(values)
        paths = [str(self.root / ("f%d.wav" % i)) for i in range(3)]
        request = dict(self.request, op="encode_audio_batch", audio_paths=paths)
        result = bridge.encode_audio_batch(request, decoder_factory=fake_decoder(tone),
                                           session_factory=audio_factory(session))
        self.assertEqual(result["count"], 1)
        self.assertIsNotNone(result["vectors"][0])
        self.assertIsNone(result["vectors"][1])
        self.assertIsNone(result["vectors"][2])
        self.assertEqual(len(result["failures"]), 2)
        self.assertEqual(result["failures"][0]["path"], paths[1])
        self.assertIn("非有限", result["failures"][0]["error"])
        self.assertEqual(result["failures"][1]["path"], paths[2])
        self.assertIn("维度", result["failures"][1]["error"])

    def test_encode_audio_batch_error_is_truncated(self):
        session = AudioSession([np.full((1, 512), 0.5, dtype=np.float32)])
        paths = [str(self.root / "f.wav")]
        request = dict(self.request, op="encode_audio_batch", audio_paths=paths)

        def always_fail(path):
            raise ValueError("E" * 900)

        result = bridge.encode_audio_batch(request, decoder_factory=always_fail,
                                           session_factory=audio_factory(session))
        self.assertEqual(result["count"], 0)
        self.assertEqual(len(result["vectors"]), 1)
        self.assertIsNone(result["vectors"][0])
        self.assertEqual(len(result["failures"]), 1)
        self.assertEqual(result["failures"][0]["path"], paths[0])
        self.assertEqual(len(result["failures"][0]["error"]), 500)


if __name__ == "__main__":
    unittest.main()
