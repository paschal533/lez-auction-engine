use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread::JoinHandle,
    time::Duration,
};

use lee::{
    public_transaction::{Message, WitnessSet},
    PublicTransaction,
};
use lee_core::account::{Account, Nonce};

use super::*;
use crate::testing::{id, key};

const SHORT: Duration = Duration::from_millis(400);

/// What the mock server does after reading one full request.
enum Behavior {
    /// HTTP 200 with a JSON-RPC success carrying this `result`.
    Result(serde_json::Value),
    /// HTTP 200 with a JSON-RPC error.
    RpcError(i32, &'static str),
    /// A bare HTTP status with an empty body.
    Status(u16),
    /// HTTP 200 whose body is not JSON.
    Garbled,
    /// Closes the socket without answering.
    CloseAfterRead,
    /// Holds the socket until the client gives up.
    Stall,
}

/// A one-connection HTTP mock. `join` returns the JSON-RPC request it received.
struct Mock {
    url: String,
    handle: JoinHandle<serde_json::Value>,
}

impl Mock {
    fn join(self) -> serde_json::Value {
        self.handle.join().expect("mock thread")
    }
}

fn read_request(stream: &mut TcpStream) -> serde_json::Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let (head_end, len) = loop {
        let n = stream.read(&mut chunk).expect("read request");
        assert!(n > 0, "client closed before sending a request");
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .expect("content-length");
            break (pos + 4, len);
        }
    };
    while buf.len() < head_end + len {
        let n = stream.read(&mut chunk).expect("read body");
        assert!(n > 0, "client closed mid-body");
        buf.extend_from_slice(&chunk[..n]);
    }
    serde_json::from_slice(&buf[head_end..head_end + len]).expect("request json")
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let resp = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes()).expect("write response");
}

fn mock(behavior: Behavior) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_request(&mut stream);
        let rid = request["id"].clone();
        match behavior {
            Behavior::Result(v) => respond(
                &mut stream,
                200,
                &serde_json::json!({"jsonrpc":"2.0","id":rid,"result":v}).to_string(),
            ),
            Behavior::RpcError(code, msg) => respond(
                &mut stream,
                200,
                &serde_json::json!({"jsonrpc":"2.0","id":rid,"error":{"code":code,"message":msg}})
                    .to_string(),
            ),
            Behavior::Status(s) => respond(&mut stream, s, ""),
            Behavior::Garbled => respond(&mut stream, 200, "<html>not json</html>"),
            Behavior::CloseAfterRead => {}
            Behavior::Stall => {
                let mut sink = [0u8; 16];
                // Returns when the client closes the connection after its timeout.
                let _ = stream.read(&mut sink);
            }
        }
        request
    });
    Mock { url, handle }
}

fn tx() -> PublicTransaction {
    let message = Message::new_preserialized(
        [7u32; 8],
        vec![id(1)],
        vec![Nonce::default()],
        vec![1, 2, 3],
    );
    let k = key(1);
    let witness = WitnessSet::for_message(&message, &[&k]);
    PublicTransaction::new(message, witness)
}

fn send_via(behavior: Behavior) -> (SdkResult<[u8; 32]>, serde_json::Value) {
    let m = mock(behavior);
    let chain = RpcChain::connect_with_timeout(&m.url, SHORT).expect("connect");
    let result = chain.send(&tx());
    (result, m.join())
}

#[test]
fn connect_rejects_a_malformed_url() {
    assert!(matches!(
        RpcChain::connect("not a url"),
        Err(SdkError::Chain(_))
    ));
}

#[test]
fn send_success_returns_the_hash_and_uses_the_documented_method() {
    let hash = "ab".repeat(32);
    let (result, request) = send_via(Behavior::Result(serde_json::json!(hash)));
    assert_eq!(result.expect("hash"), [0xab; 32]);
    assert_eq!(request["method"], "sendTransaction");
    assert_eq!(request["params"].as_array().map(Vec::len), Some(1));
}

#[test]
fn send_definite_rejection_is_a_plain_chain_error() {
    let (result, _) = send_via(Behavior::RpcError(
        JSONRPC_INVALID_PARAMS,
        "stateless check failed",
    ));
    match result {
        Err(SdkError::Chain(m)) => assert!(m.contains("rejected") && m.contains("stateless")),
        other => panic!("expected Chain, got {other:?}"),
    }
}

#[test]
fn send_other_rpc_error_is_uncertain() {
    // -32603 internal error: the node may have queued the transaction before failing.
    let (result, _) = send_via(Behavior::RpcError(-32603, "internal"));
    assert!(
        matches!(result, Err(SdkError::SendUncertain(_))),
        "{result:?}"
    );
}

