"""Loopback-only development account issuer. No payment network; never bundled into EXE.

Usage: python tools/license-dev-service.py --data-dir D:/.../build/licensing-dev --port 48761
Private key uses current Windows user's DPAPI protection. Public blob may be explicitly trusted by the client.
"""
import argparse
from contextlib import contextmanager
import ctypes as c
from ctypes import wintypes as w
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import secrets
import sqlite3
import struct
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_BODY = 64 * 1024
ITERATIONS = 400_000
ISSUER = "sound-workbench-dev-v1"


def encode(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


class Blob(c.Structure):
    _fields_ = [("size", w.DWORD), ("data", c.POINTER(c.c_ubyte))]


def protect(data, decrypt=False):
    if os.name != "nt":
        raise ValueError("开发发行服务仅支持Windows CNG/DPAPI")
    source = (c.c_ubyte * len(data)).from_buffer_copy(data)
    incoming = Blob(len(data), source)
    outgoing = Blob()
    crypt = c.WinDLL("crypt32", use_last_error=True)
    kernel = c.WinDLL("kernel32", use_last_error=True)
    kernel.LocalFree.argtypes = [c.c_void_p]
    kernel.LocalFree.restype = c.c_void_p
    if decrypt:
        fn = crypt.CryptUnprotectData
        fn.argtypes = [c.POINTER(Blob), c.c_void_p, c.c_void_p, c.c_void_p, c.c_void_p, w.DWORD, c.POINTER(Blob)]
        ok = fn(c.byref(incoming), None, None, None, None, 1, c.byref(outgoing))
    else:
        fn = crypt.CryptProtectData
        fn.argtypes = [c.POINTER(Blob), w.LPCWSTR, c.c_void_p, c.c_void_p, c.c_void_p, w.DWORD, c.POINTER(Blob)]
        ok = fn(c.byref(incoming), "Sound Workbench development issuer", None, None, None, 1, c.byref(outgoing))
    if not ok:
        raise ValueError("DPAPI私钥保护操作失败")
    try:
        return c.string_at(outgoing.data, outgoing.size)
    finally:
        kernel.LocalFree(outgoing.data)


class Rsa:
    def __init__(self, private=None, public=None):
        if os.name != "nt":
            raise ValueError("开发签名使用Windows CNG，仅支持Windows")
        self.lib = c.WinDLL("bcrypt")
        self.algorithm = c.c_void_p()
        self.key = c.c_void_p()
        self.lib.BCryptOpenAlgorithmProvider.argtypes = [c.POINTER(c.c_void_p), w.LPCWSTR, w.LPCWSTR, w.ULONG]
        self.lib.BCryptGenerateKeyPair.argtypes = [c.c_void_p, c.POINTER(c.c_void_p), w.ULONG, w.ULONG]
        self.lib.BCryptFinalizeKeyPair.argtypes = [c.c_void_p, w.ULONG]
        self.lib.BCryptImportKeyPair.argtypes = [c.c_void_p, c.c_void_p, w.LPCWSTR, c.POINTER(c.c_void_p), c.c_void_p, w.ULONG, w.ULONG]
        self.lib.BCryptExportKey.argtypes = [c.c_void_p, c.c_void_p, w.LPCWSTR, c.c_void_p, w.ULONG, c.POINTER(w.ULONG), w.ULONG]
        self.lib.BCryptSignHash.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, w.ULONG, c.c_void_p, w.ULONG, c.POINTER(w.ULONG), w.ULONG]
        self.lib.BCryptVerifySignature.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, w.ULONG, c.c_void_p, w.ULONG, w.ULONG]
        self.lib.BCryptDestroyKey.argtypes = [c.c_void_p]
        self.lib.BCryptCloseAlgorithmProvider.argtypes = [c.c_void_p, w.ULONG]
        self.check(self.lib.BCryptOpenAlgorithmProvider(c.byref(self.algorithm), "RSA", None, 0))
        if private is not None or public is not None:
            data = private if private is not None else public
            raw = (c.c_ubyte * len(data)).from_buffer_copy(data)
            self.check(self.lib.BCryptImportKeyPair(self.algorithm, None, "RSAFULLPRIVATEBLOB" if private is not None else "RSAPUBLICBLOB", c.byref(self.key), raw, len(data), 0))
        else:
            self.check(self.lib.BCryptGenerateKeyPair(self.algorithm, c.byref(self.key), 2048, 0))
            self.check(self.lib.BCryptFinalizeKeyPair(self.key, 0))

    @staticmethod
    def check(code):
        if code != 0:
            raise ValueError("Windows CNG签名操作失败")

    def export(self, private=False):
        name = "RSAFULLPRIVATEBLOB" if private else "RSAPUBLICBLOB"
        size = w.ULONG()
        self.check(self.lib.BCryptExportKey(self.key, None, name, None, 0, c.byref(size), 0))
        out = (c.c_ubyte * size.value)()
        self.check(self.lib.BCryptExportKey(self.key, None, name, out, size.value, c.byref(size), 0))
        return bytes(out[:size.value])

    def sign(self, payload):
        digest = (c.c_ubyte * 32).from_buffer_copy(hashlib.sha256(payload).digest())
        padding = c.c_wchar_p("SHA256")
        size = w.ULONG()
        self.check(self.lib.BCryptSignHash(self.key, c.byref(padding), digest, 32, None, 0, c.byref(size), 2))
        signature = (c.c_ubyte * size.value)()
        self.check(self.lib.BCryptSignHash(self.key, c.byref(padding), digest, 32, signature, size.value, c.byref(size), 2))
        return bytes(signature[:size.value])

    def verify(self, payload, signature):
        digest = (c.c_ubyte * 32).from_buffer_copy(hashlib.sha256(payload).digest())
        raw = (c.c_ubyte * len(signature)).from_buffer_copy(signature)
        padding = c.c_wchar_p("SHA256")
        return self.lib.BCryptVerifySignature(self.key, c.byref(padding), digest, 32, raw, len(signature), 2) == 0

    def close(self):
        if self.key.value:
            self.lib.BCryptDestroyKey(self.key)
            self.key = c.c_void_p()
        if self.algorithm.value:
            self.lib.BCryptCloseAlgorithmProvider(self.algorithm, 0)
            self.algorithm = c.c_void_p()

    def __del__(self):
        if hasattr(self, "key"):
            self.close()


