use crate::state::Config;
use cosmwasm_std::{Addr, Uint128, Uint64};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub denom: String,
    // reward denom is always same as denom
    pub reward_rate: Uint128,
    // nanomobx per second
    pub paused: bool,
    pub unbonding_period: Uint64, // in seconds
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddStake {},
    Unbond { amount: Uint128 },
    RemoveStake {},
    ClaimRewards {},
    UpdateConfig { config: Config },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    // GetCount returns the current count as a json-encoded number
    QueryStake { address: Addr },
    QueryRewards { address: Addr },
    QueryUnbondEntry { address: Addr },
    QueryConfig {},
    QueryState {},
    QueryStakers {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MigrateMsg {
    Migrate {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UnbondResponse {
    pub unbound_amount: Uint128,
    pub expiration_timestamp: Uint64,
    // unix timestamp when it expires
    pub is_valid: bool, // whether it was used, this allows for 1:1 mapping between Users and UnbondEntries
    pub expired: bool,
}
