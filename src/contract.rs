use cosmwasm_std::{
    attr, entry_point, to_binary, Addr, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Response, StdResult, Uint128, Uint64,
};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, UnbondResponse};
use crate::state::{Config, State, UnbondEntry, UserEntry, CONFIG, STATE, UNBOND_ENTRIES, USERS};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config: Config = Config {
        owner: info.sender.clone(),
        chief_pausing_officer: info.sender, // the owner can change it later
        denom: msg.denom,
        reward_rate: msg.reward_rate,
        paused: msg.paused,
        unbonding_period: msg.unbonding_period,
    };

    CONFIG.save(deps.storage, &config)?;

    let state: State = State {
        reward_per_token_stored: Uint128::zero(),
        last_update_time: env.block.time,
        staked_balance: Uint128::zero(),
    };

    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

#[entry_point]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Default::default())
}

// And declare a custom Error variant for the ones where you will want to make use of it
#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddStake {} => try_add_stake(deps, env, info),
        ExecuteMsg::Unbond { amount } => try_unbond(deps, env, info, amount),
        ExecuteMsg::RemoveStake {} => try_remove_stake(deps, env, info),
        ExecuteMsg::ClaimRewards {} => try_claim(deps, env, info),
        ExecuteMsg::UpdateConfig { config } => try_update_config(deps, info, config),
    }
}

pub fn try_add_stake(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    if config.paused {
        return Err(ContractError::ContractPaused {});
    }

    let funds = info
        .funds
        .iter()
        .find(|c| c.denom == config.denom)
        .ok_or(ContractError::NoFundsAvailable {})?;

    if funds.amount.is_zero() {
        return Err(ContractError::NoFundsAvailable {});
    }

    update_rewards(&mut deps, &env, funds.amount, true)?;

    let state: State = STATE.load(deps.storage)?;

    USERS.update::<_, ContractError>(deps.storage, &info.sender, |record| {
        // get current state, if there isn't one, get the default state
        let prev_user_state: UserEntry = record.unwrap_or(UserEntry {
            amount: Uint128::zero(),
            rewards: Uint128::zero(),
            user_reward_per_token_paid: Uint128::zero(),
        });

        // add the new entry into the record
        let current_user_state: UserEntry = UserEntry {
            amount: prev_user_state.amount.checked_add(funds.amount)?,
            rewards: earned(&prev_user_state, &state, &config, &env)?,
            user_reward_per_token_paid: state.reward_per_token_stored,
        };

        Ok(current_user_state)
    })?;

    Ok(Response::default().add_attribute("action", "stake"))
}

fn update_rewards(
    deps: &mut DepsMut,
    env: &Env,
    stake_amount: Uint128,
    is_addition: bool,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let prev_state: State = STATE.load(deps.storage)?;
    let mut new_staked_balance: Uint128 = prev_state.staked_balance;

    if is_addition {
        new_staked_balance = new_staked_balance.checked_add(stake_amount)?;
    } else {
        new_staked_balance = new_staked_balance.checked_sub(stake_amount)?;
    }

    let current_state: State = State {
        reward_per_token_stored: reward_per_token(&prev_state, &config, env)?,
        last_update_time: env.block.time,
        staked_balance: new_staked_balance,
    };

    STATE.save(deps.storage, &current_state)?;

    Ok(Response::default())
}

fn reward_per_token(state: &State, config: &Config, env: &Env) -> Result<Uint128, ContractError> {
    if state.staked_balance.is_zero() {
        return Ok(state.reward_per_token_stored);
    }

    let current_time: Uint128 = Uint128::from(env.block.time.nanos());
    let prev_update_time: Uint128 = Uint128::from(state.last_update_time.nanos());

    let delta_time_in_ns: Uint128 = match current_time.checked_sub(prev_update_time) {
        Ok(res) => res,
        Err(_) => return Err(ContractError::Numerical {}),
    };
    let billion: Uint128 = Uint128::from(10u64.pow(9) as u64);

    let delta_time: Uint128 = match delta_time_in_ns.checked_div(billion) {
        Ok(res) => res,
        Err(_) => return Err(ContractError::Numerical {}),
    }; // in seconds

    let rewards_per_time: Uint128 = delta_time.checked_mul(config.reward_rate)?;
    let inflated_rewards_per_time: Uint128 = rewards_per_time.checked_mul(billion)?;
    let inflated_relative_rewards_per_time: Uint128 =
        match inflated_rewards_per_time.checked_div(state.staked_balance) {
            Ok(res) => res,
            Err(_) => return Err(ContractError::Numerical {}),
        };

    Ok(state
        .reward_per_token_stored
        .checked_add(inflated_relative_rewards_per_time)?)
}

