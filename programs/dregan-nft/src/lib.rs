use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint, MintTo};
use anchor_spl::associated_token::AssociatedToken;

declare_id!("DrgNFT11111111111111111111111111111111111111");

/// DREGAN NFT Access Tiers Program
/// 3-tier system: Bronze, Silver, Gold
/// - Bronze (Tier 1): Basic access - 100 DREGAN tokens
/// - Silver (Tier 2): Premium access - 500 DREGAN tokens  
/// - Gold (Tier 3): Full access - 2000 DREGAN tokens

#[program]
pub mod dregan_nft {
    use super::*;

    /// Initialize the NFT program with admin settings
    pub fn initialize(
        ctx: Context<Initialize>,
        treasury_wallet: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.authority = ctx.accounts.authority.key();
        config.treasury = treasury_wallet;
        config.dregan_mint = ctx.accounts.dregan_mint.key();
        config.is_paused = false;
        config.total_minted = 0;
        config.bump = ctx.bumps.config;
        
        msg!("DREGAN NFT Program Initialized");
        Ok(())
    }

    /// Initialize a tier with mint and pricing
    pub fn create_tier(
        ctx: Context<CreateTier>,
        tier_id: u8,
        tier_name: String,
        price: u64,
        max_supply: u64,
        uri: String,
    ) -> Result<()> {
        require!(tier_id >= 1 && tier_id <= 3, DreganNftError::InvalidTier);
        require!(tier_name.len() <= 32, DreganNftError::NameTooLong);
        require!(uri.len() <= 200, DreganNftError::UriTooLong);

        let tier = &mut ctx.accounts.tier;
        tier.tier_id = tier_id;
        tier.tier_name = tier_name;
        tier.price = price;
        tier.max_supply = max_supply;
        tier.current_supply = 0;
        tier.nft_mint = ctx.accounts.nft_mint.key();
        tier.metadata_uri = uri;
        tier.is_active = true;
        tier.bump = ctx.bumps.tier;

        msg!("Created tier {} with price {} DREGAN", tier_id, price);
        Ok(())
    }

