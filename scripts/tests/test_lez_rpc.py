"""scripts/lib/lez_rpc.py: the offline parts of the read-only testnet probes (no network)."""
import base64
import hashlib
import importlib.util
import pathlib
import struct
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("lez_rpc", ROOT / "scripts" / "lib" / "lez_rpc.py")
rpc = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rpc)

GENESIS = "c8e3bb9ec8f246e5c2759e0e8152840aeb7df29948a9ad12c9c302a641230011"


class DeployHashTest(unittest.TestCase):
    def test_is_sha256_of_the_borsh_length_prefixed_bytecode(self):
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "g.bin"
            path.write_bytes(b"\xca\xfe")
            got = rpc.deploy_hash(path)
        self.assertEqual(got, hashlib.sha256(b"\x02\x00\x00\x00\xca\xfe").hexdigest())

    def test_matches_the_token_deployment_seen_on_the_legacy_testnet(self):
        # getTransaction(this hash) on testnet-legacy returned block 2301 (2026-10-17).
        self.assertEqual(rpc.deploy_hash(ROOT / "artifacts" / "guests" / "token.bin"),
                         "43fab3ef2a83e7037c389c8c33a4126ab0f066e87e3611dac3e3bea04dae83e8")


class BlockHashTest(unittest.TestCase):
    def block(self, block_id, hash_hex):
        raw = struct.pack("<Q", block_id) + bytes(32) + bytes.fromhex(hash_hex) + bytes(16)
        return base64.b64encode(raw).decode()

    def test_reads_the_header_hash_of_block_1(self):
        self.assertEqual(rpc.block_hash(self.block(1, GENESIS), 1), GENESIS)

    def test_refuses_another_block_or_encoding(self):
        with self.assertRaises(SystemExit):
            rpc.block_hash(self.block(2, GENESIS), 1)
        with self.assertRaises(SystemExit):
            rpc.block_hash({"header": {}}, 1)
        with self.assertRaises(SystemExit):
            rpc.block_hash(base64.b64encode(b"short").decode(), 1)


class RetryTest(unittest.TestCase):
    def flaky(self, failures, transient=True):
        calls = []

        def once(url, method, params):
            calls.append(method)
            if len(calls) <= failures:
                raise rpc.RpcError("HTTP 502 Bad Gateway", transient)
            return 42
        return once, calls

    def test_retries_a_transient_failure_with_backoff(self):
        once, calls = self.flaky(2)
        slept = []
        self.assertEqual(rpc.rpc("u", "getLastBlockId", [], sleep=slept.append, once=once), 42)
        self.assertEqual(len(calls), 3)
        self.assertEqual(slept, [2, 4])

    def test_gives_up_after_five_tries(self):
        once, calls = self.flaky(99)
        with self.assertRaises(rpc.RpcError) as e:
            rpc.rpc("u", "getLastBlockId", [], sleep=lambda _: None, once=once)
        self.assertEqual(len(calls), 5)
        self.assertIn("after 5 attempts", str(e.exception))

    def test_a_json_rpc_error_is_not_retried(self):
        once, calls = self.flaky(99, transient=False)
        with self.assertRaises(rpc.RpcError):
            rpc.rpc("u", "getBlock", [1], sleep=lambda _: None, once=once)
        self.assertEqual(len(calls), 1)

    def test_the_default_backoff_totals_thirty_seconds(self):
        once, _ = self.flaky(4)
        slept = []
        rpc.rpc("u", "getLastBlockId", [], sleep=slept.append, once=once)
        self.assertEqual(sum(slept), 30)

    def test_http_5xx_and_transport_errors_are_transient_4xx_is_not(self):
        import urllib.error

        def opener_raising(exc):
            def opener(req, timeout):
                raise exc
            return opener
        for exc, transient in [
            (urllib.error.HTTPError("u", 502, "Bad Gateway", {}, None), True),
            (urllib.error.HTTPError("u", 404, "Not Found", {}, None), False),
            (urllib.error.URLError("dns"), True),
        ]:
            with self.assertRaises(rpc.RpcError) as e:
                rpc.rpc_once("http://node.invalid/", "getLastBlockId", [], opener=opener_raising(exc))
            self.assertEqual(e.exception.transient, transient, exc)


if __name__ == "__main__":
    unittest.main()
