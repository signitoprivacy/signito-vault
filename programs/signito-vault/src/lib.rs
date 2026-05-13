use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod instructions;
pub mod state;

use instructions::claim_airsign::*;
use instructions::close_account::*;
use instructions::deposit::*;
use instructions::init_stoken_metadata::*;
use instructions::initialize_pool::*;
use instructions::mint_airsign::*;
use instructions::private_send::*;
use instructions::refresh_ots::*;
use instructions::shield::*;

declare_id!("CZBvErdLT8HL2iJS9NrRn7PhdeFWKNcMmvweEPsSbAAX");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "Signito",
    project_url: "https://signito.org",
    contacts: "email:security@signito.org",
    policy: "https://signito.org/docs/security",
    preferred_languages: "en,ru",
    source_code: "https://github.com/signitoprivacy/signito-vault"
}

#[program]
pub mod signito_vault {
    use super::*;

    // One-time pool initialization: creates shared pool_pda and shared sSOL mint.
    // Called once before any shield. Anyone can call (Anchor init ensures once-only).
    pub fn initialize_pool(ctx: Context<InitializePool>) -> Result<()> {
        instructions::initialize_pool::handler(ctx)
    }

    // Deposit SOL into the shared pool and mint sSOL to a fresh random token account.
    // Owner signs (visible once on-chain). All subsequent private_send ops hide owner.
    pub fn shield(ctx: Context<Shield>, args: ShieldArgs) -> Result<()> {
        instructions::shield::handler(ctx, args)
    }

    // OTS-verified private transfer: burn sSOL, pool sends SOL to recipient.
    // Owner wallet does NOT appear in instruction accounts -- only random stoken_ata.
    pub fn private_send(ctx: Context<PrivateSend>, args: PrivateSendArgs) -> Result<()> {
        instructions::private_send::handler(ctx, args)
    }

    // Add more SOL to an existing user position and mint more sSOL.
    pub fn deposit(ctx: Context<Deposit>, args: DepositArgs) -> Result<()> {
        instructions::deposit::handler(ctx, args)
    }

    // Reset the OTS chain when chain_depth is exhausted. Owner must sign.
    pub fn refresh_ots(ctx: Context<RefreshOts>, args: RefreshOtsArgs) -> Result<()> {
        instructions::refresh_ots::handler(ctx, args)
    }

    // AirSign: burn sSOL (OTS-verified), lock SOL in AirsignEscrow for offline voucher.
    // Relayer-mediated: owner wallet does NOT appear in instruction accounts.
    pub fn mint_airsign(ctx: Context<MintAirsign>, args: MintAirsignArgs) -> Result<()> {
        instructions::mint_airsign::handler(ctx, args)
    }

    // AirSign claim: verify Ed25519 voucher sig on-chain, release escrowed SOL to recipient.
    // Relayer-mediated: no owner or recipient wallet connection required to claim.
    pub fn claim_airsign(ctx: Context<ClaimAirsign>, args: ClaimAirsignArgs) -> Result<()> {
        instructions::claim_airsign::handler(ctx, args)
    }

    // Close an empty user_state PDA and return rent to owner.
    pub fn close_account(ctx: Context<CloseAccount>) -> Result<()> {
        instructions::close_account::handler(ctx)
    }

    // One-time admin instruction: registers Metaplex token metadata for the sSOL mint.
    // pool_pda signs as mint_authority via CPI. Call once after program upgrade.
    pub fn init_stoken_metadata(ctx: Context<InitStokenMetadata>) -> Result<()> {
        instructions::init_stoken_metadata::handler(ctx)
    }
}
