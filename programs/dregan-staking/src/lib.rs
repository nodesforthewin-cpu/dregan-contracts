use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer, MintTo};
use anchor_spl::associated_token::AssociatedToken;

declare_id!("DrgSTK1111111111111111111111111111111111111");

/// DREGAN Staking Program
/// Lock periods and rewards:
/// - 30 days: 10% APY
/// - 60 days: 15% APY
/// - 90 days: 20% APY

#[program]
pub mod dregan_staking {
    use super::*;

    /// Initialize the staking pool
    pub fn initialize(
        ctx: Context<Initialize>,
        reward_rate_30: u16,  // basis points (1000 = 10%)
        reward_rate_60: u16,  // basis points (1500 = 15%)
        reward_rate_90: u16,  // basis points (2000 = 20%)
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.authority = ctx.accounts.authority.key();
        pool.stake_mint = ctx.accounts.stake_mint.key();
        pool.reward_mint = ctx.accounts.reward_mint.key();
        pool.pool_vault = ctx.accounts.pool_vault.key();
        pool.total_staked = 0;
        pool.total_stakers = 0;
        pool.reward_rate_30 = reward_rate_30;
        pool.reward_rate_60 = reward_rate_60;
        pool.reward_rate_90 = reward_rate_90;
        pool.is_paused = false;
        pool.bump = ctx.bumps.pool;
        
        msg!("DREGAN Staking Pool Initialized");
        msg!("30-day APY: {}%", reward_rate_30 as f64 / 100.0);
        msg!("60-day APY: {}%", reward_rate_60 as f64 / 100.0);
        msg!("90-day APY: {}%", reward_rate_90 as f64 / 100.0);
        Ok(())
    }

    /// Stake DREGAN tokens for 30, 60, or 90 days
    pub fn stake(ctx: Context<Stake>, amount: u64, lock_days: u8) -> Result<()> {
        require!(amount > 0, StakingError::InvalidAmount);
        require!(
            lock_days == 30 || lock_days == 60 || lock_days == 90,
            StakingError::InvalidLockPeriod
        );

        let pool = &ctx.accounts.pool;
        require!(!pool.is_paused, StakingError::PoolPaused);

        let user_stake = &mut ctx.accounts.user_stake;
        let clock = Clock::get()?;

        // Transfer tokens from user to pool vault
        let cpi_accounts = Transfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.pool_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        // Calculate reward based on lock period
        let reward_rate = match lock_days {
            30 => pool.reward_rate_30,
            60 => pool.reward_rate_60,
            90 => pool.reward_rate_90,
            _ => return Err(StakingError::InvalidLockPeriod.into()),
        };
        
        let reward_amount = calculate_reward(amount, reward_rate, lock_days)?;

        // Update user stake info
        user_stake.user = ctx.accounts.user.key();
        user_stake.pool = ctx.accounts.pool.key();
        user_stake.amount = amount;
        user_stake.stake_timestamp = clock.unix_timestamp;
        user_stake.lock_days = lock_days;
        user_stake.unlock_timestamp = clock.unix_timestamp + (lock_days as i64 * 24 * 60 * 60);
        user_stake.reward_amount = reward_amount;
        user_stake.is_claimed = false;
        user_stake.bump = ctx.bumps.user_stake;

        // Update pool stats
        let pool = &mut ctx.accounts.pool;
        pool.total_staked = pool.total_staked.checked_add(amount).unwrap();
        pool.total_stakers = pool.total_stakers.checked_add(1).unwrap();

        emit!(Staked {
            user: ctx.accounts.user.key(),
            amount,
            lock_days,
            reward_amount,
            unlock_timestamp: user_stake.unlock_timestamp,
            timestamp: clock.unix_timestamp,
        });

        msg!("Staked {} DREGAN for {} days", amount, lock_days);
        msg!("Expected reward: {} DREGAN", reward_amount);
        msg!("Unlocks at: {}", user_stake.unlock_timestamp);
        Ok(())
    }

    /// Claim staked tokens + rewards after lock period
    pub fn unstake(ctx: Context<Unstake>) -> Result<()> {
        let user_stake = &ctx.accounts.user_stake;
        let clock = Clock::get()?;

        require!(!user_stake.is_claimed, StakingError::AlreadyClaimed);
        require!(
            clock.unix_timestamp >= user_stake.unlock_timestamp,
            StakingError::StillLocked
        );

        let staked_amount = user_stake.amount;
        let reward_amount = user_stake.reward_amount;
        let total_payout = staked_amount.checked_add(reward_amount).unwrap();

        // Transfer staked tokens + rewards to user
        let pool = &ctx.accounts.pool;
        let seeds = &[
            b"pool".as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.pool_vault.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, total_payout)?;

        // Update state
        let user_stake = &mut ctx.accounts.user_stake;
        user_stake.is_claimed = true;

        let pool = &mut ctx.accounts.pool;
        pool.total_staked = pool.total_staked.checked_sub(staked_amount).unwrap();
        pool.total_stakers = pool.total_stakers.saturating_sub(1);

        emit!(Unstaked {
            user: ctx.accounts.user.key(),
            staked_amount,
            reward_amount,
            total_payout,
            timestamp: clock.unix_timestamp,
        });

        msg!("Unstaked {} DREGAN + {} reward = {} total", staked_amount, reward_amount, total_payout);
        Ok(())
    }

