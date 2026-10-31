//! `Chain` over the LEZ v0.2.4 sequencer JSON-RPC (`sequencer_service_rpc`, feature `rpc`). Each
//! `RpcChain` owns a current-thread tokio runtime and blocks on every call (ADR-040), so it must not
//! be used from inside another tokio runtime.
//!
//! Everything the node returns is untrusted input: decode failures are typed errors, response sizes
//! and block ranges are bounded, and block ids and result lengths are checked against the request.
//! Every request has an overall timeout (the HTTP client has no separate connect timeout, so it also
//! bounds the connect). Idempotent reads are retried a bounded number of times with backoff on a
//! transient failure (transport error, timeout, unparsable reply: [`ReadRetry`]); a definite
//! JSON-RPC error is not retried, and `send` is never retried (a resend could act twice). An
//! optional deadline ([`RpcChain::with_deadline`]) caps each read's timeout and retry pause at the
//! time left: no read starts, waits or runs past it.
//!
//! `send` error mapping (see `docs/architecture/sdk.md`): only a definite rejection or a request that
//! provably never left is a plain `SdkError::Chain`; every other failure is `SdkError::SendUncertain`.

use std::{
    error::Error as StdError,
    future::Future,
    io,
    time::{Duration, Instant},
};

use common::{block::Block, transaction::LeeTransaction};
use lee::PublicTransaction;
use lee_core::account::{Account, AccountId, Nonce};
use sequencer_service_rpc::{ClientError, RpcClient as _, SequencerClient, SequencerClientBuilder};

use crate::{
    chain::{BlockSummary, Chain, TxSummary},
    error::{SdkError, SdkResult},
};

/// Overall per-request timeout (covers connect, write and read).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Most blocks one `blocks` call may ask for.
pub const MAX_BLOCK_RANGE: u64 = 256;
/// Largest response the client accepts. A `getBlockRange` reply is `batch` blocks of up to the
/// sequencer's `max_block_size` (1 MiB on LEZ v0.2.4), so `scan` batch x block size must stay under
/// this on a busy chain: the default batch of 100 can exceed it, and a response-size error is then
/// reported as a `Chain` error; lower the scan batch (the CLI's `--batch`) to recover.
const MAX_RESPONSE_BYTES: u32 = 16 * 1024 * 1024;
/// Largest JSON request body the client writes.
const MAX_REQUEST_BYTES: u32 = 2 * 1024 * 1024;
/// Largest borsh transaction `send` accepts (the JSON form is base64, about 4/3 larger).
const MAX_TX_BYTES: usize = 1024 * 1024;
/// JSON-RPC `InvalidParams`: what the sequencer answers when it refuses a transaction before the mempool.
const JSONRPC_INVALID_PARAMS: i32 = -32602;
/// Longest server-provided text copied into an error message.
const MAX_MESSAGE_CHARS: usize = 200;
/// Why a read stopped at the deadline (`with_deadline`).
const OUT_OF_TIME: &str = "the call's time budget ran out before the node answered";

/// Bounded retry of idempotent reads: `attempts` tries in all, the pause before retry `n` being
/// `first_delay * 2^(n-1)`. Never applied to `send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadRetry {
    pub attempts: u32,
    pub first_delay: Duration,
}

impl ReadRetry {
    /// One try, no retry.
    pub const NONE: Self = Self {
        attempts: 1,
        first_delay: Duration::ZERO,
    };
    /// Five tries, waiting 2, 4, 8 and 16 s between them (30 s in all): long enough for a proxy's
    /// brief 502 or a DNS hiccup, short against the confirmation window.
    pub const DEFAULT: Self = Self {
        attempts: 5,
        first_delay: Duration::from_secs(2),
    };

    /// The pause before retry `retry` (1 for the first retry).
    #[must_use]
    pub fn delay_before(&self, retry: u32) -> Duration {
        self.first_delay.saturating_mul(
            1u32.checked_shl(retry.saturating_sub(1))
                .unwrap_or(u32::MAX),
        )
    }
}

/// Runs `f` until it succeeds, fails with an error `is_transient` rejects, or `policy.attempts`
/// tries are used up; returns the last result. Only for idempotent reads.
pub fn retry_read<T, E>(
    policy: &ReadRetry,
    is_transient: impl Fn(&E) -> bool,
    sleep: &dyn Fn(Duration),
    mut f: impl FnMut() -> Result<T, E>,
) -> Result<T, E> {
    let mut tried = 1;
    loop {
        match f() {
            Err(e) if tried < policy.attempts.max(1) && is_transient(&e) => {
                sleep(policy.delay_before(tried));
                tried += 1;
            }
            other => return other,
        }
    }
}

