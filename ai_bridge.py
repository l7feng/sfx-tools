"""Local E5 metadata semantics. Never opens audio; one JSON request/response."""
from contextlib import closing
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import struct
import sys

MODEL_ID = "intfloat/multilingual-e5-small"
DIM = 384
MAX_REQUEST = 2_000_000


def readonly(path):
    return closing(sqlite3.connect(Path(path).resolve().as_uri() + "?mode=ro", uri=True, timeout=10))


def validate(request):
    if not isinstance(request, dict) or request.get("op") not in ("status", "build", "search"):
        raise ValueError("op must be status/build/search")
    for key in ("db", "index", "model_dir"):
        value = request.get(key)
        if not isinstance(value, str) or not value or len(value) > 32768 or not Path(value).is_absolute():
            raise ValueError(key + " must be an absolute path")
    if Path(request["db"]).resolve() == Path(request["index"]).resolve():
        raise ValueError("index must be separate from source catalog")
    if Path(request["index"]).exists() and Path(request["db"]).exists() and os.path.samefile(request["index"], request["db"]):
        raise ValueError("index must not alias source catalog")
    revision = request.get("revision")
    if not isinstance(revision, str) or len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
        raise ValueError("revision must be a pinned 40-character commit")
    for key in ("progress_path", "cancel_path"):
        if request.get(key) is not None:
            p = request[key]
            if not isinstance(p, str) or len(p) > 32768 or not Path(p).is_absolute():
                raise ValueError(key + " must be an absolute path")
            if Path(p).resolve() in (Path(request["db"]).resolve(), Path(request["index"]).resolve()):
                raise ValueError("control paths must be separate from databases")
    ids = request.get("selected_ids")
    if ids is not None and (not isinstance(ids, list) or len(ids) > 10000 or any(not isinstance(x, str) or not 1 <= len(x) <= 128 for x in ids)):
        raise ValueError("selected_ids must contain at most 10000 bounded IDs")
    if request["op"] == "search":
        query = request.get("query")
        if not isinstance(query, str) or not query.strip() or len(query) > 1024:
            raise ValueError("query must contain 1..1024 characters")
        k = request.get("top_k", 30)
        if isinstance(k, bool) or not isinstance(k, int) or not 1 <= k <= 100:
            raise ValueError("top_k must be 1..100")
    return request


def catalog_rows(path):
    with readonly(path) as conn:
        rows = conn.execute("SELECT a.id,a.name,a.path,COALESCE(m.tags,'[]') FROM assets a LEFT JOIN user_metadata m ON a.id=m.asset_id WHERE a.status='ready' ORDER BY a.id").fetchall()
    result = {}
    for asset_id, name, path, raw_tags in rows:
        tags = json.loads(raw_tags)
        if not isinstance(tags, list) or any(not isinstance(t, str) for t in tags):
            raise ValueError("invalid tags for asset " + asset_id)
        text = "name: " + name + "\npath: " + path + "\ntags: " + ", ".join(tags)
        result[asset_id] = (text, hashlib.sha256(text.encode("utf-8")).hexdigest())
    return result


def manifest(request, verify=False):
    directory = Path(request["model_dir"])
    data = json.loads((directory / "model-manifest.json").read_text(encoding="utf-8-sig"))
    if data.get("model_id") != MODEL_ID or data.get("revision") != request["revision"]:
        raise ValueError("model manifest revision/identity mismatch")
    files = {f["path"]: f for f in data.get("files", [])}
    for relative in ("onnx/model.onnx", "tokenizer.json"):
        entry = files.get(relative)
        path = directory / relative
        if not entry or not path.is_file() or path.stat().st_size != entry.get("size"):
            raise ValueError("model missing/incomplete: " + relative)
        if verify:
            check_cancel(request)
            h = hashlib.sha256()
            with path.open("rb") as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    h.update(block)
                    check_cancel(request)
            if h.hexdigest() != entry.get("sha256"):
                raise ValueError("model checksum mismatch: " + relative)
    return data


