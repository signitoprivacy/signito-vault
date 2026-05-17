use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod instructions;
pub mod state;

use instructions::admin_mint::*;
use instructions::burn_and_queue::*;
use instructions::claim_airsign::*;
use instructions::close_account::*;
use instructions::close_decoy::*;
use instructions::decoy_burn::*;
use instructions::decoy_shield::*;
use instructions::deposit::*;
use instructions::deposit_funder::*;
use instructions::fund_fresh_relayer::*;
use instructions::init_funder::*;
use instructions::init_stoken_metadata::*;
use instructions::initialize_pool::*;
use instructions::mint_airsign::*;
use instructions::private_send::*;
use instructions::process_queue::*;
use instructions::refresh_ots::*;
use instructions::rotate_mint::*;
use instructions::set_relayer::*;
use instructions::shield::*;
use instructions::withdraw_funder::*;

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
    pub fn initialize_pool(ctx: Context<InitializePool>) -> Result<()> {
        instructions::initialize_pool::handler(ctx)
    }

    // Deposit SOL into the shared pool and mint sSOL to a fresh random token account.
    // Owner signs (visible once on-chain). All subsequent StealthSend ops hide owner.
    pub fn shield(ctx: Context<Shield>, args: ShieldArgs) -> Result<()> {
        instructions::shield::handler(ctx, args)
    }

    // Legacy single-TX private transfer: burn sSOL, pool sends SOL to recipient in same TX.
    // Kept for backward compatibility. Use burn_and_queue + process_queue for new flows.
    pub fn private_send<'info>(ctx: Context<'_, '_, '_, 'info, PrivateSend<'info>>, args: PrivateSendArgs) -> Result<()> {
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
    pub fn mint_airsign(ctx: Context<MintAirsign>, args: MintAirsignArgs) -> Result<()> {
        instructions::mint_airsign::handler(ctx, args)
    }

    // AirSign claim: verify Ed25519 voucher sig on-chain, release escrowed SOL to recipient.
    pub fn claim_airsign(ctx: Context<ClaimAirsign>, args: ClaimAirsignArgs) -> Result<()> {
        instructions::claim_airsign::handler(ctx, args)
    }

    // Close an empty user_state PDA and return rent to owner.
    pub fn close_account(ctx: Context<CloseAccount>) -> Result<()> {
        instructions::close_account::handler(ctx)
    }

    // One-time admin: registers Metaplex token metadata for the sSOL mint.
    pub fn init_stoken_metadata(ctx: Context<InitStokenMetadata>) -> Result<()> {
        instructions::init_stoken_metadata::handler(ctx)
    }

    // --- 2-TX StealthSend flow ---

    // TX1: OTS-verified sSOL burn. Signed by ephemeral fresh_wallet (funded by FunderPDA).
    // No recipient on-chain. Recipient submitted off-chain to relayer after this TX confirms.
    // remaining_accounts: optional decoy stoken_ata[] burned in the same instruction.
    pub fn burn_and_queue<'info>(ctx: Context<'_, '_, '_, 'info, BurnAndQueue<'info>>, args: BurnAndQueueArgs) -> Result<()> {
        instructions::burn_and_queue::handler(ctx, args)
    }

    // TX2: Authorized relayer sends SOL from pool_pda to recipient.
    // No account from TX1 appears here -- zero on-chain link between burn and send.
    pub fn process_queue(ctx: Context<ProcessQueue>, args: ProcessQueueArgs) -> Result<()> {
        instructions::process_queue::handler(ctx, args)
    }

    // --- FunderPDA management ---

    // One-time: create FunderPDA with admin and relayer pubkeys.
    pub fn init_funder(ctx: Context<InitFunder>, args: InitFunderArgs) -> Result<()> {
        instructions::init_funder::handler(ctx, args)
    }

    // Deposit SOL into FunderPDA (anyone can top it up).
    pub fn deposit_funder(ctx: Context<DepositFunder>, args: DepositFunderArgs) -> Result<()> {
        instructions::deposit_funder::handler(ctx, args)
    }

    // Admin-only: withdraw SOL from FunderPDA.
    pub fn withdraw_funder(ctx: Context<WithdrawFunder>, args: WithdrawFunderArgs) -> Result<()> {
        instructions::withdraw_funder::handler(ctx, args)
    }

    // Relayer-only: send SOL from FunderPDA to an ephemeral fresh_wallet for TX1 gas.
    pub fn fund_fresh_relayer(
        ctx: Context<FundFreshRelayer>,
        args: FundFreshRelayerArgs,
    ) -> Result<()> {
        instructions::fund_fresh_relayer::handler(ctx, args)
    }

    // Admin-only: rotate the authorized relayer pubkey in FunderPDA.
    pub fn set_relayer(ctx: Context<SetRelayer>, args: SetRelayerArgs) -> Result<()> {
        instructions::set_relayer::handler(ctx, args)
    }

    // Deployer-only: create a new NonTransferable sSOL mint and update pool_pda to use it.
    // Replaces the old frozen-account design with soulbound Token-2022 enforcement.
    pub fn rotate_mint(ctx: Context<RotateMint>) -> Result<()> {
        instructions::rotate_mint::handler(ctx)
    }

    // --- Mix layer: decoy instructions for transaction privacy ---

    // Relayer-only: mint phantom sSOL to a decoy token account without SOL deposit.
    // Used to pre-fill or restore decoy accounts for the shield/unshield mix layer.
    // pool.total_deposited is NOT updated -- no backing SOL.
    pub fn admin_mint(ctx: Context<AdminMint>, args: AdminMintArgs) -> Result<()> {
        instructions::admin_mint::handler(ctx, args)
    }

    // Relayer-only: burn sSOL from N decoy accounts (via remaining_accounts) without
    // releasing SOL from pool. Included alongside a real unshield or ZK send so
    // observers see N+1 identical burns and cannot identify the real one.
    pub fn decoy_burn<'info>(
        ctx: Context<'_, '_, '_, 'info, DecoyBurn<'info>>,
        args: DecoyBurnArgs,
    ) -> Result<()> {
        instructions::decoy_burn::handler(ctx, args)
    }

    // Relayer-only: create a structurally identical stoken_ata + user_state PDA
    // alongside a real shield, without depositing SOL. Observers see N+1 new sSOL
    // accounts in one block and cannot identify the real user's account.
    pub fn decoy_shield(ctx: Context<DecoyShield>, args: DecoyShieldArgs) -> Result<()> {
        instructions::decoy_shield::handler(ctx, args)
    }

    // Relayer-only: close depleted decoy stoken_ata accounts and their user_state PDAs.
    // Returns all rent lamports to the relayer (net cost of mix layer = 0).
    // Called in a separate block after decoy_burn confirms -- never in the same TX as
    // the burn, so observers cannot link cleanup to any specific user action.
    // remaining_accounts: interleaved pairs [stoken_ata writable, user_state writable]
    pub fn close_decoy<'info>(
        ctx: Context<'_, '_, '_, 'info, CloseDecoy<'info>>,
    ) -> Result<()> {
        instructions::close_decoy::handler(ctx)
    }
}
