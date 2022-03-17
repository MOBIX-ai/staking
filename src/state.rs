use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Timestamp, Uint128, Uint64};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct UserEntry {
    pub amount: Uint128,
    pub rewards: Uint128,
    pub user_reward_per_token_paid: Uint128,
}

pub const USERS: Map<&Addr, UserEntry> = Map::new("stakes");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub chief_pausing_officer: Addr,
    pub denom: String,
    // reward denom is always same as denom
    pub reward_rate: Uint128,
    // nanomobx per second
    pub paused: bool,
    pub unbonding_period: Uint64, // in seconds
}

pub const CONFIG: Item<Config> = Item::new("config");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UnbondEntry {
    pub unbound_amount: Uint128,
    pub expiration_timestamp: Uint64,
    // unix timestamp when it expires
    pub is_valid: bool, // whether it was used, this allows for 1:1 mapping between Users and UnbondEntries
}

pub const UNBOND_ENTRIES: Map<&Addr, UnbondEntry> = Map::new("unbond_entries");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub reward_per_token_stored: Uint128,
    pub last_update_time: Timestamp,
    pub staked_balance: Uint128,
}

pub const STATE: Item<State> = Item::new("state");