class Encoder:
    def __init__(self, request):
        manifest(request, verify=True)
        import numpy as np
        import onnxruntime as ort
        from tokenizers import Tokenizer
        self.np = np
        self.tokenizer = Tokenizer.from_file(str(Path(request["model_dir"]) / "tokenizer.json"))
        self.tokenizer.enable_truncation(max_length=256)
        self.tokenizer.enable_padding(pad_id=1, pad_token="<pad>")
        options = ort.SessionOptions()
        options.intra_op_num_threads = 2
        self.session = ort.InferenceSession(str(Path(request["model_dir"]) / "onnx/model.onnx"), sess_options=options, providers=["CPUExecutionProvider"])

    def encode(self, texts, query=False):
        if not 1 <= len(texts) <= 16:
            raise ValueError("embedding batch must be 1..16")
        np = self.np
        encoded = self.tokenizer.encode_batch([("query: " if query else "passage: ") + t for t in texts])
        inputs = {"input_ids": np.asarray([e.ids for e in encoded], dtype=np.int64), "attention_mask": np.asarray([e.attention_mask for e in encoded], dtype=np.int64)}
        if any(i.name == "token_type_ids" for i in self.session.get_inputs()):
            inputs["token_type_ids"] = np.zeros_like(inputs["input_ids"])
        hidden = self.session.run(["last_hidden_state"], inputs)[0]
        if hidden.ndim != 3 or hidden.shape[2] != DIM:
            raise ValueError("unexpected model embedding dimension")
        mask = inputs["attention_mask"][..., None]
        vectors = (hidden * mask).sum(axis=1) / mask.sum(axis=1).clip(min=1)
        norms = np.linalg.norm(vectors, axis=1, keepdims=True)
        if not np.isfinite(vectors).all() or (norms <= 0).any():
            raise ValueError("invalid model vector")
        return (vectors / norms).tolist()


def check_cancel(request):
    if request.get("cancel_path") and Path(request["cancel_path"]).exists():
        raise ValueError("cancelled")


def progress(request, processed, total, status):
    if request.get("progress_path"):
        path = Path(request["progress_path"])
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(path.name + ".tmp")
        tmp.write_text(json.dumps(dict(processed=processed, total=total, status=status)), encoding="utf-8")
        os.replace(tmp, path)


def index_meta(conn, revision):
    data = dict(conn.execute("SELECT key,value FROM meta"))
    if data.get("revision") != revision or data.get("dim") != str(DIM) or data.get("schema") != "1":
        raise ValueError("index model version mismatch; rebuild index")
    return data


def pack(vector):
    if len(vector) != DIM or not all(math.isfinite(v) for v in vector):
        raise ValueError("invalid embedding")
    return struct.pack("<" + "f" * DIM, *vector)


