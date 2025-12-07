use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    clock::Clock,
    sysvar::Sysvar,
    program_pack::Pack,
};
use borsh::{BorshDeserialize, BorshSerialize};

// DREGAN Staking Pool - Native Solana Implementation
// Tiers: 30-day (10% APY), 60-day (15% APY), 90-day (20% APY)

solana_program::declare_id!("BPzxEKTjP4jguHTxAMchAqSqAkzpbNFH87C4eJpz2zfa");

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
    pub const LEN: usize = 1 + 32 + 8 + 1 + 8 + 8 + 8 + 1;
    
    pub fn calculate_rewards(&self, current_time: i64) -> u64 {
        let staking_duration = (current_time - self.stake_timestamp) as u64;
        let seconds_per_year: u64 = 365 * 24 * 60 * 60;
        let apy = self.tier.apy_basis_points();
        (self.amount * apy * staking_duration) / (seconds_per_year * 10000)
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum StakeInstruction {
    Initialize { bump: u8 },
    Stake { amount: u64, tier: StakeTier },
    Unstake,
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
        StakeInstruction::Initialize { bump } => {
            msg!("DREGAN Staking: Initialize");
            process_initialize(program_id, accounts, bump)
        }
        StakeInstruction::Stake { amount, tier } => {
            msg!("DREGAN Staking: Stake {} tokens, tier {:?}", amount, tier);
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

fn process_initialize(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    bump: u8,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let stake_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    let mut stake_data = StakeAccount::try_from_slice(&stake_account.data.borrow())
        .unwrap_or(StakeAccount {
            is_initialized: false,
            owner: Pubkey::default(),
            amount: 0,
            tier: StakeTier::Basic,
            stake_timestamp: 0,
            unlock_timestamp: 0,
            claimed_rewards: 0,
            bump: 0,
        });
    
    if stake_data.is_initialized {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    
    stake_data.is_initialized = true;
    stake_data.owner = *owner.key;
    stake_data.bump = bump;
    
    stake_data.serialize(&mut &mut stake_account.data.borrow_mut()[..])?;
    
    msg!("Stake account initialized for {}", owner.key);
    Ok(())
}

fn process_stake(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
    tier: StakeTier,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let stake_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    let _token_account = next_account_info(accounts_iter)?;
    let _vault = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    let mut stake_data = StakeAccount::try_from_slice(&stake_account.data.borrow())?;
    
    if !stake_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if stake_data.owner != *owner.key {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let clock = Clock::get()?;
    let current_time = clock.unix_timestamp;
    
    stake_data.amount = stake_data.amount.checked_add(amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    stake_data.tier = tier.clone();
    stake_data.stake_timestamp = current_time;
    stake_data.unlock_timestamp = current_time + tier.lock_duration();
    
    stake_data.serialize(&mut &mut stake_account.data.borrow_mut()[..])?;
    
    msg!("Staked {} tokens, unlock at {}", amount, stake_data.unlock_timestamp);
    Ok(())
}

fn process_unstake(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let stake_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    let mut stake_data = StakeAccount::try_from_slice(&stake_account.data.borrow())?;
    
    if !stake_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if stake_data.owner != *owner.key {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let clock = Clock::get()?;
    if clock.unix_timestamp < stake_data.unlock_timestamp {
        msg!("Cannot unstake: lock period not ended");
        return Err(ProgramError::Custom(1)); // StillLocked
    }
    
    let amount = stake_data.amount;
    stake_data.amount = 0;
    stake_data.serialize(&mut &mut stake_account.data.borrow_mut()[..])?;
    
    msg!("Unstaked {} tokens", amount);
    Ok(())
}

fn process_claim_rewards(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let stake_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    let mut stake_data = StakeAccount::try_from_slice(&stake_account.data.borrow())?;
    
    if !stake_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if stake_data.owner != *owner.key {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let clock = Clock::get()?;
    let total_rewards = stake_data.calculate_rewards(clock.unix_timestamp);
    let claimable = total_rewards.saturating_sub(stake_data.claimed_rewards);
    
    if claimable == 0 {
        msg!("No rewards to claim");
        return Err(ProgramError::Custom(2)); // NoRewards
    }
    
    stake_data.claimed_rewards = total_rewards;
    stake_data.serialize(&mut &mut stake_account.data.borrow_mut()[..])?;
    
    msg!("Claimed {} rewards", claimable);
    Ok(())
}