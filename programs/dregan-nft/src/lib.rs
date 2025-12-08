use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    clock::Clock,
    sysvar::Sysvar,
};
use borsh::{BorshDeserialize, BorshSerialize};
use spl_token::state::Account as TokenAccount;

// DREGAN NFT Access Control - Fixed Version with On-Chain Balance Verification
// Reads actual token balance from chain instead of trusting client input

solana_program::declare_id!("qTSt5stsafLoERpm4j61meXw5ywNnMwgXSDxsiZDJ4C");

// Token thresholds for each tier (in smallest token units, assuming 6 decimals)
// BASIC: 100 DREGAN (100 * 10^6 = 100_000_000)
// PRO: 500 DREGAN (500 * 10^6 = 500_000_000)  
// ELITE: 1000 DREGAN (1000 * 10^6 = 1_000_000_000)
pub const BASIC_THRESHOLD: u64 = 100_000_000;    // 100 DREGAN
pub const PRO_THRESHOLD: u64 = 500_000_000;      // 500 DREGAN
pub const ELITE_THRESHOLD: u64 = 1_000_000_000;  // 1000 DREGAN

// Seed for PDA derivation
pub const ACCESS_SEED: &[u8] = b"access";

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub enum AccessTier {
    None,
    Basic,
    Pro,
    Elite,
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
    
    pub fn to_u8(&self) -> u8 {
        match self {
            AccessTier::None => 0,
            AccessTier::Basic => 1,
            AccessTier::Pro => 2,
            AccessTier::Elite => 3,
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct AccessConfig {
    pub is_initialized: bool,
    pub authority: Pubkey,
    pub token_mint: Pubkey,
    pub bump: u8,
}

impl AccessConfig {
    pub const LEN: usize = 1 + 32 + 32 + 1; // 66 bytes
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
    pub const LEN: usize = 1 + 32 + 1 + 8 + 8 + 1; // 51 bytes
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum AccessInstruction {
    /// Initialize the access control config
    /// Accounts: [config_account, authority, token_mint]
    InitializeConfig { bump: u8 },
    
    /// Initialize a user access account
    /// Accounts: [access_account, owner, system_program]
    InitializeAccess { bump: u8 },
    
    /// Verify user's access tier by reading their actual token balance
    /// Accounts: [access_account, owner, user_token_account, config_account]
    VerifyAccess,
    
    /// Check current tier (read-only)
    /// Accounts: [access_account]
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
        AccessInstruction::InitializeConfig { bump } => {
            msg!("DREGAN Access: Initialize Config");
            process_initialize_config(program_id, accounts, bump)
        }
        AccessInstruction::InitializeAccess { bump } => {
            msg!("DREGAN Access: Initialize Access Account");
            process_initialize_access(program_id, accounts, bump)
        }
        AccessInstruction::VerifyAccess => {
            msg!("DREGAN Access: Verify Access");
            process_verify_access(program_id, accounts)
        }
        AccessInstruction::CheckTier => {
            msg!("DREGAN Access: Check Tier");
            process_check_tier(program_id, accounts)
        }
    }
}

fn process_initialize_config(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    bump: u8,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let config_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let token_mint = next_account_info(accounts_iter)?;
    
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify config_account is owned by this program
    if config_account.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let config = AccessConfig {
        is_initialized: true,
        authority: *authority.key,
        token_mint: *token_mint.key,
        bump,
    };
    
    config.serialize(&mut &mut config_account.data.borrow_mut()[..])?;
    msg!("Access config initialized, token mint: {}", token_mint.key);
    Ok(())
}

fn process_initialize_access(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    bump: u8,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let access_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify access_account is owned by this program
    if access_account.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Verify PDA derivation
    let (expected_pda, expected_bump) = Pubkey::find_program_address(
        &[ACCESS_SEED, owner.key.as_ref()],
        program_id,
    );
    if *access_account.key != expected_pda || bump != expected_bump {
        msg!("Invalid access account PDA");
        return Err(ProgramError::InvalidSeeds);
    }
    
    let access_data = AccessAccount {
        is_initialized: true,
        owner: *owner.key,
        current_tier: AccessTier::None,
        last_verified_balance: 0,
        verification_timestamp: 0,
        bump,
    };
    
    access_data.serialize(&mut &mut access_account.data.borrow_mut()[..])?;
    msg!("Access account initialized for {}", owner.key);
    Ok(())
}

fn process_verify_access(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let access_account = next_account_info(accounts_iter)?;
    let owner = next_account_info(accounts_iter)?;
    let user_token_account = next_account_info(accounts_iter)?;
    let config_account = next_account_info(accounts_iter)?;
    
    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify accounts owned by program
    if access_account.owner != program_id || config_account.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let mut access_data = AccessAccount::try_from_slice(&access_account.data.borrow())?;
    let config = AccessConfig::try_from_slice(&config_account.data.borrow())?;
    
    if !access_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if !config.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    if access_data.owner != *owner.key {
        msg!("Access account owner mismatch");
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Verify user_token_account is an SPL Token account
    if user_token_account.owner != &spl_token::id() {
        msg!("Invalid token account - not owned by token program");
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Read actual token balance from chain
    let token_data = TokenAccount::unpack(&user_token_account.data.borrow())?;
    
    // Verify token account belongs to the owner
    if token_data.owner != *owner.key {
        msg!("Token account owner mismatch");
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Verify token account is for the correct mint
    if token_data.mint != config.token_mint {
        msg!("Token mint mismatch. Expected: {}, Got: {}", config.token_mint, token_data.mint);
        return Err(ProgramError::InvalidAccountData);
    }
    
    let balance = token_data.amount;
    let new_tier = AccessTier::from_balance(balance);
    let clock = Clock::get()?;
    
    access_data.current_tier = new_tier.clone();
    access_data.last_verified_balance = balance;
    access_data.verification_timestamp = clock.unix_timestamp;
    
    access_data.serialize(&mut &mut access_account.data.borrow_mut()[..])?;
    
    msg!(
        "Access verified: balance = {}, tier = {:?} (level {})",
        balance,
        new_tier,
        new_tier.to_u8()
    );
    Ok(())
}

fn process_check_tier(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let access_account = next_account_info(accounts_iter)?;
    
    // Verify account owned by program
    if access_account.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    let access_data = AccessAccount::try_from_slice(&access_account.data.borrow())?;
    
    if !access_data.is_initialized {
        return Err(ProgramError::UninitializedAccount);
    }
    
    msg!(
        "Current tier: {:?} (level {}), last verified balance: {}, verified at: {}",
        access_data.current_tier,
        access_data.current_tier.to_u8(),
        access_data.last_verified_balance,
        access_data.verification_timestamp
    );
    Ok(())
}