def token_payload(token, public, device_id=None, now=None):
    if not isinstance(token, str) or len(token) > 32768:
        raise ValueError("离线凭证大小无效")
    parts = token.split(".")
    if len(parts) != 2:
        raise ValueError("离线凭证格式无效")
    try:
        payload, signature = map(bytes.fromhex, parts)
    except ValueError:
        raise ValueError("离线凭证编码无效") from None
    if len(payload) > 8192 or len(signature) != 256 or not Rsa(public=public).verify(payload, signature):
        raise ValueError("离线签名无效或不是受信发行端")
    value = json.loads(payload)
    current = int(time.time()) if now is None else now
    if value.get("issuer") != ISSUER or value.get("version") != 1 or value.get("issued_at", current + 301) > current + 300:
        raise ValueError("凭证发行信息无效")
    if device_id is not None and value.get("device_id") != device_id:
        raise ValueError("凭证不属于此设备")
    expiry = value.get("expires_at")
    if expiry is not None and expiry <= current:
        raise ValueError("凭证已过期")
    return value


class Store:
    def __init__(self, root):
        self.root = Path(root).resolve()
        self.root.mkdir(parents=True, exist_ok=True)
        if self.root.is_symlink():
            raise ValueError("发行端目录不能是链接")
        self.lock = threading.Lock()
        secret = self.root / "issuer-private.dpapi"
        public = self.root / "issuer-public.blob"
        if secret.exists():
            self.rsa = Rsa(private=protect(secret.read_bytes(), True))
            if not public.is_file() or public.read_bytes() != self.rsa.export():
                raise ValueError("发行端公钥与私钥不一致，未覆盖")
        else:
            if public.exists():
                raise ValueError("只存在公钥，不能覆盖或猜测发行身份")
            self.rsa = Rsa()
            with secret.open("xb") as file:
                file.write(protect(self.rsa.export(True)))
            with public.open("xb") as file:
                file.write(self.rsa.export())
        self.db = self.root / "accounts.sqlite"
        with self.connection() as db:
            db.executescript("""CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY,email TEXT UNIQUE NOT NULL,salt TEXT NOT NULL,password_hash TEXT NOT NULL,iterations INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS sessions(token_hash TEXT PRIMARY KEY,account_id TEXT REFERENCES accounts(id),expires INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS orders(id TEXT PRIMARY KEY,account_id TEXT REFERENCES accounts(id),state TEXT NOT NULL,created INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY,digest TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS devices(account_id TEXT NOT NULL,device_id TEXT NOT NULL,order_id TEXT NOT NULL REFERENCES orders(id),active INTEGER NOT NULL,changed INTEGER NOT NULL,PRIMARY KEY(account_id,device_id));""")

    @contextmanager
    def connection(self):
        db = sqlite3.connect(self.db, timeout=5)
        db.row_factory = sqlite3.Row
        db.execute("PRAGMA foreign_keys=ON")
        try:
            with db:
                yield db
        finally:
            db.close()

    def handle(self, action, request, bearer=""):
        if not isinstance(request, dict):
            raise ValueError("请求必须是JSON对象")
        with self.lock, self.connection() as db:
            db.execute("BEGIN IMMEDIATE")
            now = int(time.time())
            if action == "register":
                email = str(request.get("email", "")).strip().casefold()
                password = request.get("password")
                if len(email) > 254 or not re.fullmatch(r"[^\s@]{1,64}@[^\s@]{1,190}", email) or not isinstance(password, str) or not 12 <= len(password) <= 256:
                    raise ValueError("请输入有效邮箱和12至256字符密码；这是本地测试账号")
                salt = secrets.token_bytes(32)
                digest = hashlib.pbkdf2_hmac("sha256", password.encode(), salt, ITERATIONS).hex()
                try:
                    db.execute("INSERT INTO accounts VALUES(?,?,?,?,?)", (str(uuid.uuid4()), email, salt.hex(), digest, ITERATIONS))
                except sqlite3.IntegrityError:
                    raise ValueError("该测试账号已存在") from None
                return {"created": True, "development": True}
            if action == "login":
                email = str(request.get("email", "")).strip().casefold()
                password = request.get("password")
                row = db.execute("SELECT * FROM accounts WHERE email=?", (email,)).fetchone()
                salt = bytes.fromhex(row["salt"]) if row else b"0" * 32
                if not isinstance(password, str) or len(password) > 256:
                    raise ValueError("账号或密码不正确")
                digest = hashlib.pbkdf2_hmac("sha256", password.encode(), salt, row["iterations"] if row else ITERATIONS).hex()
                if row is None or not hmac.compare_digest(digest, row["password_hash"]):
                    raise ValueError("账号或密码不正确")
                token = secrets.token_urlsafe(32)
                db.execute("DELETE FROM sessions WHERE expires<=?", (now,))
                db.execute("INSERT INTO sessions VALUES(?,?,?)", (hashlib.sha256(token.encode()).hexdigest(), row["id"], now + 8 * 3600))
                return {"session": token, "email": row["email"], "development": True}
            auth = db.execute("SELECT a.id,a.email FROM sessions s JOIN accounts a ON a.id=s.account_id WHERE s.token_hash=? AND s.expires>?", (hashlib.sha256(bearer.encode()).hexdigest(), now)).fetchone()
            if auth is None:
                raise ValueError("请先登录，或登录已过期")
            account = auth["id"]
            if action == "logout":
                db.execute("DELETE FROM sessions WHERE token_hash=?", (hashlib.sha256(bearer.encode()).hexdigest(),))
                return {"logged_out": True}
            if action == "account":
                orders = [dict(row) for row in db.execute("SELECT id,state,created FROM orders WHERE account_id=? ORDER BY created DESC,id DESC LIMIT 100", (account,))]
                devices = [dict(row) for row in db.execute("SELECT device_id,order_id,active,changed FROM devices WHERE account_id=? ORDER BY changed DESC,device_id LIMIT 100", (account,))]
                current = db.execute("SELECT d.device_id,d.order_id,d.active,o.state AS order_state FROM devices d JOIN orders o ON o.id=d.order_id WHERE d.account_id=? AND d.device_id=?", (account, str(request.get("device_id", "")))).fetchone()
                return {"email": auth["email"], "orders": orders, "devices": devices, "current_device": dict(current) if current else None, "development": True}
            if action == "create_order":
                order = str(uuid.uuid4())
                db.execute("INSERT INTO orders VALUES(?,?,'created',?)", (order, account, now))
                return {"order_id": order, "state": "created", "development": True, "real_payment": False}
            order = str(request.get("order_id", ""))
            row = db.execute("SELECT * FROM orders WHERE id=? AND account_id=?", (order, account)).fetchone()
            if action in ("test_paid", "refund"):
                if row is None:
                    raise ValueError("测试订单不存在")
                event = str(request.get("event_id", ""))
                if not 1 <= len(event) <= 128:
                    raise ValueError("测试事件ID无效")
                digest = hashlib.sha256(encode({"action": action, "order_id": order, "account_id": account})).hexdigest()
                seen = db.execute("SELECT digest FROM events WHERE id=?", (event,)).fetchone()
                if seen:
                    if seen[0] != digest:
                        raise ValueError("事件ID被不同请求复用")
                    return {"idempotent": True, "order_id": order, "state": row["state"]}
                state = "paid" if action == "test_paid" else "refunded"
                if row["state"] == "refunded" or action == "refund" and row["state"] != "paid":
                    raise ValueError("订单状态不允许该变更")
                db.execute("INSERT INTO events VALUES(?,?)", (event, digest))
                db.execute("UPDATE orders SET state=? WHERE id=?", (state, order))
                if state == "refunded":
                    db.execute("UPDATE devices SET active=0,changed=? WHERE account_id=? AND order_id=?", (now, account, order))
                return {"order_id": order, "state": state, "real_payment": False}
            device = str(request.get("device_id", ""))
            if not re.fullmatch(r"[0-9a-f-]{36}", device):
                raise ValueError("测试设备标识无效")
            if action == "deactivate":
                db.execute("UPDATE devices SET active=0,changed=? WHERE account_id=? AND device_id=?", (now, account, device))
                return {"device_id": device, "active": False}
            if action != "activate":
                raise ValueError("不支持的开发动作")
            if row is None or row["state"] != "paid":
                raise ValueError("需要已模拟支付的测试订单，未发生真实收费")
            old = db.execute("SELECT active FROM devices WHERE account_id=? AND device_id=?", (account, device)).fetchone()
            count = db.execute("SELECT COUNT(*) FROM devices WHERE account_id=? AND active=1", (account,)).fetchone()[0]
            if (old is None or not old[0]) and count >= 2:
                raise ValueError("最多两台有效激活设备，请先解绑")
            db.execute("INSERT INTO devices VALUES(?,?,?,1,?) ON CONFLICT(account_id,device_id) DO UPDATE SET order_id=excluded.order_id,active=1,changed=excluded.changed", (account, device, order, now))
            value = {"issuer": ISSUER, "version": 1, "license_id": str(uuid.uuid4()), "subject": account, "order_id": order, "device_id": device, "issued_at": now, "expires_at": None, "features": ["buyout-local"]}
            payload = encode(value)
            token = payload.hex() + "." + self.rsa.sign(payload).hex()
            return {"device_id": device, "active": True, "offline_token": token, "development": True, "offline_revocation": "解绑/退款在联网核验后生效；永久离线凭证不能即时撤销"}


