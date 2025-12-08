use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    clock::Clock,
    sysvar::Sysvar,
    rent::Rent,
};
use borsh::{BorshDeserialize, BorshSerialize};
use spl_token::state::Account as TokenAccount;

// DREGAN Staking Pool - Fixed Version with Actual Token Transfers
// Tiers: 30-day (10% APY), 60-day (15% APY), 90-day (20% APY)

solana_program::declare_id!("8nEE9CgLAEMmVmN5R4tdPuVhJLp4sU9i87QiFVXcdwKP");

// Seeds for PDA derivation
pub const STAKE_SEED: &[u8] = b"stake";
pub const VAULT_SEED: &[u8] = b"vault";
pub const REWARD_VAULT_SEED: &[u8] = b"reward_vault";

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub enum StakeTier {
    Basic,  // 30 days, 10% APY
    Pro,    // 60 days, 15% APY
    Elite,  // 90 days, 20% APY
}

impl StakeTier {
    pub fn lock_duration(&self) -> i64 {
        match self {
            StakeTier::Basic => 30 * 24 * 60 * 60,  // 30 days
            StakeTier::Pro => 60 * 24 * 60 * 60,    // 60 days
            StakeTier::Elite => 90 * 24 * 60 * 60,  // 90 days
        }
    }
    
