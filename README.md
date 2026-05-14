# signito-vault

Anchor program for the Signito non-custodial privacy protocol on Solana.

**Program ID (mainnet):** `CZBvErdLT8HL2iJS9NrRn7PhdeFWKNcMmvweEPsSbAAX`

## Architecture

- Shared pool PDA (`seeds=[b"pool"]`) holds all deposited SOL for the protocol.
- Shared sSOL mint (`zZs2Buajob4MLf1UYzVRhNFuimSmiXVY1VRSPc9bysi`) - Token-2022 NonTransferable + PermanentDelegate.
- Per-user `UserState` PDA (`seeds=[b"user_state", stoken_ata.key()]`) - derived from random stoken ATA, not wallet address.
- `FunderPDA` (`seeds=[b"funder"]`) - operational SOL reserve for funding ephemeral wallets.

## Privacy Model

- `private_send` / `burn_and_queue` + `process_queue`: owner wallet does not appear in instruction accounts.
- OTS chain: PBKDF2-derived hash chain, each withdrawal consumes one hash preimage.
- 2-TX StealthSend: TX1 burns sSOL (no recipient on-chain), TX2 sends SOL to recipient (no link to TX1).

## Build (reproducible)

```bash
# Install solana-verifiable-build
cargo install solana-verify

# Build with verifiable Docker image
solana-verify build --library-name signito_vault

# Verify against on-chain program
solana-verify verify-from-repo \
  --url https://api.mainnet-beta.solana.com \
  --program-id CZBvErdLT8HL2iJS9NrRn7PhdeFWKNcMmvweEPsSbAAX \
  https://github.com/signitoprivacy/signito-vault
```

## Manual deploy / upgrade

```bash
export PLATFORM_TOOLS="$HOME/.cache/solana/v1.52/platform-tools"
export PATH="$PLATFORM_TOOLS/rust/bin:$PLATFORM_TOOLS/llvm/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd signito-vault
cargo build --target sbf-solana-solana --release
llvm-objcopy --strip-all \
  ../target/sbf-solana-solana/release/signito_vault.so \
  ../target/deploy/signito_vault_upgrade.so
solana program deploy --program-id <KEYPAIR> ../target/deploy/signito_vault_upgrade.so
```

## License

MIT