    /// Purchase an NFT from a specific tier using DREGAN tokens
    pub fn purchase_nft(ctx: Context<PurchaseNft>, tier_id: u8) -> Result<()> {
        let config = &ctx.accounts.config;
        let tier = &mut ctx.accounts.tier;

        require!(!config.is_paused, DreganNftError::ProgramPaused);
        require!(tier.is_active, DreganNftError::TierNotActive);
        require!(tier.tier_id == tier_id, DreganNftError::InvalidTier);
        require!(tier.current_supply < tier.max_supply, DreganNftError::SoldOut);

        let price = tier.price;

        // Transfer DREGAN tokens from buyer to treasury
        let cpi_accounts = Transfer {
            from: ctx.accounts.buyer_token_account.to_account_info(),
            to: ctx.accounts.treasury_token_account.to_account_info(),
            authority: ctx.accounts.buyer.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, price)?;

        // Mint NFT to buyer
        let seeds = &[
            b"config".as_ref(),
            &[config.bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts = MintTo {
            mint: ctx.accounts.nft_mint.to_account_info(),
            to: ctx.accounts.buyer_nft_account.to_account_info(),
            authority: ctx.accounts.config.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer,
        );
        token::mint_to(cpi_ctx, 1)?;

        // Update state
        tier.current_supply += 1;

        // Create holder record
        let holder = &mut ctx.accounts.holder_record;
        holder.holder = ctx.accounts.buyer.key();
        holder.tier_id = tier_id;
        holder.nft_mint = ctx.accounts.nft_mint.key();
        holder.purchase_timestamp = Clock::get()?.unix_timestamp;
        holder.price_paid = price;
        holder.bump = ctx.bumps.holder_record;

        emit!(NftPurchased {
            buyer: ctx.accounts.buyer.key(),
            tier_id,
            price_paid: price,
            nft_mint: ctx.accounts.nft_mint.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("NFT purchased! Tier: {}, Price: {} DREGAN", tier_id, price);
        Ok(())
    }

    /// Verify if a wallet holds a specific tier NFT
    pub fn verify_holder(ctx: Context<VerifyHolder>, tier_id: u8) -> Result<bool> {
        let holder = &ctx.accounts.holder_record;
        let nft_account = &ctx.accounts.nft_account;

        // Verify holder record matches
        let is_valid = holder.holder == ctx.accounts.wallet.key()
            && holder.tier_id == tier_id
            && nft_account.amount >= 1
            && nft_account.owner == ctx.accounts.wallet.key();

        msg!("Holder verification: {}", is_valid);
        Ok(is_valid)
    }

    /// Get tier access level for a holder (returns 0 if none, 1-3 for tiers)
    pub fn get_access_level(ctx: Context<GetAccessLevel>) -> Result<u8> {
        let holder = &ctx.accounts.holder_record;
        let nft_account = &ctx.accounts.nft_account;

        if holder.holder == ctx.accounts.wallet.key() && nft_account.amount >= 1 {
            msg!("Access level: {}", holder.tier_id);
            return Ok(holder.tier_id);
        }

        msg!("No access");
        Ok(0)
    }

    /// Admin: Pause/unpause the program
    pub fn set_paused(ctx: Context<AdminAction>, paused: bool) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(ctx.accounts.authority.key() == config.authority, DreganNftError::Unauthorized);
        
        config.is_paused = paused;
        msg!("Program paused: {}", paused);
        Ok(())
    }

    /// Admin: Update tier status
    pub fn set_tier_active(ctx: Context<UpdateTier>, is_active: bool) -> Result<()> {
        let config = &ctx.accounts.config;
        require!(ctx.accounts.authority.key() == config.authority, DreganNftError::Unauthorized);
        
        let tier = &mut ctx.accounts.tier;
        tier.is_active = is_active;
        msg!("Tier {} active: {}", tier.tier_id, is_active);
        Ok(())
    }

    /// Admin: Update tier price
    pub fn update_tier_price(ctx: Context<UpdateTier>, new_price: u64) -> Result<()> {
        let config = &ctx.accounts.config;
        require!(ctx.accounts.authority.key() == config.authority, DreganNftError::Unauthorized);
        
        let tier = &mut ctx.accounts.tier;
        let old_price = tier.price;
        tier.price = new_price;
        msg!("Tier {} price updated: {} -> {}", tier.tier_id, old_price, new_price);
        Ok(())
    }
}

// ============================================================================
// ACCOUNTS
// ============================================================================

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + ProgramConfig::LEN,
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, ProgramConfig>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub dregan_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct CreateTier<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        constraint = authority.key() == config.authority @ DreganNftError::Unauthorized
    )]
    pub config: Account<'info, ProgramConfig>,
    