class Handler(BaseHTTPRequestHandler):
    server_version = "SoundWorkbenchDev/1"
    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            if self.client_address[0] != "127.0.0.1" or self.headers.get("Origin"):
                raise ValueError("仅支持原生客户端的本地开发连接")
            if self.headers.get("Content-Type", "").split(";")[0].strip() != "application/json":
                raise ValueError("仅接受application/json")
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 < size <= MAX_BODY:
                raise ValueError("请求大小无效")
            self.connection.settimeout(5)
            request = json.loads(self.rfile.read(size))
            action = self.path.removeprefix("/v1/")
            if self.path != "/v1/" + action or not re.fullmatch("[a-z_]+", action):
                raise ValueError("请求路径无效")
            bearer = self.headers.get("Authorization", "").removeprefix("Bearer ")
            result = self.server.store.handle(action, request, bearer)
            body, code = encode({"ok": True, "result": result}), 200
        except (ValueError, sqlite3.Error, json.JSONDecodeError):
            # Validation errors contain no request body, password or bearer token.
            body, code = encode({"ok": False, "error": str(__import__("sys").exc_info()[1])[:300]}), 400
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser(description="Local development issuer; no real purchases or public deployment")
    parser.add_argument("--data-dir", required=True, type=Path)
    parser.add_argument("--port", default=48761, type=int)
    args = parser.parse_args()
    if not args.data_dir.is_absolute() or not 1024 <= args.port <= 65535:
        parser.error("use an absolute external runtime directory and port1024..65535")
    if "个人知识库" in str(args.data_dir):
        parser.error("runtime/private-key files must remain outside the knowledge vault")
    store = Store(args.data_dir)
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    server.store = store
    server.daemon_threads = True
    print(f"Development issuer listening at 127.0.0.1:{args.port}; no real payments")
    server.serve_forever()


if __name__ == "__main__":
    main()
