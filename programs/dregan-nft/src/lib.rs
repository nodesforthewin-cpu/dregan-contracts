use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};
use borsh::{BorshDeserialize, BorshSerialize};

// DREGAN NFT Access Control - Native Solana Implementation
// Tiers: BASIC, PRO, ELITE based on DREGAN token holdings

solana_program::declare_id!("7vSfwTmJCMKbZxZdBKntNgNrLiQysYrTiGjP7HzHjjUZ");

// Access tier thresholds (in DREGAN tokens with 9 decimals)
pub const BASIC_THRESHOLD: u64 = 100_000_000_000;   // 100 DREGAN
pub const PRO_THRESHOLD: u64 = 500_000_000_000;     // 500 DREGAN
pub const ELITE_THRESHOLD: u64 = 1_000_000_000_000; // 1000 DREGAN

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub enum AccessTier {
    None,
    Basic,  // 100+ DREGAN: Token Sniper, Basic Alerts
    Pro,    // 500+ DREGAN: Advanced Sniper, AI Signals
    Elite,  // 1000+ DREGAN: Full Platform, Priority Access
}

impl AccessTier {
    pub fn from_balance(balance: u64) -> Self {
        if balance >= ELITE_THRESHOLD {
            AccessTier::Elite
        } else if balance >= PRO_THRESHOLD {
            AccessTier::Pro
        } else if balance >= BASIC_THRESHOLD {
            AccessTier::Basic
        } else {
            AccessTier::None
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct AccessAccount {
    pub is_initialized: bool,
    pub owner: Pubkey,
    pub current_tier: AccessTier,
    pub last_verified_balance: u64,
    pub verification_timestamp: i64,
    pub bump: u8,
}

impl AccessAccount {
    pub const LEN: usize = 1 + 32 + 1 + 8 + 8 + 1;
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum AccessInstruction {
    Initialize { bump: u8 },
    VerifyAccess { balance: u64, timestamp: i64 },
    CheckTier,
}

entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = AccessInstruction::try_from_slice(instruction_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    
    match instruction {
        AccessInstruction::Initialize { bump } => {
            msg!("DREGAN Access: Initialize");
            process_initialize(program_id, accounts, bump)
        }
        AccessInstruction::VerifyAccess { balance, timestamp } => {
            msg!("DREGAN Access: Verify with balance {}", balance);
            process_verify_access(program_id, accounts, balance, timestamp)
        }
        AccessInstruction::CheckTier => {
            msg!("DREGAN Access: Check Tier");
            process_check_tier(program_id, accounts)
        }
    }
}

fn process_initialize(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    bump: u8,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let access_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    let mut access_data = AccessAccount::try_from_slice(&access_account.data.borrow())
        .unwrap_or(AccessAccount {
            is_initialized: false,
            owner: Pubkey::default(),
            current_tier: AccessTier::None,
            last_verified_balance: 0,
            verification_timestamp: 0,
            bump: 0,
        });
    
    if access_data.is_initialized {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    
    access_data.is_initialized = true;
    access_data.owner = *owner.key;
    access_data.bump = bump;
    
    access_data.serialize(&mut &mut access_account.data.borrow_mut()[..])?;
    
    msg!("Access account initialized for {}", owner.key);
    Ok(())
}

fn process_verify_access(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    balance: u64,
    timestamp: i64,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let access_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    let mut access_data = AccessAccount::try_from_slice(&access_account.data.borrow())?;
    
    if !access_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if access_data.owner != *owner.key {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let new_tier = AccessTier::from_balance(balance);
    access_data.current_tier = new_tier.clone();
    access_data.last_verified_balance = balance;
    access_data.verification_timestamp = timestamp;
    
    access_data.serialize(&mut &mut access_account.data.borrow_mut()[..])?;
    
    msg!("Access verified: tier {:?}, balance {}", new_tier, balance);
    Ok(())
}

fn process_check_tier(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let access_account = next_account_info(accounts_iter)?;
    
    let access_data = AccessAccount::try_from_slice(&access_account.data.borrow())?;
    
    if !access_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    msg!("Current tier: {:?}, Last balance: {}", 
         access_data.current_tier, access_data.last_verified_balance);
    Ok(())
}