    #[account(
        init,
        payer = authority,
        space = 8 + TierConfig::LEN,
        seeds = [b"tier", &[tier_id]],
        bump
    )]
    pub tier: Account<'info, TierConfig>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    /// The NFT mint for this tier
    #[account(mut)]
    pub nft_mint: Account<'info, Mint>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct PurchaseNft<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump
    )]
    pub config: Account<'info, ProgramConfig>,
    
    #[account(
        mut,
        seeds = [b"tier", &[tier_id]],
        bump = tier.bump
    )]
    pub tier: Account<'info, TierConfig>,
    
    #[account(
        init,
        payer = buyer,
        space = 8 + HolderRecord::LEN,
        seeds = [b"holder", buyer.key().as_ref(), &[tier_id]],
        bump
    )]
    pub holder_record: Account<'info, HolderRecord>,
    
    #[account(mut)]
    pub buyer: Signer<'info>,
    
    /// Buyer's DREGAN token account
    #[account(
        mut,
        constraint = buyer_token_account.mint == config.dregan_mint,
        constraint = buyer_token_account.owner == buyer.key()
    )]
    pub buyer_token_account: Account<'info, TokenAccount>,
    
    /// Treasury's DREGAN token account
    #[account(
        mut,
        constraint = treasury_token_account.mint == config.dregan_mint,
        constraint = treasury_token_account.owner == config.treasury
    )]
    pub treasury_token_account: Account<'info, TokenAccount>,
    
    /// NFT mint for this tier
    #[account(
        mut,
        constraint = nft_mint.key() == tier.nft_mint
    )]
    pub nft_mint: Account<'info, Mint>,
    
    /// Buyer's NFT token account
    #[account(
        init_if_needed,
        payer = buyer,
        associated_token::mint = nft_mint,
        associated_token::authority = buyer
    )]
    pub buyer_nft_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct VerifyHolder<'info> {
    #[account(
        seeds = [b"holder", wallet.key().as_ref(), &[tier_id]],
        bump = holder_record.bump
    )]
    pub holder_record: Account<'info, HolderRecord>,
    
    pub wallet: Signer<'info>,
    
    #[account(
        constraint = nft_account.mint == holder_record.nft_mint,
        constraint = nft_account.owner == wallet.key()
    )]
    pub nft_account: Account<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct GetAccessLevel<'info> {
    #[account(
        seeds = [b"holder", wallet.key().as_ref(), &[holder_record.tier_id]],
        bump = holder_record.bump
    )]
    pub holder_record: Account<'info, HolderRecord>,
    
    pub wallet: Signer<'info>,
    
    #[account(
        constraint = nft_account.mint == holder_record.nft_mint,
        constraint = nft_account.owner == wallet.key()
    )]
    pub nft_account: Account<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct AdminAction<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump
    )]
    pub config: Account<'info, ProgramConfig>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateTier<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump
    )]
    pub config: Account<'info, ProgramConfig>,
    
    #[account(
        mut,
        seeds = [b"tier", &[tier.tier_id]],
        bump = tier.bump
    )]
    pub tier: Account<'info, TierConfig>,
    
    pub authority: Signer<'info>,
}

// ============================================================================
// STATE ACCOUNTS
// ============================================================================

#[account]
pub struct ProgramConfig {
    pub authority: Pubkey,
    pub treasury: Pubkey,
    pub dregan_mint: Pubkey,
    pub is_paused: bool,
    pub total_minted: u64,
    pub bump: u8,
}

impl ProgramConfig {
    pub const LEN: usize = 32 + 32 + 32 + 1 + 8 + 1;
}

#[account]
pub struct TierConfig {
    pub tier_id: u8,
    pub tier_name: String,      // Max 32 chars
    pub price: u64,             // Price in DREGAN tokens
    pub max_supply: u64,
    pub current_supply: u64,
    pub nft_mint: Pubkey,
    pub metadata_uri: String,   // Max 200 chars
    pub is_active: bool,
    pub bump: u8,
}

impl TierConfig {
    pub const LEN: usize = 1 + (4 + 32) + 8 + 8 + 8 + 32 + (4 + 200) + 1 + 1;
}

#[account]
pub struct HolderRecord {
    pub holder: Pubkey,
    pub tier_id: u8,
    pub nft_mint: Pubkey,
    pub purchase_timestamp: i64,
    pub price_paid: u64,
    pub bump: u8,
}

impl HolderRecord {
    pub const LEN: usize = 32 + 1 + 32 + 8 + 8 + 1;
}

// ============================================================================
// EVENTS
// ============================================================================

#[event]
pub struct NftPurchased {
    pub buyer: Pubkey,
    pub tier_id: u8,
    pub price_paid: u64,
    pub nft_mint: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct TierCreated {
    pub tier_id: u8,
    pub tier_name: String,
    pub price: u64,
    pub max_supply: u64,
}

// ============================================================================
// ERRORS
// ============================================================================

#[error_code]
pub enum DreganNftError {
    #[msg("Invalid tier. Must be 1 (Bronze), 2 (Silver), or 3 (Gold)")]
    InvalidTier,
    
    #[msg("Tier name too long. Max 32 characters")]
    NameTooLong,
    
    #[msg("Metadata URI too long. Max 200 characters")]
    UriTooLong,
    
    #[msg("Program is currently paused")]
    ProgramPaused,
    
    #[msg("This tier is not active")]
    TierNotActive,
    
    #[msg("This tier is sold out")]
    SoldOut,
    
    #[msg("Insufficient DREGAN tokens")]
    InsufficientFunds,
    
    #[msg("Unauthorized action")]
    Unauthorized,
    
    #[msg("Already owns NFT from this tier")]
    AlreadyOwned,
}
