use cosmwasm_std::{OverflowError, StdError};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Overflow(#[from] OverflowError),

    #[error("Numerical")]
    Numerical {},

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("No funds available")]
    NoFundsAvailable {},

    #[error("User not found")]
    UserNotFound {},

    #[error("No rewards available")]
    NoRewardsAvailable {},

    #[error("Insufficient funds")]
    InsufficientFunds {},

    #[error("Invalid state")]
    InvalidState {},

    #[error("No stake record available")]
    NoRecordAvailable {},

    #[error("Couldn't read contract config")]
    CouldNotReadConfig {},

    #[error("Couldn't read contract state")]
    CouldNotReadState {},

    #[error("No unbonded stake, you have to unbond and wait for the unbonding period before you can remove stake")]
    BondedStake {},

    #[error("Cannot unbond 0 nanomobx")]
    ZeroAmountUnbond {},

    #[error("The contract is paused")]
    ContractPaused {},

    #[error("Not enough expired stake to remove")]
    NotEnoughExpiredStakeToRemove {},
}
