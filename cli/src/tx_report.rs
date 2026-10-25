//! What a transaction command prints after `Included`: the accounts it touched, re-read from the chain.
//! Inclusion means a transaction of this signer applied at the sent nonce, not necessarily this very
//! hash (ADR-042), so the figures come from the chain and are never computed from the quote.

use std::io::Write;

use lee_core::{account::AccountId, program::ProgramId};

use auction_sdk::{
    chain::Chain, decode::Reader, error::SdkResult, ops::Touch, verify::VerifiedConfig,
};

use crate::{
    env::Env,
    error::CliError,
    output::{kv, yes_no},
};

/// Prints the applied result of every touched account. A failed read is reported, never fatal: the
/// transaction is already included.
pub fn print_applied<C: Chain>(
    env: &Env<C>,
    v: &VerifiedConfig,
    touched: &[Touch],
    out: &mut dyn Write,
) -> Result<(), CliError> {
    if touched.is_empty() {
        return Ok(());
    }
    kv(
        out,
        "note",
        "the figures below are read from the chain after inclusion (ADR-042)",
    )?;
    let r = Reader::new(&env.chain, v.engine.program);
    for t in touched {
        let (key, value) = match t {
            Touch::Holding {
                label,
                id,
                definition,
            } => (
                format!("after {label}"),
                describe(holding(env, &r, v, id, definition)),
            ),
            Touch::Auction(id) => ("after auction".to_owned(), describe(auction(&r, id))),
            Touch::Accounting => ("after accounting".to_owned(), describe(accounting(&r, v))),
        };
        kv(out, &key, &value)?;
    }
    Ok(())
}

fn describe(r: SdkResult<String>) -> String {
    r.unwrap_or_else(|e| format!("unreadable: {e}"))
}

fn holding<C: Chain>(
    env: &Env<C>,
    r: &Reader<'_, C>,
    v: &VerifiedConfig,
    id: &AccountId,
    definition: &AccountId,
) -> SdkResult<String> {
    // A never-initialised recipient (it only signed) has no token owner yet.
    if env.chain.account(id)?.program_owner == ProgramId::default() {
        return Ok("uninitialised".to_owned());
    }
    Ok(
        match r.balance(id, &v.state.token_program_id, definition)? {
            Some(b) => format!("balance {b}"),
            None => "uninitialised".to_owned(),
        },
    )
}

fn auction<C: Chain>(r: &Reader<'_, C>, id: &AccountId) -> SdkResult<String> {
    let st = r.auction(id)?;
    let remaining = st.to_core()?.remaining_target();
    Ok(format!(
        "settled {}, collateral left {}, remaining target {remaining}",
        yes_no(st.settled),
        st.collateral_left
    ))
}

/// One line of the engine accounting; shared by `config` and the post-inclusion report.
pub(crate) fn accounting<C: Chain>(r: &Reader<'_, C>, v: &VerifiedConfig) -> SdkResult<String> {
    let a = r.accounting(&v.engine.accounting)?;
    Ok(format!(
        "unbacked debt {}, debt on auction {}, surplus live {} (next nonce {}), debt live {} (next nonce {})",
        a.unbacked_debt,
        a.debt_on_auction,
        yes_no(a.surplus_live),
        a.surplus_nonce,
        yes_no(a.debt_live),
        a.debt_nonce
    ))
}

/// Lot left of the latest surplus and debt auctions that are live, for `config`.
pub(crate) fn system_lots<C: Chain>(
    r: &Reader<'_, C>,
    v: &VerifiedConfig,
) -> SdkResult<Vec<(&'static str, String)>> {
    let a = r.accounting(&v.engine.accounting)?;
    let mut lots = Vec::new();
    if let (true, Some(n)) = (a.surplus_live, a.surplus_nonce.checked_sub(1)) {
        let s = r.surplus_auction(&v.engine.surplus_auction(n))?;
        lots.push(("surplus lot left", s.lot_left.to_string()));
    }
    if let (true, Some(n)) = (a.debt_live, a.debt_nonce.checked_sub(1)) {
        let d = r.debt_auction(&v.engine.debt_auction(n))?;
        lots.push(("debt lot left", d.lot_left.to_string()));
    }
    Ok(lots)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use auction_sdk::{
        confirm::ConfirmSettings,
        testing::{
            id,
            standard::{self, k},
        },
    };

    use super::*;

    #[test]
    fn the_report_reads_the_chain_each_time_it_is_printed() {
        let fx = standard::standard();
        let dir = tempfile::tempdir().expect("tmp");
        let env = Env {
            chain: &fx.chain,
            deployment: fx.deployment.clone(),
            config: None,
            wallet_home: None,
            state_dir: dir.path().to_path_buf(),
            at_ms: None,
            confirm: ConfirmSettings {
                poll_interval: Duration::ZERO,
                drop_after_blocks: 5,
                max_polls: 1,
            },
            sleep: |_| {},
        };
        let v = env.verified().expect("verifies");
        let holding = id(k::BIDDER_STABLE);
        let touch = [Touch::Holding {
            label: "bidder".into(),
            id: holding,
            definition: v.state.stablecoin_definition_id,
        }];
        let print = || {
            let mut out = Vec::new();
            print_applied(&env, &v, &touch, &mut out).expect("prints");
            String::from_utf8(out).expect("utf-8")
        };
        let before = print();
        fx.chain.put_holding(
            holding,
            v.state.token_program_id,
            v.state.stablecoin_definition_id,
            424_242,
        );
        let after = print();
        assert!(after.contains("balance 424242"), "{after}");
        assert_ne!(before, after);
    }
}