/// A failure a later identical read may not repeat: the connection, the proxy (HTTP 5xx), a
/// timeout, or a reply that did not parse. A JSON-RPC error object from the node is definite.
fn transient(e: &ClientError) -> bool {
    matches!(
        e,
        ClientError::Transport(_)
            | ClientError::RequestTimeout
            | ClientError::RestartNeeded(_)
            | ClientError::ParseError(_)
    )
}

pub struct RpcChain {
    client: SequencerClient,
    rt: tokio::runtime::Runtime,
    url: String,
    retry: ReadRetry,
    deadline: Option<Instant>,
}

impl std::fmt::Debug for RpcChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcChain").field("url", &self.url).finish()
    }
}

impl RpcChain {
    /// Connects with the default 30 s request timeout and [`ReadRetry::DEFAULT`].
    pub fn connect(url: &str) -> SdkResult<Self> {
        Self::connect_with_timeout(url, DEFAULT_TIMEOUT)
    }

    pub fn connect_with_timeout(url: &str, timeout: Duration) -> SdkResult<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| SdkError::Chain(format!("tokio runtime: {e}")))?;
        let client = {
            let _guard = rt.enter();
            SequencerClientBuilder::default()
                .request_timeout(timeout)
                .max_request_size(MAX_REQUEST_BYTES)
                .max_response_size(MAX_RESPONSE_BYTES)
                .build(url)
                .map_err(|e| SdkError::Chain(format!("{url}: {e}")))?
        };
        Ok(Self {
            client,
            rt,
            url: url.to_owned(),
            retry: ReadRetry::DEFAULT,
            deadline: None,
        })
    }

    /// Replaces the read retry policy (default [`ReadRetry::DEFAULT`]).
    #[must_use]
    pub fn with_read_retry(self, retry: ReadRetry) -> Self {
        Self { retry, ..self }
    }

    /// Sets the deadline of every later read: each try's timeout is capped at the time left, a retry
    /// whose pause would end at or after it is not made, and no read starts once it has passed.
    /// `send` only checks that the deadline has not passed (it is never retried).
    #[must_use]
    pub fn with_deadline(self, deadline: Instant) -> Self {
        Self {
            deadline: Some(deadline),
            ..self
        }
    }

    /// The time left before the deadline (`None`: no deadline).
    fn time_left(&self) -> Option<Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
    }

    fn out_of_time(&self) -> bool {
        self.time_left() == Some(Duration::ZERO)
    }

    /// One idempotent read, retried per `self.retry` within the deadline. `make` builds a fresh
    /// request each try.
    fn call<T, F, Fut>(&self, what: &str, make: F) -> SdkResult<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, ClientError>>,
    {
        ensure_no_runtime()?;
        if self.out_of_time() {
            return Err(SdkError::Chain(format!(
                "{what} at {}: {OUT_OF_TIME}",
                self.url
            )));
        }
        let tries = std::cell::Cell::new(0u32);
        // A retry is made only if its pause ends before the deadline.
        let retry_fits = |e: &ClientError| {
            transient(e)
                && self
                    .time_left()
                    .is_none_or(|left| self.retry.delay_before(tries.get()) < left)
        };
        retry_read(&self.retry, retry_fits, &std::thread::sleep, || {
            tries.set(tries.get() + 1);
            match self.time_left() {
                None => self.rt.block_on(make()),
                Some(Duration::ZERO) => Err(ClientError::RequestTimeout),
                Some(left) => self.rt.block_on(async {
                    tokio::time::timeout(left, make())
                        .await
                        .unwrap_or(Err(ClientError::RequestTimeout))
                }),
            }
        })
        .map_err(|e| {
            let after = if tries.get() > 1 {
                format!(" (after {} attempts)", tries.get())
            } else {
                String::new()
            };
            let cut = if self.out_of_time() {
                format!(" ({OUT_OF_TIME})")
            } else {
                String::new()
            };
            SdkError::Chain(format!(
                "{what} at {}: {}{after}{cut}",
                self.url,
                truncated(&e.to_string())
            ))
        })
    }
}

/// `block_on` panics inside another tokio runtime; refuse instead.
fn ensure_no_runtime() -> SdkResult<()> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(SdkError::Chain(
            "RpcChain used inside a tokio runtime".into(),
        ));
    }
    Ok(())
}

fn truncated(s: &str) -> String {
    s.chars().take(MAX_MESSAGE_CHARS).collect()
}

