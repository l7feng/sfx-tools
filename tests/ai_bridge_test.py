"""Contract/index tests with synthetic vectors, not a semantic quality evaluation."""
from contextlib import closing
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("ai_bridge", Path(__file__).resolve().parents[1] / "ai_bridge.py")
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)


class FakeEncoder:
    def __init__(self, request):
        pass

    def encode(self, texts, query=False):
        return [[1.0] + [0.0] * (bridge.DIM - 1) for _ in texts]


class BridgeTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.request = dict(op="build", db=str(self.root / "catalog.sqlite"), index=str(self.root / "semantic.sqlite"), model_dir=str(self.root / "model"), revision="a" * 40)
        with closing(sqlite3.connect(self.request["db"], isolation_level=None)) as conn:
            conn.executescript("CREATE TABLE assets(id TEXT PRIMARY KEY,name TEXT,path TEXT,status TEXT); CREATE TABLE user_metadata(asset_id TEXT PRIMARY KEY,tags TEXT);")
            conn.executemany("INSERT INTO assets VALUES(?,?,?,?)", [("a", "雨声.wav", "X:/雨/雨声.wav", "ready"), ("b", "Wind.wav", "X:/风/Wind.wav", "ready"), ("c", "offline.wav", "X:/offline.wav", "missing")])
            conn.execute("INSERT INTO user_metadata VALUES('a',?)", [json.dumps(["夜晚", "rain"])])

    def tearDown(self):
        self.temp.cleanup()

    def build(self, **extra):
        return bridge.build(dict(self.request, **extra), FakeEncoder)

    def search(self, **extra):
        return bridge.search(dict(self.request, op="search", query="night rain", **extra), FakeEncoder)

    def test_build_search_stable_order_selected_and_source_untouched(self):
        before = Path(self.request["db"]).read_bytes()
        self.assertEqual(self.build()["indexed"], 2)
        self.assertEqual([x["id"] for x in self.search()["items"]], ["a", "b"])
        self.assertEqual([x["id"] for x in self.search(selected_ids=["b"])["items"]], ["b"])
        self.assertEqual(self.search(selected_ids=[])["items"], [])
        self.assertEqual(len(self.search(top_k=1)["items"]), 1)
        self.assertEqual(Path(self.request["db"]).read_bytes(), before)
        self.assertEqual(self.build()["skipped"], 2)

    def test_metadata_change_delete_excluded_and_rebuild_incremental(self):
        self.build()
        with closing(sqlite3.connect(self.request["db"], isolation_level=None)) as conn:
            conn.execute("UPDATE user_metadata SET tags='[\"new\"]' WHERE asset_id='a'")
            conn.execute("DELETE FROM assets WHERE id='b'")
        self.assertEqual(self.search()["items"], [])
        self.assertEqual(self.search()["stale"], 2)
        result = self.build()
        self.assertEqual((result["indexed"], result["deleted"]), (1, 2))
        self.assertEqual(self.search()["items"][0]["id"], "a")

    def test_failed_batch_rolls_back_previous_index(self):
        self.build()
        with closing(sqlite3.connect(self.request["db"], isolation_level=None)) as conn:
            conn.execute("UPDATE assets SET name='modified' WHERE id='a'")
        with bridge.readonly(self.request["index"]) as conn:
            before = conn.execute("SELECT * FROM vectors ORDER BY asset_id").fetchall()

        class Failure(FakeEncoder):
            def encode(self, texts, query=False):
                raise RuntimeError("synthetic inference failure")

        with self.assertRaisesRegex(RuntimeError, "synthetic"):
            bridge.build(self.request, Failure)
        with bridge.readonly(self.request["index"]) as conn:
            self.assertEqual(before, conn.execute("SELECT * FROM vectors ORDER BY asset_id").fetchall())

    def test_cancel_after_inference_rolls_back_and_batches_bounded(self):
        self.build()
        cancel = self.root / "cancel"
        with closing(sqlite3.connect(self.request["db"], isolation_level=None)) as conn:
            conn.executemany("INSERT INTO assets VALUES(?,?,?,'ready')", [(str(i), "test", "X:/test" + str(i)) for i in range(35)])
        batches = []

        class CancelEncoder(FakeEncoder):
            def encode(self, texts, query=False):
                batches.append(len(texts))
                cancel.touch()
                return super().encode(texts, query)

        with self.assertRaisesRegex(ValueError, "cancelled"):
            bridge.build(dict(self.request, cancel_path=str(cancel)), CancelEncoder)
        self.assertEqual(batches, [16])
        with bridge.readonly(self.request["index"]) as conn:
            self.assertEqual(conn.execute("SELECT COUNT(*) FROM vectors").fetchone()[0], 2)

    def test_model_revision_mismatch_refused_until_full_rebuild(self):
        self.build()
        with self.assertRaisesRegex(ValueError, "version mismatch"):
            self.search(revision="b" * 40)
        with self.assertRaisesRegex(ValueError, "full rebuild"):
            self.build(revision="b" * 40, selected_ids=["a"])
        self.build(revision="b" * 40)
        self.assertEqual(len(self.search(revision="b" * 40)["items"]), 2)

    def test_validation_rejects_unbounded_or_unsafe_requests(self):
        bridge.validate(self.request)
        for extra in [dict(index=self.request["db"]), dict(db="relative"), dict(revision="main"), dict(selected_ids=["x"] * 10001), dict(selected_ids=[1]), dict(cancel_path=self.request["db"])]:
            with self.assertRaises(ValueError):
                bridge.validate(dict(self.request, **extra))
        for extra in [dict(query=""), dict(query="雨" * 1025), dict(top_k=0), dict(top_k=True), dict(top_k=101)]:
            request = dict(self.request, op="search", query="rain")
            request.update(extra)
            with self.assertRaises(ValueError):
                bridge.validate(request)

    def test_manifest_checks_revision_size_and_hash(self):
        import hashlib
        folder = Path(self.request["model_dir"])
        (folder / "onnx").mkdir(parents=True)
        entries = []
        for name in ("tokenizer.json", "onnx/model.onnx"):
            content = b"synthetic-model"
            (folder / name).write_bytes(content)
            entries.append(dict(path=name, size=len(content), sha256=hashlib.sha256(content).hexdigest()))
        (folder / "model-manifest.json").write_text(json.dumps(dict(model_id=bridge.MODEL_ID, revision=self.request["revision"], files=entries)))
        bridge.manifest(self.request, verify=True)
        (folder / "tokenizer.json").write_bytes(b"synthetic-false")
        with self.assertRaisesRegex(ValueError, "checksum"):
            bridge.manifest(self.request, verify=True)
        with self.assertRaisesRegex(ValueError, "revision"):
            bridge.manifest(dict(self.request, revision="b" * 40))

    def test_status_without_dependencies_and_progress(self):
        self.assertFalse(bridge.status(self.request)["available"])
        # A cancelled first build can leave an empty SQLite file, which is not a ready index.
        with closing(sqlite3.connect(self.request["index"])):
            pass
        self.assertFalse(bridge.status(self.request)["available"])
        target = self.root / "progress.json"
        self.build(progress_path=str(target))
        self.assertEqual(json.loads(target.read_text()), dict(processed=2, total=2, status="completed"))
        result = bridge.status(self.request)
        self.assertFalse(result["model_ready"])
        self.assertEqual(result["indexed"], 2)


if __name__ == "__main__":
    unittest.main()
