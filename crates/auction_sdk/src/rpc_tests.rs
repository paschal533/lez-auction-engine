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
                // Returns when the client closes the connection, or after a short bound.
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("read timeout");
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
    drop(chain);
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
    let chain = RpcChain::connect_with_timeout(&m.url, SHORT)
        .expect("connect")
        .with_read_retry(ReadRetry::NONE);
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

fn block_json(id: u64) -> serde_json::Value {
    use common::block::HashableBlockData;
    let data = HashableBlockData {
        block_id: id,
        prev_block_hash: common::HashType([0; 32]),
        timestamp: 1,
        transactions: vec![],
    };
    serde_json::to_value(data.into_pending_block(&key(1))).expect("block json")
}

#[test]
fn blocks_with_a_wrong_count_or_wrong_ids_are_typed_errors() {
    let cases = [
        (serde_json::json!([block_json(5)]), "asked for 2"),
        (
            serde_json::json!([block_json(5), block_json(9)]),
            "expected block 6",
        ),
    ];
    for (reply, expect) in cases {
        let m = mock(Behavior::Result(reply));
        let chain = RpcChain::connect_with_timeout(&m.url, SHORT).expect("connect");
        match chain.blocks(5, 6) {
            Err(SdkError::Chain(msg)) => assert!(msg.contains(expect), "{msg}"),
            other => panic!("expected Chain, got {other:?}"),
        }
        assert_eq!(m.join()["method"], "getBlockRange");
    }
    let m = mock(Behavior::Result(serde_json::json!([
        block_json(5),
        block_json(6)
    ])));
    let chain = RpcChain::connect_with_timeout(&m.url, SHORT).expect("connect");
    let got = chain.blocks(5, 6).expect("blocks");
    assert_eq!(got.iter().map(|b| b.id).collect::<Vec<_>>(), vec![5, 6]);
    m.join();
}

#[test]
fn use_inside_a_tokio_runtime_is_an_error_not_a_panic() {
    let chain = RpcChain::connect_with_timeout("http://127.0.0.1:9", SHORT).expect("connect");
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("rt");
    rt.block_on(async {
        assert!(
            matches!(chain.last_block_id(), Err(SdkError::Chain(m)) if m.contains("tokio runtime"))
        );
        assert!(
            matches!(chain.send(&tx()), Err(SdkError::Chain(m)) if m.contains("tokio runtime"))
        );
    });
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

/// A mock answering up to `behaviors.len()` connections in order; `join` returns how many requests
/// it received (it stops waiting for the next one after a short idle bound).
fn mock_seq(behaviors: Vec<Behavior>) -> (String, JoinHandle<Vec<serde_json::Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    listener.set_nonblocking(true).expect("nonblocking");
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for behavior in behaviors {
            let deadline = std::time::Instant::now() + Duration::from_millis(1500);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() > deadline {
                            return seen;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            };
            stream.set_nonblocking(false).expect("blocking");
            let request = read_request(&mut stream);
            let rid = request["id"].clone();
            match behavior {
                Behavior::Result(v) => respond(
                    &mut stream,
                    200,
                    &serde_json::json!({"jsonrpc":"2.0","id":rid,"result":v}).to_string(),
                ),
                Behavior::Status(s) => respond(&mut stream, s, ""),
                _ => panic!("mock_seq supports Result and Status"),
            }
            seen.push(request);
        }
        seen
    });
    (url, handle)
}

const NO_WAIT: ReadRetry = ReadRetry {
    attempts: 5,
    first_delay: Duration::ZERO,
};

#[test]
fn the_default_read_retry_is_five_tries_over_about_thirty_seconds() {
    let r = ReadRetry::DEFAULT;
    assert_eq!(r.attempts, 5);
    let total: Duration = (1..r.attempts).map(|a| r.delay_before(a)).sum();
    assert_eq!(total, Duration::from_secs(30));
    assert_eq!(ReadRetry::NONE.attempts, 1);
}

#[test]
fn retry_read_stops_at_the_first_success_and_backs_off() {
    let calls = std::cell::Cell::new(0u32);
    let slept = std::cell::RefCell::new(Vec::new());
    let got: Result<u32, &str> = retry_read(
        &ReadRetry::DEFAULT,
        |_| true,
        &|d| slept.borrow_mut().push(d),
        || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err("transient")
            } else {
                Ok(7)
            }
        },
    );
    assert_eq!(got, Ok(7));
    assert_eq!(calls.get(), 3);
    assert_eq!(
        *slept.borrow(),
        vec![Duration::from_secs(2), Duration::from_secs(4)]
    );
}

#[test]
fn retry_read_gives_up_after_the_limit() {
    let calls = std::cell::Cell::new(0u32);
    let got: Result<u32, &str> = retry_read(
        &NO_WAIT,
        |_| true,
        &|_| {},
        || {
            calls.set(calls.get() + 1);
            Err("down")
        },
    );
    assert_eq!(got, Err("down"));
    assert_eq!(calls.get(), 5);
}

#[test]
fn retry_read_does_not_retry_a_definite_error() {
    let calls = std::cell::Cell::new(0u32);
    let got: Result<u32, &str> = retry_read(
        &NO_WAIT,
        |_| false,
        &|_| {},
        || {
            calls.set(calls.get() + 1);
            Err("invalid params")
        },
    );
    assert_eq!(got, Err("invalid params"));
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_read_is_retried_after_a_transient_http_failure() {
    let (url, handle) = mock_seq(vec![
        Behavior::Status(502),
        Behavior::Status(503),
        Behavior::Result(serde_json::json!(42)),
    ]);
    let chain = RpcChain::connect_with_timeout(&url, SHORT)
        .expect("connect")
        .with_read_retry(NO_WAIT);
    assert_eq!(chain.last_block_id().expect("height after retries"), 42);
    drop(chain);
    let seen = handle.join().expect("mock");
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|r| r["method"] == "getLastBlockId"));
}

#[test]
fn a_read_that_keeps_failing_reports_the_attempts() {
    let (url, handle) = mock_seq((0..5).map(|_| Behavior::Status(502)).collect());
    let chain = RpcChain::connect_with_timeout(&url, SHORT)
        .expect("connect")
        .with_read_retry(NO_WAIT);
    match chain.account(&id(1)) {
        Err(SdkError::Chain(m)) => assert!(m.contains("5 attempts"), "{m}"),
        other => panic!("expected Chain, got {other:?}"),
    }
    drop(chain);
    assert_eq!(handle.join().expect("mock").len(), 5);
}

#[test]
fn send_is_never_retried() {
    let (url, handle) = mock_seq(vec![
        Behavior::Status(502),
        Behavior::Result(serde_json::json!(hex::encode([9u8; 32]))),
    ]);
    let chain = RpcChain::connect_with_timeout(&url, SHORT)
        .expect("connect")
        .with_read_retry(NO_WAIT);
    assert!(matches!(chain.send(&tx()), Err(SdkError::SendUncertain(_))));
    drop(chain);
    let seen = handle.join().expect("mock");
    assert_eq!(seen.len(), 1, "one sendTransaction, never a second");
    assert_eq!(seen[0]["method"], "sendTransaction");
}
