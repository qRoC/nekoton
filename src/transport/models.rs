use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use ton_block::{AccountStuff, Transaction};
use ton_types::UInt256;

use nekoton_abi::{ExecutionContext, GenTimings, LastTransactionId, TransactionId};
use nekoton_utils::{serde_account_stuff, Clock};

use crate::core::models::{ContractState, PendingTransaction};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RawContractState {
    NotExists,
    Exists(ExistingContract),
}

impl RawContractState {
    pub fn brief(&self) -> ContractState {
        match self {
            Self::NotExists => ContractState::default(),
            Self::Exists(state) => state.brief(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExistingContract {
    #[serde(with = "serde_account_stuff")]
    pub account: AccountStuff,
    pub timings: GenTimings,
    pub last_transaction_id: LastTransactionId,
}

impl ExistingContract {
    pub fn brief(&self) -> ContractState {
        ContractState {
            balance: self.account.storage.balance.grams.0 as u64,
            gen_timings: self.timings,
            last_transaction_id: Some(self.last_transaction_id),
            is_deployed: matches!(
                self.account.storage.state,
                ton_block::AccountState::AccountActive { .. }
            ),
            code_hash: match &self.account.storage.state {
                ton_block::AccountState::AccountActive { state_init, .. } => {
                    state_init.code.as_ref().map(ton_types::Cell::repr_hash)
                }
                _ => None,
            },
        }
    }

    pub fn as_context<'a>(&'a self, clock: &'a dyn Clock) -> ExecutionContext<'a> {
        ExecutionContext {
            clock,
            account_stuff: &self.account,
        }
    }
}

impl PartialEq for ExistingContract {
    fn eq(&self, other: &Self) -> bool {
        self.account
            .storage
            .last_trans_lt
            .eq(&other.account.storage.last_trans_lt)
    }
}

#[derive(Clone, Debug)]
pub struct RawTransaction {
    pub hash: UInt256,
    pub data: Transaction,
}

impl PartialEq for RawTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.data.lt == other.data.lt && self.hash == other.hash
    }
}

impl Eq for RawTransaction {}

impl PartialOrd for RawTransaction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.data.lt.partial_cmp(&other.data.lt)
    }
}

impl Ord for RawTransaction {
    fn cmp(&self, other: &Self) -> Ordering {
        self.data.lt.cmp(&other.data.lt)
    }
}

impl RawTransaction {
    pub fn id(&self) -> TransactionId {
        TransactionId {
            lt: self.data.lt,
            hash: self.hash,
        }
    }
}

impl PartialEq<RawTransaction> for PendingTransaction {
    fn eq(&self, other: &RawTransaction) -> bool {
        if other.data.now >= self.expire_at {
            return false;
        }

        matches!(
            other.data.in_msg.as_ref().map(|msg| msg.cell().repr_hash()),
            Some(message_hash) if self.message_hash == message_hash
        )
    }
}