    pub fn apy_basis_points(&self) -> u64 {
        match self {
            StakeTier::Basic => 1000,  // 10%
            StakeTier::Pro => 1500,    // 15%
            StakeTier::Elite => 2000,  // 20%
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct StakeAccount {
    pub is_initialized: bool,
    pub owner: Pubkey,
    pub amount: u64,
    pub tier: StakeTier,
    pub stake_timestamp: i64,
    pub unlock_timestamp: i64,
    pub claimed_rewards: u64,
    pub bump: u8,
}

impl StakeAccount {
    pub const LEN: usize = 1 + 32 + 8 + 1 + 8 + 8 + 8 + 1; // 67 bytes
    
    pub fn calculate_rewards(&self, current_time: i64) -> u64 {
        if self.amount == 0 || self.stake_timestamp == 0 {
            return 0;
        }
        let staking_duration = (current_time - self.stake_timestamp).max(0) as u64;
        let seconds_per_year: u64 = 365 * 24 * 60 * 60;
        let apy = self.tier.apy_basis_points();
        // rewards = amount * apy * duration / (seconds_per_year * 10000)
        // Using checked math to prevent overflow
        self.amount
            .checked_mul(apy)
            .and_then(|v| v.checked_mul(staking_duration))
            .and_then(|v| v.checked_div(seconds_per_year))
            .and_then(|v| v.checked_div(10000))
            .unwrap_or(0)
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct PoolConfig {
    pub is_initialized: bool,
    pub authority: Pubkey,
    pub token_mint: Pubkey,
    pub stake_vault: Pubkey,
    pub reward_vault: Pubkey,
    pub total_staked: u64,
    pub total_rewards_distributed: u64,
    pub bump: u8,
}

impl PoolConfig {
    pub const LEN: usize = 1 + 32 + 32 + 32 + 32 + 8 + 8 + 1; // 146 bytes
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum StakeInstruction {
    /// Initialize the staking pool
    /// Accounts: [pool_config, authority, token_mint, stake_vault, reward_vault, system_program, token_program, rent]
    InitializePool { bump: u8 },
    
    /// Initialize a user stake account
    /// Accounts: [stake_account, owner, system_program]
    InitializeStake { bump: u8 },
    
    /// Stake tokens
    /// Accounts: [stake_account, owner, user_token_account, stake_vault, pool_config, token_program]
    Stake { amount: u64, tier: StakeTier },
    
    /// Unstake tokens (after lock period)
    /// Accounts: [stake_account, owner, user_token_account, stake_vault, pool_config, vault_authority, token_program]
    Unstake,
    
    /// Claim staking rewards
    /// Accounts: [stake_account, owner, user_token_account, reward_vault, pool_config, vault_authority, token_program]
    ClaimRewards,
}

entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = StakeInstruction::try_from_slice(instruction_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    
    match instruction {
        StakeInstruction::InitializePool { bump } => {
            msg!("DREGAN Staking: Initialize Pool");
            process_initialize_pool(program_id, accounts, bump)
        }
        StakeInstruction::InitializeStake { bump } => {
            msg!("DREGAN Staking: Initialize Stake Account");
            process_initialize_stake(program_id, accounts, bump)
        }
        StakeInstruction::Stake { amount, tier } => {
            msg!("DREGAN Staking: Stake {} tokens", amount);
            process_stake(program_id, accounts, amount, tier)
        }
        StakeInstruction::Unstake => {
            msg!("DREGAN Staking: Unstake");
            process_unstake(program_id, accounts)
        }
        StakeInstruction::ClaimRewards => {
            msg!("DREGAN Staking: Claim Rewards");
            process_claim_rewards(program_id, accounts)
        }
    }
}

fn process_initialize_pool(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    bump: u8,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let pool_config = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let token_mint = next_account_info(accounts_iter)?;
    let stake_vault = next_account_info(accounts_iter)?;
    let reward_vault = next_account_info(accounts_iter)?;
    
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify pool_config is owned by this program
    if pool_config.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let config = PoolConfig {
        is_initialized: true,
        authority: *authority.key,
        token_mint: *token_mint.key,
        stake_vault: *stake_vault.key,
        reward_vault: *reward_vault.key,
        total_staked: 0,
        total_rewards_distributed: 0,
        bump,
    };
    
    config.serialize(&mut &mut pool_config.data.borrow_mut()[..])?;
    msg!("Staking pool initialized by {}", authority.key);
    Ok(())
}

fn process_initialize_stake(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    bump: u8,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let stake_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify stake_account is owned by this program
    if stake_account.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Verify PDA derivation
    let (expected_pda, expected_bump) = Pubkey::find_program_address(
        &[STAKE_SEED, owner.key.as_ref()],
        program_id,
    );
    if *stake_account.key != expected_pda || bump != expected_bump {
        msg!("Invalid stake account PDA");
        return Err(ProgramError::InvalidSeeds);
    }
    
    let stake_data = StakeAccount {
        is_initialized: true,
        owner: *owner.key,
        amount: 0,
        tier: StakeTier::Basic,
        stake_timestamp: 0,
        unlock_timestamp: 0,
        claimed_rewards: 0,
        bump,
    };
    
    stake_data.serialize(&mut &mut stake_account.data.borrow_mut()[..])?;
    msg!("Stake account initialized for {}", owner.key);
    Ok(())
}

fn process_stake(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
    tier: StakeTier,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let stake_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    let user_token_account = next_account_info(accounts_iter)?;
    let stake_vault = next_account_info(accounts_iter)?;
    let pool_config_account = next_account_info(accounts_iter)?;
    let token_program = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify accounts owned by program
    if stake_account.owner != program_id || pool_config_account.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Verify token program
    if *token_program.key != spl_token::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    let mut stake_data = StakeAccount::try_from_slice(&stake_account.data.borrow())?;
    let mut pool_config = PoolConfig::try_from_slice(&pool_config_account.data.borrow())?;
    
    if !stake_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if !pool_config.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if stake_data.owner != *owner.key {
        msg!("Stake account owner mismatch");
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Verify stake vault matches pool config
    if *stake_vault.key != pool_config.stake_vault {
        msg!("Invalid stake vault");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Check if user already has an active stake
    if stake_data.amount > 0 {
        msg!("Already have active stake. Unstake first.");
        return Err(ProgramError::Custom(3));
    }
    
    // Verify user has enough tokens
    let user_token_data = TokenAccount::unpack(&user_token_account.data.borrow())?;
    if user_token_data.amount < amount {
        msg!("Insufficient token balance");
        return Err(ProgramError::InsufficientFunds);
    }
    
    // Transfer tokens from user to stake vault
    let transfer_ix = spl_token::instruction::transfer(
        token_program.key,
        user_token_account.key,
        stake_vault.key,
        owner.key,
        &[],
        amount,
    )?;
    
    invoke(
        &transfer_ix,
        &[
            user_token_account.clone(),
            stake_vault.clone(),
            owner.clone(),
            token_program.clone(),
        ],
    )?;
    
    // Update stake account
    let clock = Clock::get()?;
    stake_data.amount = amount;
    stake_data.tier = tier.clone();
    stake_data.stake_timestamp = clock.unix_timestamp;
    stake_data.unlock_timestamp = clock.unix_timestamp + tier.lock_duration();
    stake_data.claimed_rewards = 0;
    
    // Update pool config
    pool_config.total_staked = pool_config.total_staked
        .checked_add(amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    
    stake_data.serialize(&mut &mut stake_account.data.borrow_mut()[..])?;
    pool_config.serialize(&mut &mut pool_config_account.data.borrow_mut()[..])?;
    
    msg!("Staked {} tokens, tier {:?}, unlock at {}", amount, tier, stake_data.unlock_timestamp);
    Ok(())
}

fn process_unstake(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let stake_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    let user_token_account = next_account_info(accounts_iter)?;
    let stake_vault = next_account_info(accounts_iter)?;
    let pool_config_account = next_account_info(accounts_iter)?;
    let vault_authority = next_account_info(accounts_iter)?;
    let token_program = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify accounts owned by program
    if stake_account.owner != program_id || pool_config_account.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let mut stake_data = StakeAccount::try_from_slice(&stake_account.data.borrow())?;
    let mut pool_config = PoolConfig::try_from_slice(&pool_config_account.data.borrow())?;
    
    if !stake_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if stake_data.owner != *owner.key {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    if stake_data.amount == 0 {
        msg!("No tokens staked");
        return Err(ProgramError::Custom(4));
    }
    
    // Check lock period
    let clock = Clock::get()?;
    if clock.unix_timestamp < stake_data.unlock_timestamp {
        msg!("Cannot unstake: lock period not ended. Unlock at {}", stake_data.unlock_timestamp);
        return Err(ProgramError::Custom(1));
    }
    
    let amount = stake_data.amount;
    
    // Derive vault authority PDA
    let (expected_authority, authority_bump) = Pubkey::find_program_address(
        &[VAULT_SEED],
        program_id,
    );
    if *vault_authority.key != expected_authority {
        msg!("Invalid vault authority");
        return Err(ProgramError::InvalidSeeds);
    }
    
    // Transfer tokens from vault back to user
    let transfer_ix = spl_token::instruction::transfer(
        token_program.key,
        stake_vault.key,
        user_token_account.key,
        vault_authority.key,
        &[],
        amount,
    )?;
    
    let seeds = &[VAULT_SEED, &[authority_bump]];
    invoke_signed(
        &transfer_ix,
        &[
            stake_vault.clone(),
            user_token_account.clone(),
            vault_authority.clone(),
            token_program.clone(),
        ],
        &[seeds],
    )?;
    
    // Update stake account
    stake_data.amount = 0;
    stake_data.stake_timestamp = 0;
    stake_data.unlock_timestamp = 0;
    
    // Update pool config
    pool_config.total_staked = pool_config.total_staked.saturating_sub(amount);
    
    stake_data.serialize(&mut &mut stake_account.data.borrow_mut()[..])?;
    pool_config.serialize(&mut &mut pool_config_account.data.borrow_mut()[..])?;
    
    msg!("Unstaked {} tokens", amount);
    Ok(())
}

fn process_claim_rewards(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let stake_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    let user_token_account = next_account_info(accounts_iter)?;
    let reward_vault = next_account_info(accounts_iter)?;
    let pool_config_account = next_account_info(accounts_iter)?;
    let vault_authority = next_account_info(accounts_iter)?;
    let token_program = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify accounts owned by program
    if stake_account.owner != program_id || pool_config_account.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let mut stake_data = StakeAccount::try_from_slice(&stake_account.data.borrow())?;
    let mut pool_config = PoolConfig::try_from_slice(&pool_config_account.data.borrow())?;
    
    if !stake_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if stake_data.owner != *owner.key {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    if stake_data.amount == 0 {
        msg!("No active stake");
        return Err(ProgramError::Custom(5));
    }
    
    // Calculate claimable rewards
    let clock = Clock::get()?;
    let total_rewards = stake_data.calculate_rewards(clock.unix_timestamp);
    let claimable = total_rewards.saturating_sub(stake_data.claimed_rewards);
    
    if claimable == 0 {
        msg!("No rewards to claim");
        return Err(ProgramError::Custom(2));
    }
    
    // Verify reward vault has sufficient balance
    let reward_vault_data = TokenAccount::unpack(&reward_vault.data.borrow())?;
    if reward_vault_data.amount < claimable {
        msg!("Insufficient rewards in vault");
        return Err(ProgramError::InsufficientFunds);
    }
    
    // Derive vault authority PDA
    let (expected_authority, authority_bump) = Pubkey::find_program_address(
        &[VAULT_SEED],
        program_id,
    );
    if *vault_authority.key != expected_authority {
        msg!("Invalid vault authority");
        return Err(ProgramError::InvalidSeeds);
    }
    
    // Transfer rewards from reward vault to user
    let transfer_ix = spl_token::instruction::transfer(
        token_program.key,
        reward_vault.key,
        user_token_account.key,
        vault_authority.key,
        &[],
        claimable,
    )?;
    
    let seeds = &[VAULT_SEED, &[authority_bump]];
    invoke_signed(
        &transfer_ix,
        &[
            reward_vault.clone(),
            user_token_account.clone(),
            vault_authority.clone(),
            token_program.clone(),
        ],
        &[seeds],
    )?;
    
    // Update stake account
    stake_data.claimed_rewards = total_rewards;
    
    // Update pool config
    pool_config.total_rewards_distributed = pool_config.total_rewards_distributed
        .checked_add(claimable)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    
    stake_data.serialize(&mut &mut stake_account.data.borrow_mut()[..])?;
    pool_config.serialize(&mut &mut pool_config_account.data.borrow_mut()[..])?;
    
    msg!("Claimed {} reward tokens", claimable);
    Ok(())
}