fn earned(
    user: &UserEntry,
    state: &State,
    config: &Config,
    env: &Env,
) -> Result<Uint128, ContractError> {
    let reward_per_token: Uint128 = reward_per_token(state, config, env)?;
    let delta_reward: Uint128 = reward_per_token.checked_sub(user.user_reward_per_token_paid)?;
    let inflated_relative_delta_reward: Uint128 = user.amount.checked_mul(delta_reward)?;
    let relative_delta_reward: Uint128 =
        match inflated_relative_delta_reward.checked_div(Uint128::from(10u64.pow(9))) {
            Ok(res) => res,
            Err(_) => return Err(ContractError::Numerical {}),
        };
    let total_rewards: Uint128 = relative_delta_reward.checked_add(user.rewards)?;

    Ok(total_rewards)
}

pub fn try_unbond(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let user: UserEntry = USERS.load(deps.storage, &info.sender)?;

    if config.paused {
        return Err(ContractError::ContractPaused {});
    }

    if user.amount.is_zero() {
        return Err(ContractError::NoRecordAvailable {});
    }

    if amount.is_zero() {
        return Err(ContractError::ZeroAmountUnbond {});
    }

    if user.amount.lt(&amount) {
        return Err(ContractError::InsufficientFunds {});
    }

    update_rewards(&mut deps, &env, Uint128::zero(), false)?;

    let state: State = STATE.load(deps.storage)?;

    let user_updated: UserEntry = UserEntry {
        amount: user.amount.checked_sub(amount)?,
        user_reward_per_token_paid: state.reward_per_token_stored,
        rewards: earned(&user, &state, &config, &env)?,
    };

    USERS.update::<_, ContractError>(deps.storage, &info.sender, |_| Ok(user_updated))?;

    UNBOND_ENTRIES.update::<_, ContractError>(deps.storage, &info.sender, |prev_state| {
        let prev_unbond_entry: UnbondEntry = prev_state.unwrap_or(UnbondEntry {
            unbound_amount: Uint128::zero(),
            expiration_timestamp: Uint64::zero(),
            is_valid: false,
        });

        let billion: Uint64 = Uint64::from(10u64.pow(9) as u64);
        let current_time: Uint64 = Uint64::from(env.block.time.nanos());
        let expiration_timestamp: Uint64 =
            current_time.checked_add(config.unbonding_period.checked_mul(billion)?)?;
        let unbond_entry: UnbondEntry = UnbondEntry {
            unbound_amount: amount.checked_add(prev_unbond_entry.unbound_amount)?,
            expiration_timestamp,
            is_valid: true,
        };

        Ok(unbond_entry)
    })?;

    Ok(Response::default().add_attribute("action", "unbond"))
}

pub fn try_remove_stake(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    if config.paused {
        return Err(ContractError::ContractPaused {});
    }

    let unbond_entry: UnbondEntry =
        UNBOND_ENTRIES
            .load(deps.storage, &info.sender)
            .unwrap_or(UnbondEntry {
                unbound_amount: Uint128::zero(),
                expiration_timestamp: Uint64::zero(),
                is_valid: false,
            });
    let current_time: Uint64 = Uint64::from(env.block.time.nanos());

    if !unbond_entry.is_valid || unbond_entry.expiration_timestamp.gt(&current_time) {
        return Err(ContractError::BondedStake {});
    }

    update_rewards(&mut deps, &env, unbond_entry.unbound_amount, false)?;

    UNBOND_ENTRIES.update::<_, ContractError>(deps.storage, &info.sender, |prev_state| {
        let prev_entry = prev_state.expect("unexpected error, UserEntry should have been found!");

        let current_entry: UnbondEntry = UnbondEntry {
            unbound_amount: Uint128::zero(),
            expiration_timestamp: prev_entry.expiration_timestamp,
            is_valid: false,
        };

        Ok(current_entry)
    })?;

    let msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            denom: config.denom,
            amount: unbond_entry.unbound_amount,
        }],
    };

    let attrs = vec![attr("action", "withdraw")];

    Ok(Response::new().add_attributes(attrs).add_message(msg))
}