    /// Emergency unstake - forfeit rewards, get principal back immediately
    pub fn emergency_unstake(ctx: Context<Unstake>) -> Result<()> {
        let user_stake = &ctx.accounts.user_stake;

        require!(!user_stake.is_claimed, StakingError::AlreadyClaimed);

        let staked_amount = user_stake.amount;
        let clock = Clock::get()?;

        // Calculate penalty (forfeit all rewards)
        let pool = &ctx.accounts.pool;
        let seeds = &[
            b"pool".as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];

        // Return only principal (no rewards)
        let cpi_accounts = Transfer {
            from: ctx.accounts.pool_vault.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, staked_amount)?;

        // Update state
        let user_stake = &mut ctx.accounts.user_stake;
        let forfeited = user_stake.reward_amount;
        user_stake.is_claimed = true;
        user_stake.reward_amount = 0;

        let pool = &mut ctx.accounts.pool;
        pool.total_staked = pool.total_staked.checked_sub(staked_amount).unwrap();
        pool.total_stakers = pool.total_stakers.saturating_sub(1);

        emit!(EmergencyUnstaked {
            user: ctx.accounts.user.key(),
            staked_amount,
            forfeited_rewards: forfeited,
            timestamp: clock.unix_timestamp,
        });

        msg!("Emergency unstaked {} DREGAN (forfeited {} rewards)", staked_amount, forfeited);
        Ok(())
    }

    /// View stake info (read-only)
    pub fn get_stake_info(ctx: Context<GetStakeInfo>) -> Result<StakeInfo> {
        let user_stake = &ctx.accounts.user_stake;
        let clock = Clock::get()?;
        
        let time_remaining = if clock.unix_timestamp >= user_stake.unlock_timestamp {
            0
        } else {
            user_stake.unlock_timestamp - clock.unix_timestamp
        };

        Ok(StakeInfo {
            amount: user_stake.amount,
            reward_amount: user_stake.reward_amount,
            lock_days: user_stake.lock_days,
            unlock_timestamp: user_stake.unlock_timestamp,
            time_remaining_seconds: time_remaining,
            is_unlocked: clock.unix_timestamp >= user_stake.unlock_timestamp,
            is_claimed: user_stake.is_claimed,
        })
    }

    /// Admin: Pause/unpause the pool
    pub fn set_paused(ctx: Context<AdminAction>, paused: bool) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(ctx.accounts.authority.key() == pool.authority, StakingError::Unauthorized);
        
        pool.is_paused = paused;
        msg!("Pool paused: {}", paused);
        Ok(())
    }

    /// Admin: Update reward rates
    pub fn update_reward_rates(
        ctx: Context<AdminAction>,
        rate_30: u16,
        rate_60: u16,
        rate_90: u16,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(ctx.accounts.authority.key() == pool.authority, StakingError::Unauthorized);
        
        pool.reward_rate_30 = rate_30;
        pool.reward_rate_60 = rate_60;
        pool.reward_rate_90 = rate_90;
        
        msg!("Reward rates updated: 30d={}%, 60d={}%, 90d={}%", 
            rate_30 as f64 / 100.0,
            rate_60 as f64 / 100.0,
            rate_90 as f64 / 100.0
        );
        Ok(())
    }

    /// Admin: Fund reward pool
    pub fn fund_rewards(ctx: Context<FundRewards>, amount: u64) -> Result<()> {
        let pool = &ctx.accounts.pool;
        require!(ctx.accounts.authority.key() == pool.authority, StakingError::Unauthorized);

        let cpi_accounts = Transfer {
            from: ctx.accounts.authority_token_account.to_account_info(),
            to: ctx.accounts.pool_vault.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        msg!("Funded reward pool with {} DREGAN", amount);
        Ok(())
    }
}

/// Calculate rewards based on amount, rate (basis points), and lock period
fn calculate_reward(amount: u64, rate_bps: u16, lock_days: u8) -> Result<u64> {
    // Formula: (amount * rate * days) / (10000 * 365)
    // rate is in basis points (1000 = 10%)
    let reward = (amount as u128)
        .checked_mul(rate_bps as u128)
        .ok_or(StakingError::MathOverflow)?
        .checked_mul(lock_days as u128)
        .ok_or(StakingError::MathOverflow)?
        .checked_div(10000 * 365)
        .ok_or(StakingError::MathOverflow)?;

    Ok(reward as u64)
}