/// The public transactions of a block (privacy-preserving and deployment transactions are skipped).
#[must_use]
pub fn summarize(b: &Block) -> BlockSummary {
    BlockSummary {
        id: b.header.block_id,
        hash: b.header.hash.0,
        prev_hash: b.header.prev_block_hash.0,
        timestamp_ms: b.header.timestamp,
        txs: b
            .body
            .transactions
            .iter()
            .filter_map(|t| match t {
                LeeTransaction::Public(p) => Some(TxSummary {
                    hash: p.hash(),
                    program_id: p.message.program_id,
                    account_ids: p.message.account_ids.clone(),
                    instruction_words: p.message.instruction_data.clone(),
                }),
                LeeTransaction::PrivacyPreserving(_) | LeeTransaction::ProgramDeployment(_) => None,
            })
            .collect(),
    }
}

/// True when some error in the source chain proves the request never reached the server: the TCP
/// connect was refused, or the host name did not resolve. Anything else (including a timeout, which
/// cannot say whether the request was written) is not proof.
fn never_left(e: &(dyn StdError + 'static)) -> bool {
    let mut cur: Option<&(dyn StdError + 'static)> = Some(e);
    while let Some(err) = cur {
        if let Some(io_err) = err.downcast_ref::<io::Error>() {
            if io_err.kind() == io::ErrorKind::ConnectionRefused {
                return true;
            }
        }
        if err.to_string().contains("dns error") {
            return true;
        }
        cur = err.source();
    }
    false
}

/// The `send` error table. `Chain` = definitely not accepted; `SendUncertain` = it may have been.
fn classify_send_error(url: &str, e: &ClientError) -> SdkError {
    match e {
        ClientError::Call(obj) if obj.code() == JSONRPC_INVALID_PARAMS => SdkError::Chain(format!(
            "sendTransaction rejected by {url}: {}",
            truncated(obj.message())
        )),
        ClientError::Transport(inner) if never_left(&**inner) => SdkError::Chain(format!(
            "sendTransaction not delivered to {url}: {}",
            truncated(&e.to_string())
        )),
        _ => SdkError::SendUncertain(format!("{url}: {}", truncated(&e.to_string()))),
    }
}

impl Chain for RpcChain {
    fn account(&self, id: &AccountId) -> SdkResult<Account> {
        self.call("getAccount", || self.client.get_account(*id))
    }

    fn nonces(&self, ids: &[AccountId]) -> SdkResult<Vec<Nonce>> {
        let nonces = self.call("getAccountsNonces", || {
            self.client.get_accounts_nonces(ids.to_vec())
        })?;
        if nonces.len() != ids.len() {
            return Err(SdkError::Chain(format!(
                "getAccountsNonces at {}: asked for {} nonces, got {}",
                self.url,
                ids.len(),
                nonces.len()
            )));
        }
        Ok(nonces)
    }

    fn last_block_id(&self) -> SdkResult<u64> {
        self.call("getLastBlockId", || self.client.get_last_block_id())
    }

    fn blocks(&self, from: u64, to: u64) -> SdkResult<Vec<BlockSummary>> {
        if from > to || to - from >= MAX_BLOCK_RANGE {
            return Err(SdkError::Chain(format!(
                "getBlockRange: range {from}..={to} is empty or wider than {MAX_BLOCK_RANGE} blocks"
            )));
        }
        let blocks = self.call("getBlockRange", || self.client.get_block_range(from, to))?;
        let expected = to - from + 1;
        if u64::try_from(blocks.len()).ok() != Some(expected) {
            return Err(SdkError::Chain(format!(
                "getBlockRange at {}: asked for {expected} blocks, got {}",
                self.url,
                blocks.len()
            )));
        }
        let summaries: Vec<BlockSummary> = blocks.iter().map(summarize).collect();
        for (offset, s) in (0u64..).zip(&summaries) {
            if s.id != from + offset {
                return Err(SdkError::Chain(format!(
                    "getBlockRange at {}: expected block {}, got {}",
                    self.url,
                    from + offset,
                    s.id
                )));
            }
        }
        Ok(summaries)
    }

    fn send(&self, tx: &PublicTransaction) -> SdkResult<[u8; 32]> {
        // Pre-send, so a definite error is correct.
        ensure_no_runtime()?;
        if self.out_of_time() {
            return Err(SdkError::Chain(format!(
                "sendTransaction at {}: {OUT_OF_TIME}",
                self.url
            )));
        }
        let wire = LeeTransaction::Public(tx.clone());
        let size = borsh::to_vec(&wire)
            .map_err(|e| SdkError::Encode(format!("transaction serialisation: {e}")))?
            .len();
        if size > MAX_TX_BYTES {
            // Refused locally, before any byte is written.
            return Err(SdkError::Chain(format!(
                "transaction is {size} bytes, over the {MAX_TX_BYTES} byte client limit"
            )));
        }
        match self.rt.block_on(self.client.send_transaction(wire)) {
            Ok(hash) => Ok(hash.0),
            Err(e) => Err(classify_send_error(&self.url, &e)),
        }
    }
}

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod tests;