pub fn try_claim(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let user: UserEntry = USERS.load(deps.storage, &info.sender).unwrap_or(UserEntry {
        amount: Uint128::zero(),
        rewards: Uint128::zero(),
        user_reward_per_token_paid: Uint128::zero(),
    });

    update_rewards(&mut deps, &env, Uint128::zero(), true)?;

    let state: State = STATE.load(deps.storage)?;
    let config: Config = CONFIG.load(deps.storage)?;
    let payout_amount = earned(&user, &state, &config, &env)?;

    if user.rewards.is_zero() && payout_amount.is_zero() {
        return Err(ContractError::NoRewardsAvailable {});
    }

    let contract_balance: Coin = deps
        .querier
        .query_balance(env.contract.address, "nanomobx".to_string())
        .unwrap_or(Coin {
            amount: Uint128::zero(),
            denom: "nanomobx".to_string(),
        });
    let total_amount: Uint128 = contract_balance.amount;
    let staked_amount: Uint128 = state.staked_balance;
    let available_funds: Uint128 = total_amount
        .checked_sub(staked_amount)
        .map_err(|_| ContractError::NoFundsAvailable {})?;

    if user.rewards.gt(&available_funds) {
        return Err(ContractError::NoFundsAvailable {});
    }

    if payout_amount.gt(&available_funds) {
        return Err(ContractError::NoFundsAvailable {});
    }

    USERS.update::<_, ContractError>(deps.storage, &info.sender, |record| {
        let prev_user_state: UserEntry = record.ok_or(ContractError::InvalidState {})?;
        let new_user_state: UserEntry = UserEntry {
            amount: prev_user_state.amount,
            rewards: Uint128::zero(),
            user_reward_per_token_paid: state.reward_per_token_stored,
        };

        Ok(new_user_state)
    })?;

    let msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            denom: config.denom,
            amount: payout_amount,
        }],
    };

    let attrs = vec![attr("action", "claim")];

    Ok(Response::new().add_attributes(attrs).add_message(msg))
}

pub fn try_update_config(
    deps: DepsMut,
    info: MessageInfo,
    potential_new_config: Config,
) -> Result<Response, ContractError> {
    let old_config: Config = CONFIG.load(deps.storage)?;

    if old_config.owner == info.sender {
        // the owner can change all configs
        CONFIG.save(deps.storage, &potential_new_config)?;
    } else if old_config.chief_pausing_officer == info.sender {
        // the "pausing_officer" can only change who the pausing officer is
        // and also whether the contract is paused or not
        let new_config: Config = Config {
            owner: old_config.owner,
            chief_pausing_officer: potential_new_config.chief_pausing_officer,
            denom: old_config.denom,
            reward_rate: old_config.reward_rate,
            paused: potential_new_config.paused,
            unbonding_period: old_config.unbonding_period,
        };

        CONFIG.save(deps.storage, &new_config)?;
    } else {
        return Err(ContractError::Unauthorized {});
    }

    Ok(Response::default())
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::QueryStake { address } => to_binary(&query_stake(deps, address)?),
        QueryMsg::QueryRewards { address } => to_binary(&query_rewards(deps, address, env)?),
        QueryMsg::QueryUnbondEntry { address } => {
            to_binary(&query_unbond_entries(deps, address, env)?)
        }
        QueryMsg::QueryConfig {} => to_binary(&query_config(deps)?),
        QueryMsg::QueryState {} => to_binary(&query_state(deps)?),
    }
}