#[test]
fn send_http_5xx_is_uncertain() {
    let (result, _) = send_via(Behavior::Status(500));
    assert!(
        matches!(result, Err(SdkError::SendUncertain(_))),
        "{result:?}"
    );
}

#[test]
fn send_garbled_response_is_uncertain() {
    let (result, _) = send_via(Behavior::Garbled);
    assert!(
        matches!(result, Err(SdkError::SendUncertain(_))),
        "{result:?}"
    );
}

#[test]
fn send_connection_closed_after_the_request_was_read_is_uncertain() {
    let (result, _) = send_via(Behavior::CloseAfterRead);
    assert!(
        matches!(result, Err(SdkError::SendUncertain(_))),
        "{result:?}"
    );
}

#[test]
fn send_timeout_after_the_request_was_written_is_uncertain() {
    let (result, _) = send_via(Behavior::Stall);
    assert!(
        matches!(result, Err(SdkError::SendUncertain(_))),
        "{result:?}"
    );
}

#[test]
fn send_connection_refused_is_a_plain_chain_error() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    drop(listener);
    let chain = RpcChain::connect_with_timeout(&url, Duration::from_secs(5)).expect("connect");
    match chain.send(&tx()) {
        Err(SdkError::Chain(m)) => assert!(m.contains("not delivered"), "{m}"),
        other => panic!("expected Chain, got {other:?}"),
    }
}

#[test]
fn send_dns_failure_is_a_plain_chain_error() {
    // `.invalid` never resolves (RFC 6761); no packet reaches any server.
    let chain =
        RpcChain::connect_with_timeout("http://sequencer.invalid:3040", Duration::from_secs(10))
            .expect("connect");
    match chain.send(&tx()) {
        Err(SdkError::Chain(m)) => assert!(m.contains("not delivered"), "{m}"),
        other => panic!("expected Chain, got {other:?}"),
    }
}

#[test]
fn reads_use_the_documented_methods_and_fail_as_chain_errors() {
    let m = mock(Behavior::Result(serde_json::json!(42)));
    let chain = RpcChain::connect_with_timeout(&m.url, SHORT).expect("connect");
    assert_eq!(chain.last_block_id().expect("height"), 42);
    assert_eq!(m.join()["method"], "getLastBlockId");

    let m = mock(Behavior::Result(
        serde_json::to_value(Account::default()).expect("account json"),
    ));
    let chain = RpcChain::connect_with_timeout(&m.url, SHORT).expect("connect");
    assert_eq!(chain.account(&id(1)).expect("account"), Account::default());
    assert_eq!(m.join()["method"], "getAccount");

    let m = mock(Behavior::Garbled);
    let chain = RpcChain::connect_with_timeout(&m.url, SHORT).expect("connect");
    assert!(matches!(chain.last_block_id(), Err(SdkError::Chain(_))));
    m.join();
}

#[test]
fn nonces_reply_of_the_wrong_length_is_an_error() {
    let m = mock(Behavior::Result(
        serde_json::to_value(vec![Nonce::default()]).expect("json"),
    ));
    let chain = RpcChain::connect_with_timeout(&m.url, SHORT).expect("connect");
    assert!(matches!(
        chain.nonces(&[id(1), id(2)]),
        Err(SdkError::Chain(_))
    ));
    assert_eq!(m.join()["method"], "getAccountsNonces");
}

#[test]
fn blocks_refuses_an_empty_or_oversized_range_without_a_request() {
    // Nothing listens at this address: a request would fail differently (and slowly).
    let chain = RpcChain::connect_with_timeout("http://127.0.0.1:9", SHORT).expect("connect");
    for (from, to) in [(5, 4), (0, MAX_BLOCK_RANGE)] {
        match chain.blocks(from, to) {
            Err(SdkError::Chain(m)) => assert!(m.contains("range"), "{m}"),
            other => panic!("expected range error, got {other:?}"),
        }
    }
}

/// Needs a running localnet sequencer started by `scripts/localnet-smoke.sh step_sequencer` (port
/// 3040): from WSL, `LEZ_RPC_URL=http://127.0.0.1:3040 scripts/wsl-cargo.sh test -p auction_sdk
/// --features rpc,testing -- --ignored live_rpc`.
#[test]
#[ignore = "needs a running sequencer at LEZ_RPC_URL"]
fn live_rpc_reads_height_clock_and_blocks() {
    let url = std::env::var("LEZ_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:3040".into());
    let c = RpcChain::connect(&url).expect("connect");
    let head = c.last_block_id().expect("height");
    assert!(head >= 1);
    assert!(crate::chain::clock_ms(&c).expect("clock") > 0);
    let b = c.blocks(head, head).expect("block");
    assert_eq!(b[0].id, head);
}
