//! Unified async-completion message for the event loop.
//!
//! All background work (wallet jobs, balance/identity/price fetches, NNS lookups) reports back
//! through a **single** `mpsc::UnboundedSender<Msg>` instead of one channel per result type. The
//! event loop has one `msg_rx` arm that routes each variant to its reducer.

use crate::command_runner::{
    BalanceRefreshCompletion, HomeIdentityCompletion, JobCompletion, MasterAddressesCompletion,
    NnsLookupCompletion, OwnedNnsNamesCompletion, SendSimplePlanCompletion,
};

#[derive(Debug)]
pub(crate) enum Msg {
    /// A scheduled wallet command (over the HTTP API) finished.
    Job(JobCompletion),
    /// Background balance-sidebar refresh finished.
    Balance(BalanceRefreshCompletion),
    /// Simple-send planner preview finished.
    Plan(SendSimplePlanCompletion),
    /// NNS name-availability lookup finished.
    NnsLookup(NnsLookupCompletion),
    /// Owned `.nock` names for the active address loaded.
    OwnedNnsNames(OwnedNnsNamesCompletion),
    /// Home identity (active address + optional `.nock` name) resolved.
    Identity(HomeIdentityCompletion),
    /// Master addresses for the home wallet picker loaded.
    MasterAddresses(MasterAddressesCompletion),
    /// CoinGecko price fetch finished.
    Price(Result<f64, String>),
}