fn query_stake(deps: Deps, address: Addr) -> StdResult<Uint128> {
    let user: UserEntry = USERS.load(deps.storage, &address)?;
    let unbond: UnbondEntry = UNBOND_ENTRIES
        .load(deps.storage, &address)
        .unwrap_or(UnbondEntry {
            unbound_amount: Uint128::zero(),
            expiration_timestamp: Uint64::zero(),
            is_valid: false,
        });

    Ok(user.amount.checked_add(unbond.unbound_amount)?)
}

fn query_rewards(deps: Deps, address: Addr, env: Env) -> StdResult<Uint128> {
    let user: UserEntry = USERS.load(deps.storage, &address)?;
    let config: Config = CONFIG.load(deps.storage)?;
    let state: State = STATE.load(deps.storage)?;
    if env.block.time.nanos().gt(&state.last_update_time.nanos()) {
        let rewards = earned(&user, &state, &config, &env).unwrap_or(user.rewards);
        Ok(rewards)
    } else {
        let rewards = user.rewards;
        Ok(rewards)
    }
}

fn query_unbond_entries(deps: Deps, address: Addr, env: Env) -> StdResult<UnbondResponse> {
    let unbond_entries: UnbondEntry = UNBOND_ENTRIES.load(deps.storage, &address)?;

    Ok(UnbondResponse {
        expiration_timestamp: unbond_entries.expiration_timestamp,
        unbound_amount: unbond_entries.unbound_amount,
        is_valid: unbond_entries.is_valid,
        expired: unbond_entries
            .expiration_timestamp
            .le(&Uint64::from(env.block.time.nanos())),
    })
}

fn query_config(deps: Deps) -> StdResult<Config> {
    let config: Config = CONFIG.load(deps.storage)?;

    Ok(config)
}

