# DREGAN Smart Contracts

Solana smart contracts for the DREGAN platform.

## Contracts

- **dregan-staking**: Staking pools with 30/60/90-day locks (10%/15%/20% APY)
- **dregan-nft**: NFT-based access tiers (BASIC/PRO/ELITE)

## Deployment

Contracts are automatically built and deployed via GitHub Actions.

Set these secrets in your repository:
- `DEPLOY_WALLET`: Base58-encoded Solana keypair
- `DEVNET_RPC_URL`: RPC endpoint (optional, defaults to public devnet)