// ============================================================================
// ACCOUNTS
// ============================================================================

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + Pool::LEN,
        seeds = [b"pool"],
        bump
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(
        init,
        payer = authority,
        token::mint = stake_mint,
        token::authority = pool,
        seeds = [b"vault"],
        bump
    )]
    pub pool_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub stake_mint: Account<'info, Mint>,
    pub reward_mint: Account<'info, Mint>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(
        init,
        payer = user,
        space = 8 + UserStake::LEN,
        seeds = [b"stake", user.key().as_ref(), &Clock::get().unwrap().unix_timestamp.to_le_bytes()],
        bump
    )]
    pub user_stake: Account<'info, UserStake>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        constraint = user_token_account.mint == pool.stake_mint,
        constraint = user_token_account.owner == user.key()
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        seeds = [b"vault"],
        bump,
        constraint = pool_vault.mint == pool.stake_mint
    )]
    pub pool_vault: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(
        mut,
        constraint = user_stake.user == user.key(),
        constraint = user_stake.pool == pool.key()
    )]
    pub user_stake: Account<'info, UserStake>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        constraint = user_token_account.mint == pool.stake_mint,
        constraint = user_token_account.owner == user.key()
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        seeds = [b"vault"],
        bump,
        constraint = pool_vault.mint == pool.stake_mint
    )]
    pub pool_vault: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct GetStakeInfo<'info> {
    pub user_stake: Account<'info, UserStake>,
}

#[derive(Accounts)]
pub struct AdminAction<'info> {
    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct FundRewards<'info> {
    #[account(
        seeds = [b"pool"],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(
        mut,
        constraint = authority_token_account.mint == pool.stake_mint,
        constraint = authority_token_account.owner == authority.key()
    )]
    pub authority_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        seeds = [b"vault"],
        bump
    )]
    pub pool_vault: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

// ============================================================================
// STATE ACCOUNTS
// ============================================================================

#[account]
pub struct Pool {
    pub authority: Pubkey,
    pub stake_mint: Pubkey,
    pub reward_mint: Pubkey,
    pub pool_vault: Pubkey,
    pub total_staked: u64,
    pub total_stakers: u64,
    pub reward_rate_30: u16,  // basis points
    pub reward_rate_60: u16,
    pub reward_rate_90: u16,
    pub is_paused: bool,
    pub bump: u8,
}

impl Pool {
    pub const LEN: usize = 32 + 32 + 32 + 32 + 8 + 8 + 2 + 2 + 2 + 1 + 1;
}

#[account]
pub struct UserStake {
    pub user: Pubkey,
    pub pool: Pubkey,
    pub amount: u64,
    pub stake_timestamp: i64,
    pub lock_days: u8,
    pub unlock_timestamp: i64,
    pub reward_amount: u64,
    pub is_claimed: bool,
    pub bump: u8,
}

impl UserStake {
    pub const LEN: usize = 32 + 32 + 8 + 8 + 1 + 8 + 8 + 1 + 1;
}

// ============================================================================
// RETURN TYPES
// ============================================================================

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct StakeInfo {
    pub amount: u64,
    pub reward_amount: u64,
    pub lock_days: u8,
    pub unlock_timestamp: i64,
    pub time_remaining_seconds: i64,
    pub is_unlocked: bool,
    pub is_claimed: bool,
}

// ============================================================================
// EVENTS
// ============================================================================

#[event]
pub struct Staked {
    pub user: Pubkey,
    pub amount: u64,
    pub lock_days: u8,
    pub reward_amount: u64,
    pub unlock_timestamp: i64,
    pub timestamp: i64,
}

#[event]
pub struct Unstaked {
    pub user: Pubkey,
    pub staked_amount: u64,
    pub reward_amount: u64,
    pub total_payout: u64,
    pub timestamp: i64,
}

#[event]
pub struct EmergencyUnstaked {
    pub user: Pubkey,
    pub staked_amount: u64,
    pub forfeited_rewards: u64,
    pub timestamp: i64,
}

// ============================================================================
// ERRORS
// ============================================================================

#[error_code]
pub enum StakingError {
    #[msg("Invalid amount. Must be greater than 0")]
    InvalidAmount,
    
    #[msg("Invalid lock period. Must be 30, 60, or 90 days")]
    InvalidLockPeriod,
    
    #[msg("Tokens are still locked")]
    StillLocked,
    
    #[msg("Already claimed")]
    AlreadyClaimed,
    
    #[msg("Pool is paused")]
    PoolPaused,
    
    #[msg("Unauthorized")]
    Unauthorized,
    
    #[msg("Math overflow")]
    MathOverflow,
}