fn query_state(deps: Deps) -> StdResult<State> {
    let state: State = STATE.load(deps.storage)?;

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{
        mock_dependencies, mock_dependencies_with_balance, mock_env, mock_info, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        attr, coins, from_binary, BlockInfo, ContractInfo, CosmosMsg, Timestamp, TransactionInfo,
    };

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::zero(),
            paused: false,
            unbonding_period: Uint64::zero(),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));

        let env = mock_env();

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the config
        let res = query(deps.as_ref(), env.clone(), QueryMsg::QueryConfig {}).unwrap();
        let value: Config = from_binary(&res).unwrap();
        assert_eq!(
            Config {
                owner: Addr::unchecked("creator"),
                chief_pausing_officer: Addr::unchecked("creator"),
                denom: "nanomobx".to_string(),
                reward_rate: Uint128::zero(),
                paused: false,
                unbonding_period: Uint64::zero(),
            },
            value
        );

        let res = query(deps.as_ref(), env.clone(), QueryMsg::QueryState {}).unwrap();
        let value: State = from_binary(&res).unwrap();
        assert_eq!(
            State {
                reward_per_token_stored: Uint128::zero(),
                last_update_time: env.block.time,
                staked_balance: Uint128::zero(),
            },
            value
        );
    }

    #[test]
    fn update_config() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::zero(),
            paused: false,
            unbonding_period: Uint64::zero(),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        let res = query(deps.as_ref(), env.clone(), QueryMsg::QueryConfig {}).unwrap();
        let old_config: Config = from_binary(&res).unwrap();
        assert_eq!(
            Config {
                owner: Addr::unchecked("creator"),
                chief_pausing_officer: Addr::unchecked("creator"),
                denom: "nanomobx".to_string(),
                reward_rate: Uint128::zero(),
                paused: false,
                unbonding_period: Uint64::zero(),
            },
            old_config
        );

        let new_config = Config {
            owner: old_config.clone().owner,
            chief_pausing_officer: Addr::unchecked("CPO"),
            denom: old_config.clone().denom,
            reward_rate: Uint128::from(1u128),
            paused: old_config.paused,
            unbonding_period: Uint64::from(1u64),
        };

        let update_config_msg = ExecuteMsg::UpdateConfig {
            config: new_config.clone(),
        };

        let _res = execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

        let res = query(deps.as_ref(), env.clone(), QueryMsg::QueryConfig {}).unwrap();
        let current_config: Config = from_binary(&res).unwrap();
        assert_eq!(new_config.clone(), current_config.clone());
        assert_ne!(old_config.clone(), current_config.clone());
    }

    #[test]
    fn cpo_should_only_update_cpo_and_paused() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::zero(),
            paused: false,
            unbonding_period: Uint64::zero(),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        let res = query(deps.as_ref(), env.clone(), QueryMsg::QueryConfig {}).unwrap();

        // the owner hires a new CPO
        let old_config: Config = from_binary(&res).unwrap();
        let creator_updated_config = Config {
            owner: old_config.clone().owner,
            chief_pausing_officer: Addr::unchecked("cpo"),
            denom: old_config.clone().denom,
            reward_rate: Uint128::from(1u128),
            paused: old_config.paused,
            unbonding_period: Uint64::from(1u64),
        };

        let update_config_msg = ExecuteMsg::UpdateConfig {
            config: creator_updated_config.clone(),
        };
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

        // the CPO tries to take over but fails
        let malicious_cpo_config: Config = Config {
            owner: Addr::unchecked("cpo"),
            chief_pausing_officer: Addr::unchecked("cpo2"),
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::from(1_000_000_000u128),
            paused: true,
            unbonding_period: Uint64::zero(),
        };

        let update_config_msg = ExecuteMsg::UpdateConfig {
            config: malicious_cpo_config.clone(),
        };

        let cpo_info = mock_info("cpo", &coins(0, "nanomobx"));
        let _res = execute(
            deps.as_mut(),
            env.clone(),
            cpo_info.clone(),
            update_config_msg,
        )
        .unwrap();
        let res = query(deps.as_ref(), env.clone(), QueryMsg::QueryConfig {}).unwrap();
        let current_config: Config = from_binary(&res).unwrap();

        assert_ne!(malicious_cpo_config.clone(), current_config.clone());
        assert_eq!(malicious_cpo_config.paused, current_config.paused);
        assert_eq!(
            malicious_cpo_config.chief_pausing_officer,
            current_config.chief_pausing_officer
        );
        assert_eq!(
            creator_updated_config.unbonding_period,
            current_config.unbonding_period
        );
        assert_eq!(
            creator_updated_config.reward_rate,
            current_config.reward_rate
        );
    }

    #[test]
    fn add_stake() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::zero(),
            paused: false,
            unbonding_period: Uint64::zero(),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let info = mock_info("anyone", &coins(10, "nanomobx"));
        let add_stake_msg = ExecuteMsg::AddStake {};
        let _res = execute(deps.as_mut(), env.clone(), info, add_stake_msg).unwrap();

        let res = query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::QueryStake {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(10u128), value);
    }

    #[test]
    fn unbond_and_remove_stake() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::zero(),
            paused: false,
            unbonding_period: Uint64::zero(),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let info = mock_info("anyone", &coins(10, "nanomobx"));
        let add_stake_msg = ExecuteMsg::AddStake {};
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), add_stake_msg).unwrap();

        let mut new_env = mock_env();
        new_env.block.height += 3;

        let unbond_msg = ExecuteMsg::Unbond {
            amount: Uint128::from(10 as u128),
        };
        let _res = execute(deps.as_mut(), new_env.clone(), info.clone(), unbond_msg);

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryUnbondEntry {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: UnbondResponse = from_binary(&res).unwrap();

        assert_eq!(true, value.is_valid);
        assert_eq!(
            Uint64::from(new_env.block.time.nanos()),
            value.expiration_timestamp
        );
        assert_eq!(true, value.expired);

        let remove_stake_msg = ExecuteMsg::RemoveStake {};
        let _res = execute(
            deps.as_mut(),
            new_env.clone(),
            info.clone(),
            remove_stake_msg,
        )
        .unwrap();

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryUnbondEntry {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: UnbondResponse = from_binary(&res).unwrap();

        assert_eq!(false, value.is_valid);
        assert_eq!(
            Uint64::from(new_env.block.time.nanos()),
            value.expiration_timestamp
        );

        assert_eq!(true, value.expired);

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryStake {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(0u128), value);
    }

    #[test]
    fn unbond_twice() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::zero(),
            paused: false,
            unbonding_period: Uint64::zero(),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let info = mock_info("anyone", &coins(10, "nanomobx"));
        let add_stake_msg = ExecuteMsg::AddStake {};
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), add_stake_msg).unwrap();

        let mut new_env = mock_env();
        // new_env.block.height += 3;
        new_env.block.time = Timestamp::from_nanos(env.block.time.nanos() + 3 * 1_000_000_000);

        let unbond_msg = ExecuteMsg::Unbond {
            amount: Uint128::from(10u128),
        };
        let _res = execute(deps.as_mut(), new_env.clone(), info.clone(), unbond_msg);

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryUnbondEntry {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: UnbondResponse = from_binary(&res).unwrap();

        assert_eq!(true, value.is_valid);
        assert_eq!(
            Uint64::from(new_env.block.time.nanos()),
            value.expiration_timestamp
        );
        assert_eq!(true, value.expired);

        let remove_stake_msg = ExecuteMsg::RemoveStake {};
        let _res = execute(
            deps.as_mut(),
            new_env.clone(),
            info.clone(),
            remove_stake_msg,
        )
        .unwrap();

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryUnbondEntry {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: UnbondEntry = from_binary(&res).unwrap();

        assert_eq!(false, value.is_valid);
        assert_eq!(
            Uint64::from(new_env.block.time.nanos()),
            value.expiration_timestamp
        );

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryStake {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(0u128), value);

        let second_unbond_msg = ExecuteMsg::Unbond {
            amount: Uint128::from(10u128),
        };
        let err = execute(
            deps.as_mut(),
            new_env.clone(),
            info.clone(),
            second_unbond_msg,
        )
        .unwrap_err();

        match err {
            ContractError::NoRecordAvailable {} => {}
            e => panic!("unexpected error: {}", e),
        }
    }

    #[test]
    fn unbond_period() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::zero(),
            paused: false,
            unbonding_period: Uint64::from(300u64),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let info = mock_info("anyone", &coins(10, "nanomobx"));
        let add_stake_msg = ExecuteMsg::AddStake {};
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), add_stake_msg).unwrap();

        let mut new_env = mock_env();
        new_env.block.height += 3;

        let unbond_msg = ExecuteMsg::Unbond {
            amount: Uint128::from(10 as u128),
        };
        let _res = execute(deps.as_mut(), new_env.clone(), info.clone(), unbond_msg);

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryUnbondEntry {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: UnbondResponse = from_binary(&res).unwrap();

        let billion: Uint64 = Uint64::from(10u64.pow(9) as u64);
        let current_time: Uint64 = Uint64::from(env.block.time.nanos());
        let expiration_timestamp: Uint64 = current_time
            .checked_add(Uint64::from(300u64).checked_mul(billion).unwrap())
            .unwrap();
        assert_eq!(true, value.is_valid);
        assert_eq!(expiration_timestamp, value.expiration_timestamp);
        assert_eq!(false, value.expired);

        let remove_stake_msg = ExecuteMsg::RemoveStake {};
        let err = execute(
            deps.as_mut(),
            new_env.clone(),
            info.clone(),
            remove_stake_msg,
        )
        .unwrap_err();

        match err {
            ContractError::BondedStake {} => {}
            e => panic!("unexpecter error: {}", e),
        }

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryUnbondEntry {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: UnbondResponse = from_binary(&res).unwrap();

        assert_eq!(true, value.is_valid);
        assert_eq!(expiration_timestamp.clone(), value.expiration_timestamp);
        assert_eq!(false, value.expired);

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryStake {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(10u128), value);

        let mut newest_env = mock_env();
        // new_env.block.height += 3;
        newest_env.block.time = Timestamp::from_nanos(env.block.time.nanos() + 300 * 1_000_000_000);

        let remove_stake_msg = ExecuteMsg::RemoveStake {};
        let _res = execute(
            deps.as_mut(),
            newest_env.clone(),
            info.clone(),
            remove_stake_msg,
        )
        .unwrap();

        let res = query(
            deps.as_ref(),
            newest_env.clone(),
            QueryMsg::QueryUnbondEntry {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: UnbondResponse = from_binary(&res).unwrap();

        assert_eq!(false, value.is_valid);
        assert_eq!(expiration_timestamp.clone(), value.expiration_timestamp);
        assert_eq!(true, value.expired);

        let res = query(
            deps.as_ref(),
            newest_env.clone(),
            QueryMsg::QueryStake {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(0u128), value);
    }

    fn env_at_height(height: u64) -> Env {
        let time = Timestamp::from_seconds((5u64 * height) + 1u64);

        Env {
            block: BlockInfo {
                height,
                time,
                chain_id: Default::default(),
            },
            contract: ContractInfo {
                address: Addr::unchecked(MOCK_CONTRACT_ADDR),
            },
            transaction: { Some(TransactionInfo { index: 0 }) },
        }
    }

    #[test]
    fn check_result_claim_failure_due_to_high_reward_rate() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::from(1_000_000_000u128),
            paused: false,
            unbonding_period: Uint64::from(1u64),
        };

        // create the contract
        instantiate(
            deps.as_mut(),
            env_at_height(1),
            mock_info("creator", &coins(1000, "nanomobx")),
            msg,
        )
        .unwrap();

        // add a series of stakes
        execute(
            deps.as_mut(),
            env_at_height(2),
            mock_info("user1", &coins(10, "nanomobx")),
            ExecuteMsg::AddStake {},
        )
        .unwrap();
        execute(
            deps.as_mut(),
            env_at_height(2),
            mock_info("user2", &coins(200, "nanomobx")),
            ExecuteMsg::AddStake {},
        )
        .unwrap();
        execute(
            deps.as_mut(),
            env_at_height(2),
            mock_info("user3", &coins(20000, "nanomobx")),
            ExecuteMsg::AddStake {},
        )
        .unwrap();

        assert_eq!(
            USERS.may_load(deps.as_ref().storage, &Addr::unchecked("user1")),
            Ok(Some(UserEntry {
                amount: Uint128::from(10u128),
                rewards: Uint128::zero(),
                user_reward_per_token_paid: Uint128::zero(),
            }))
        );

        assert_eq!(
            USERS.may_load(deps.as_ref().storage, &Addr::unchecked("user2")),
            Ok(Some(UserEntry {
                amount: Uint128::from(200u128),
                rewards: Uint128::zero(),
                user_reward_per_token_paid: Uint128::zero(),
            }))
        );

        assert_eq!(
            USERS.may_load(deps.as_ref().storage, &Addr::unchecked("user3")),
            Ok(Some(UserEntry {
                amount: Uint128::from(20000u128),
                rewards: Uint128::zero(),
                user_reward_per_token_paid: Uint128::zero(),
            }))
        );

        // trigger calculation of rewards - will all fail because the reward rate is so high
        assert_eq!(
            execute(
                deps.as_mut(),
                env_at_height(12),
                mock_info("user1", &[]),
                ExecuteMsg::ClaimRewards {},
            ),
            Err(ContractError::NoFundsAvailable {})
        );
        assert_eq!(
            execute(
                deps.as_mut(),
                env_at_height(12),
                mock_info("user2", &[]),
                ExecuteMsg::ClaimRewards {},
            ),
            Err(ContractError::NoFundsAvailable {})
        );
        assert_eq!(
            execute(
                deps.as_mut(),
                env_at_height(12),
                mock_info("user3", &[]),
                ExecuteMsg::ClaimRewards {},
            ),
            Err(ContractError::NoFundsAvailable {})
        );
    }

    #[test]
    fn claim_rewards() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::from(1u128),
            paused: false,
            unbonding_period: Uint64::from(1u64),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let info = mock_info("anyone", &coins(10, "nanomobx"));
        let add_stake_msg = ExecuteMsg::AddStake {};
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), add_stake_msg).unwrap();

        let other_info = mock_info("another", &coins(10, "nanomobx"));
        let add_stake_msg = ExecuteMsg::AddStake {};
        let _res = execute(
            deps.as_mut(),
            env.clone(),
            other_info.clone(),
            add_stake_msg,
        )
        .unwrap();

        let mut new_env = mock_env();
        new_env.block.height += 4;
        new_env.block.time = Timestamp::from_nanos(env.block.time.nanos() + 4 * 1_000_000_000);

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryRewards {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: Uint128 = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(2u128), value);

        let add_stake_msg = ExecuteMsg::AddStake {};
        let _res = execute(deps.as_mut(), new_env.clone(), info.clone(), add_stake_msg).unwrap();

        let res = query(deps.as_ref(), new_env.clone(), QueryMsg::QueryState {}).unwrap();
        let value: State = from_binary(&res).unwrap();

        assert_eq!(value.last_update_time, new_env.block.time);
        assert_eq!(value.staked_balance, Uint128::from(30u128));

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryRewards {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: Uint128 = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(2u128), value);

        let claim_msg = ExecuteMsg::ClaimRewards {};
        let _res = execute(deps.as_mut(), new_env.clone(), info.clone(), claim_msg);

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryRewards {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: Uint128 = from_binary(&res).unwrap();

        assert_eq!(Uint128::zero(), value);
    }

    #[test]
    fn claim_rewards_right_after_stake() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::from(1u128),
            paused: false,
            unbonding_period: Uint64::from(1u64),
        };

        let info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let info = mock_info("anyone", &coins(10, "nanomobx"));
        let add_stake_msg = ExecuteMsg::AddStake {};
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), add_stake_msg).unwrap();

        let mut new_env = mock_env();
        new_env.block.height += 4;
        new_env.block.time = Timestamp::from_nanos(env.block.time.nanos() + 4 * 1_000_000_000);

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryRewards {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: Uint128 = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(4u128), value);

        let claim_msg = ExecuteMsg::ClaimRewards {};
        let res = execute(deps.as_mut(), new_env.clone(), info.clone(), claim_msg).unwrap();

        assert_eq!(res.attributes.len(), 1);
        assert_eq!(res.attributes[0], attr("action", "claim"));

        assert_eq!(
            res.messages[0].msg,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "anyone".into(),
                amount: coins(4, "nanomobx"),
            })
        );

        let res = query(
            deps.as_ref(),
            new_env.clone(),
            QueryMsg::QueryRewards {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value: Uint128 = from_binary(&res).unwrap();

        assert_eq!(Uint128::zero(), value);
    }

    #[test]
    fn pause_and_auth() {
        let mut deps = mock_dependencies_with_balance(&coins(200, "nanomobx"));

        let msg = InstantiateMsg {
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::from(1u128),
            paused: true,
            unbonding_period: Uint64::from(1u64),
        };

        let creator_info = mock_info("creator", &coins(1000, "nanomobx"));
        let env = mock_env();
        let _res = instantiate(deps.as_mut(), env.clone(), creator_info.clone(), msg).unwrap();

        let info = mock_info("anyone", &coins(10, "nanomobx"));
        let add_stake_msg = ExecuteMsg::AddStake {};
        let err = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            add_stake_msg.clone(),
        )
        .unwrap_err();

        match err {
            ContractError::ContractPaused {} => {}
            e => panic!("unexpecter error: {}", e),
        }

        let new_config = Config {
            owner: Addr::unchecked("creator"),
            chief_pausing_officer: Addr::unchecked("CPO"),
            denom: "nanomobx".to_string(),
            reward_rate: Uint128::from(1u128),
            paused: false,
            unbonding_period: Uint64::from(1u64),
        };

        let update_config_msg = ExecuteMsg::UpdateConfig {
            config: new_config.clone(),
        };

        // Check if Authorization works
        let auth_err = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            update_config_msg.clone(),
        )
        .unwrap_err();

        match auth_err {
            ContractError::Unauthorized {} => {}
            e => panic!("unexpecter error: {}", e),
        }

        let _res = execute(
            deps.as_mut(),
            env.clone(),
            creator_info.clone(),
            update_config_msg,
        )
        .unwrap();
        let _res = execute(deps.as_mut(), env.clone(), info, add_stake_msg.clone()).unwrap();

        let res = query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::QueryStake {
                address: Addr::unchecked("anyone"),
            },
        )
        .unwrap();
        let value = from_binary(&res).unwrap();

        assert_eq!(Uint128::from(10u128), value);
    }
}
