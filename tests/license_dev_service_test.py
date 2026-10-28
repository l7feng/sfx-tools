import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import time
import unittest
import uuid

spec = importlib.util.spec_from_file_location("issuer", Path(__file__).resolve().parents[1] / "tools/license-dev-service.py")
issuer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(issuer)


class LicenseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="sound-workbench-license-test-")
        self.store = issuer.Store(Path(self.temp.name))
        self.email = "test@local.invalid"
        self.password = "test-only-password-2026"
        self.store.handle("register", {"email": self.email, "password": self.password})
        self.session = self.store.handle("login", {"email": self.email, "password": self.password})["session"]

    def tearDown(self):
        self.store.rsa.close()
        self.temp.cleanup()

    def request(self, action, **values):
        return self.store.handle(action, values, self.session)

    def paid(self):
        order = self.request("create_order")["order_id"]
        self.request("test_paid", order_id=order, event_id=str(uuid.uuid4()))
        return order

    def test_password_and_sessions_are_hashed(self):
        with self.store.connection() as db:
            row = db.execute("SELECT * FROM accounts").fetchone()
            self.assertNotEqual(row["password_hash"], self.password)
            self.assertEqual(row["iterations"], issuer.ITERATIONS)
            self.assertEqual(len(bytes.fromhex(row["salt"])), 32)
            sessions = [r[0] for r in db.execute("SELECT token_hash FROM sessions")]
            self.assertNotIn(self.session, sessions)
        with self.assertRaises(ValueError):
            self.store.handle("login", {"email": self.email, "password": "wrong"})
        self.request("logout")
        with self.assertRaises(ValueError):
            self.request("account")

    def test_device_limit_and_deactivation(self):
        order = self.paid()
        devices = [str(uuid.uuid4()) for _ in range(3)]
        for device in devices[:2]:
            result = self.request("activate", order_id=order, device_id=device)
            payload = issuer.token_payload(result["offline_token"], self.store.rsa.export(), device)
            self.assertIsNone(payload["expires_at"])
        with self.assertRaisesRegex(ValueError, "两台"):
            self.request("activate", order_id=order, device_id=devices[2])
        self.request("deactivate", device_id=devices[0])
        self.assertTrue(self.request("activate", order_id=order, device_id=devices[2])["active"])

    def test_test_payment_idempotence_and_refund(self):
        order = self.request("create_order")["order_id"]
        event = str(uuid.uuid4())
        self.request("test_paid", order_id=order, event_id=event)
        self.assertTrue(self.request("test_paid", order_id=order, event_id=event)["idempotent"])
        with self.assertRaises(ValueError):
            self.request("refund", order_id=order, event_id=event)
        device = str(uuid.uuid4())
        token = self.request("activate", order_id=order, device_id=device)["offline_token"]
        self.request("refund", order_id=order, event_id=str(uuid.uuid4()))
        account = self.request("account")
        self.assertFalse(account["devices"][0]["active"])
        with self.assertRaises(ValueError):
            self.request("activate", order_id=order, device_id=device)
        # Offline signatures cannot learn server-side refund until a client reconnects.
        self.assertEqual(issuer.token_payload(token, self.store.rsa.export(), device)["device_id"], device)

    def test_signature_tamper_expiry_device_and_trust(self):
        order = self.paid()
        device = str(uuid.uuid4())
        token = self.request("activate", order_id=order, device_id=device)["offline_token"]
        parts = token.split(".")
        changed = ("00" if parts[0][:2] != "00" else "01") + parts[0][2:] + "." + parts[1]
        with self.assertRaises(ValueError):
            issuer.token_payload(changed, self.store.rsa.export(), device)
        with self.assertRaises(ValueError):
            issuer.token_payload(token, self.store.rsa.export(), str(uuid.uuid4()))
        other = issuer.Rsa()
        try:
            with self.assertRaises(ValueError):
                issuer.token_payload(token, other.export(), device)
        finally:
            other.close()
        payload = issuer.token_payload(token, self.store.rsa.export(), device)
        payload["expires_at"] = int(time.time()) - 1
        raw = issuer.encode(payload)
        expired = raw.hex() + "." + self.store.rsa.sign(raw).hex()
        with self.assertRaisesRegex(ValueError, "过期"):
            issuer.token_payload(expired, self.store.rsa.export(), device)

    def test_private_key_protected_and_restart_stable(self):
        protected = (self.store.root / "issuer-private.dpapi").read_bytes()
        self.assertNotEqual(protected, self.store.rsa.export(True))
        second = issuer.Store(self.store.root)
        try:
            self.assertEqual(second.rsa.export(), self.store.rsa.export())
            self.assertEqual(second.handle("account", {}, self.session)["email"], self.email)
        finally:
            second.rsa.close()


if __name__ == "__main__":
    unittest.main()
