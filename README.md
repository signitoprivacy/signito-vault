# Signito Vault

Non-custodial transaction privacy protocol on Solana. Anchor program powering SafeVault (OTS Protocol), StealthSend (ZK-style burn+queue), and AirSign (offline vouchers).

## Deployed Addresses (Mainnet)

| Account | Address |
|---|---|
| Program ID | `CZBvErdLT8HL2iJS9NrRn7PhdeFWKNcMmvweEPsSbAAX` |
| Pool PDA | `ABgGyjfdqKQxq5d9T2UK78QUemf1RmQTYthRJkKSAm6H` |
| sSOL Mint | `zZs2Buajob4MLf1UYzVRhNFuimSmiXVY1VRSPc9bysi` |

The sSOL mint uses SPL Token-2022 with two extensions:
- `NonTransferable`: sSOL is soulbound, visible in wallets but cannot be sent
- `PermanentDelegate = pool_pda`: allows the pool to burn sSOL without the token account owner's signature, enabling private withdrawals

## Instructions

| Instruction | Description |
|---|---|
| `initialize_pool` | One-time: create pool PDA and shared sSOL mint |
| `shield` | Deposit SOL, mint sSOL to a fresh random token account |
| `deposit` | Add more SOL to an existing vault position |
| `refresh_ots` | Rotate the OTS hash chain for a vault |
| `burn_and_queue` | Burn sSOL, queue a pending withdrawal (two-TX privacy) |
| `process_queue` | Release queued SOL to recipient |
| `private_send` | Legacy single-TX burn + transfer (kept for compatibility) |
| `mint_airsign` | Burn sSOL into an offline-claimable escrow voucher |
| `claim_airsign` | Claim a voucher to a wallet address |
| `close_account` | Close an empty user state account |
| `rotate_mint` | Admin: replace the shared sSOL mint |
| `init_stoken_metadata` | Admin: set on-chain token metadata |
| `set_relayer` | Admin: configure the trusted relayer |
| `init_funder` / `deposit_funder` / `withdraw_funder` | Funder PDA management |
| `fund_fresh_relayer` | Internal: fund ephemeral wallets for gas |

## Architecture

- **Pool PDA** (`seeds=[b"pool"]`): shared across all users. Holds deposited SOL, is mint authority and permanent delegate of the sSOL mint.
- **UserState PDA** (`seeds=[b"user_state", stokenAta]`): derived from a random stoken account address, NOT from the user's wallet -- privacy guarantee.
- **OTS Protocol**: each vault uses a PBKDF2-derived hash chain. Every withdrawal consumes one OTS (one-time signature), advancing the chain tip.
- **Two-actor shield**: the depositing keypair (server-controlled, discarded after one use) and the display owner (user's real wallet, non-signer) are separate. sSOL appears under the user's wallet in Phantom without ever exposing the deposit origin.
- **Burn + queue**: `burn_and_queue` and `process_queue` are separate transactions, breaking the on-chain link between depositor and recipient.

## Build

Requires Solana platform tools v1.52 and a Rust SBF toolchain.

```sh
export PLATFORM_TOOLS="$HOME/.cache/solana/v1.52/platform-tools"
export PATH="$PLATFORM_TOOLS/rust/bin:$PLATFORM_TOOLS/llvm/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"

cd programs/signito-vault
cargo build --target sbf-solana-solana --release
llvm-objcopy --strip-all \
  target/sbf-solana-solana/release/signito_vault.so \
  ../target/deploy/signito_vault_upgrade.so
```

## License

MIT
