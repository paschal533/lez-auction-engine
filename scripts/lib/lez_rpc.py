#!/usr/bin/env python3
"""Read-only LEZ v0.2.4 JSON-RPC probes for the network scripts (scripts/testnet-legacy.sh).

Never sends a transaction. Subcommands:
  genesis URL            hash of block 1 (the chain identity, ADR-046)
  last-block URL         the node's last block id
  deploy-hash BIN        hash of the unsigned ProgramDeploymentTransaction of BIN (offline)
  deployed URL BIN       block id that included BIN's deployment transaction, or "no"
  nonces URL ID...       the nonces of these base58 account ids, space separated, in order

The deployment transaction of v0.2.4 is `borsh(ProgramDeploymentTransaction { message: { bytecode } })`
(a u32 little-endian length, then the bytes) and its hash is the SHA-256 of that encoding. It has no
signer and no nonce, so the hash depends on the bytes alone: `getTransaction(hash)` finds the
transaction if, and only if, exactly these bytes were deployed on this chain. A program deployed from
other bytes, or present in the genesis state, is not seen ("no").
"""
import base64
import hashlib
import json
import struct
import sys
import time
import urllib.error
import urllib.request

TIMEOUT_S = 30
# Bounded retry of these idempotent reads (the scripts never send through this module): 5 tries,
# waiting 2, 4, 8 and 16 s between them, on a transport failure, an HTTP 5xx or an unparsable reply.
ATTEMPTS = 5
FIRST_DELAY_S = 2


class RpcError(Exception):
    def __init__(self, message, transient):
        super().__init__(message)
        self.transient = transient


def rpc_once(url, method, params, opener=urllib.request.urlopen):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
    try:
        with opener(req, timeout=TIMEOUT_S) as resp:
            reply = json.load(resp)
    except urllib.error.HTTPError as e:
        raise RpcError(f"{method} at {url}: HTTP {e.code} {e.reason}", e.code >= 500) from e
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise RpcError(f"{method} at {url}: {e}", True) from e
    if "error" in reply:
        raise RpcError(f"{method} at {url}: {reply['error']}", False)
    return reply.get("result")


def rpc(url, method, params, attempts=ATTEMPTS, first_delay=FIRST_DELAY_S, sleep=time.sleep,
        once=rpc_once):
    for tried in range(1, attempts + 1):
        try:
            return once(url, method, params)
        except RpcError as e:
            if not e.transient or tried == attempts:
                suffix = f" (after {tried} attempts)" if tried > 1 else ""
                raise RpcError(f"{e}{suffix}", e.transient) from e
            sleep(first_delay * 2 ** (tried - 1))
    raise AssertionError("unreachable")


def block_hash(block, block_id):
    """The node returns a block as base64 borsh: header first, `block_id: u64`,
    `prev_block_hash: [u8; 32]`, `hash: [u8; 32]`, ..."""
    if not isinstance(block, str):
        sys.exit(f"getBlock: unexpected block encoding {type(block).__name__}")
    raw = base64.b64decode(block)
    if len(raw) < 72 or struct.unpack_from("<Q", raw)[0] != block_id:
        sys.exit(f"getBlock: the reply is not block {block_id}")
    return raw[40:72].hex()


def deploy_hash(path):
    with open(path, "rb") as f:
        code = f.read()
    return hashlib.sha256(struct.pack("<I", len(code)) + code).hexdigest()


def main(argv):
    try:
        run(argv)
    except RpcError as e:
        sys.exit(str(e))


def run(argv):
    if len(argv) == 2 and argv[0] == "deploy-hash":
        print(deploy_hash(argv[1]))
    elif len(argv) == 2 and argv[0] == "genesis":
        block = rpc(argv[1], "getBlock", [1])
        if not block:
            sys.exit(f"getBlock at {argv[1]}: the node returned no block 1")
        print(block_hash(block, 1))
    elif len(argv) == 2 and argv[0] == "last-block":
        print(int(rpc(argv[1], "getLastBlockId", [])))
    elif len(argv) >= 3 and argv[0] == "nonces":
        got = rpc(argv[1], "getAccountsNonces", [argv[2:]])
        if not isinstance(got, list) or len(got) != len(argv) - 2:
            sys.exit(f"getAccountsNonces at {argv[1]}: asked for {len(argv) - 2} nonces, got {got!r:.80}")
        print(" ".join(str(int(n)) for n in got))
    elif len(argv) == 3 and argv[0] == "deployed":
        found = rpc(argv[1], "getTransaction", [deploy_hash(argv[2])])
        print("no" if found is None else int(found[1]))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