def build(request, encoder_factory=Encoder):
    check_cancel(request)
    rows = catalog_rows(request["db"])
    selected = request.get("selected_ids")
    targets = rows if selected is None else {k: rows[k] for k in set(selected) if k in rows}
    progress(request, 0, len(targets), "loading")
    # Verify model before touching index; a failed or killed build keeps committed data.
    encoder = encoder_factory(request)
    Path(request["index"]).parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(request["index"], timeout=10)
    try:
        conn.execute("BEGIN IMMEDIATE")
        conn.execute("CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL)")
        conn.execute("CREATE TABLE IF NOT EXISTS vectors(asset_id TEXT PRIMARY KEY,text_hash TEXT NOT NULL,embedding BLOB NOT NULL)")
        metadata = dict(conn.execute("SELECT key,value FROM meta"))
        if metadata and (metadata.get("revision") != request["revision"] or metadata.get("dim") != str(DIM) or metadata.get("schema") != "1"):
            if selected is not None:
                raise ValueError("model changed: full rebuild required")
            conn.execute("DELETE FROM vectors")
        previous = dict(conn.execute("SELECT asset_id,text_hash FROM vectors"))
        stale = [key for key, digest in previous.items() if key not in rows or rows[key][1] != digest]
        conn.executemany("DELETE FROM vectors WHERE asset_id=?", [(k,) for k in stale])
        pending = [(key, text, digest) for key, (text, digest) in targets.items() if previous.get(key) != digest]
        skipped = len(targets) - len(pending)
        for offset in range(0, len(pending), 16):
            check_cancel(request)
            batch = pending[offset:offset + 16]
            vectors = encoder.encode([item[1] for item in batch])
            if len(vectors) != len(batch):
                raise ValueError("model batch size mismatch")
            conn.executemany("INSERT OR REPLACE INTO vectors VALUES(?,?,?)", [(item[0], item[2], pack(vec)) for item, vec in zip(batch, vectors)])
            progress(request, skipped + offset + len(batch), len(targets), "building")
        check_cancel(request)
        conn.executemany("INSERT OR REPLACE INTO meta VALUES(?,?)", [("schema", "1"), ("revision", request["revision"]), ("dim", str(DIM))])
        conn.commit()
        count = conn.execute("SELECT COUNT(*) FROM vectors").fetchone()[0]
    except BaseException:
        conn.rollback()
        raise
    finally:
        conn.close()
    progress(request, len(targets), len(targets), "completed")
    return dict(ok=True, indexed=count, skipped=skipped, deleted=len(stale))


def search(request, encoder_factory=Encoder):
    check_cancel(request)
    with readonly(request["index"]) as conn:
        index_meta(conn, request["revision"])
        indexed = conn.execute("SELECT asset_id,text_hash,embedding FROM vectors").fetchall()
    query = encoder_factory(request).encode([request["query"].strip()], query=True)[0]
    # Read current metadata AFTER inference so edits/deletions during model loading are excluded.
    rows = catalog_rows(request["db"])
    selected = request.get("selected_ids")
    selected = None if selected is None else set(selected)
    scores, stale = [], 0
    for asset_id, digest, blob in indexed:
        check_cancel(request)
        if asset_id not in rows or rows[asset_id][1] != digest:
            stale += 1
            continue
        if selected is not None and asset_id not in selected:
            continue
        if len(blob) != DIM * 4:
            raise ValueError("corrupt index vector")
        vector = struct.unpack("<" + "f" * DIM, blob)
        score = sum(a * b for a, b in zip(query, vector))
        if not math.isfinite(score):
            raise ValueError("non-finite index vector")
        scores.append(dict(id=asset_id, score=max(-1.0, min(1.0, score))))
    scores.sort(key=lambda item: (-item["score"], item["id"]))
    return dict(ok=True, items=scores[:request.get("top_k", 30)], stale=stale)


def status(request):
    model_ready = False
    try:
        manifest(request)
        model_ready = True
    except (OSError, ValueError, KeyError, TypeError):
        pass
    result = dict(ok=True, available=False, indexed=0, revision=None, model_ready=model_ready)
    if Path(request["index"]).is_file():
        with readonly(request["index"]) as conn:
            tables = {row[0] for row in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
            if not {"meta", "vectors"}.issubset(tables):
                return result
            data = dict(conn.execute("SELECT key,value FROM meta"))
            result["revision"] = data.get("revision")
            result["indexed"] = conn.execute("SELECT COUNT(*) FROM vectors").fetchone()[0]
            result["available"] = model_ready and data.get("revision") == request["revision"] and data.get("dim") == str(DIM) and data.get("schema") == "1"
    return result


def main():
    try:
        raw = sys.stdin.buffer.read(MAX_REQUEST + 1)
        if len(raw) > MAX_REQUEST:
            raise ValueError("request too large")
        request = validate(json.loads(raw))
        result = {"status": status, "build": build, "search": search}[request["op"]](request)
    except Exception as error:
        print(str(error), file=sys.stderr)
        result = dict(ok=False, error=str(error))
    print(json.dumps(result, ensure_ascii=True, allow_nan=False))


if __name__ == "__main__":
    main()
