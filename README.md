# DREGAN Smart Contracts

Native Solana smart contracts for the DREGAN AI utilities platform.

## Deployed Contracts (Devnet)

| Contract | Program ID |
|----------|------------|
| **Staking** | `8nEE9CgLAEMmVmN5R4tdPuVhJLp4sU9i87QiFVXcdwKP` |
| **NFT Access** | `qTSt5stsafLoERpm4j61meXw5ywNnMwgXSDxsiZDJ4C` |

## Build Requirements

- Solana CLI: 1.17.28
- Rust: 1.68.0
- solana-program: 1.17.28
- borsh: 0.10.3

## Contracts

### Staking Contract (`dregan-staking`)

Manages staking pools with configurable lock periods and APY rates:

- 30-day lock: 10% APY
- 60-day lock: 15% APY
- 90-day lock: 20% APY

**Instructions**:
- `initialize` - Initialize staking program
- `stake` - Stake tokens with lock period
- `unstake` - Withdraw staked tokens after lock
- `claim_rewards` - Claim earned rewards

### NFT Access Contract (`dregan-nft`)

Three-tier NFT access system for platform features:

| Tier | Required Amount | Features |
|------|-----------------|----------|
| BASIC | 1,000 DREGAN | Launch Monitor, Basic Alerts |
| PRO | 5,000 DREGAN | + Sniper Bot, Honeypot Detection |
| ELITE | 25,000 DREGAN | + AI Hub, Chart Oracle, Bot Builder |

**Instructions**:
- `initialize` - Initialize NFT program
- `mint_access_nft` - Mint access NFT based on holdings
- `verify_access` - Verify user access tier
- `upgrade_tier` - Upgrade to higher tier

## Building

```bash
# Install Solana CLI 1.17.28
sh -c "$(curl -sSfL https://release.anza.xyz/v1.17.28/install)"

# Build contracts
cd programs/dregan-staking
cargo build-sbf

cd ../dregan-nft
cargo build-sbf
```

## Deployment

Automated via GitHub Actions. Trigger manually from Actions tab.

## Token

DREGAN Token: `FBDBUPkifjpY5AzT8cHg9CKjJEwJqKfKdpCnHQq4mray`

## License

MIT