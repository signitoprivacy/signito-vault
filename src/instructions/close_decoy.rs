use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{FunderState, PoolState};

// Close depleted decoy token accounts and their associated user_state PDAs.
// Returns all rent lamports to the relayer.
//
// Called in a separate block after decoy_burn has confirmed.
// Keeping this in a different block from the burn breaks any on-chain link
// between the user's unshield and the cleanup TX.
//
// pool_pda must be the close_authority on each stoken_ata (set at decoy_shield time).
// Each stoken_ata must have a zero sSOL balance (burned by decoy_burn beforehand).
//
// remaining_accounts layout -- interleaved pairs:
//   [stoken_ata_0 writable, user_state_0 writable,
//    stoken_ata_1 writable, user_state_1 writable, ...]
//
// Fixed accounts:
//   0. relayer          writable signer (must match funder_pda.relayer, receives rent)
//   1. funder_pda       readonly PDA (seeds=[b"funder"], auth check)
//   2. pool_pda         readonly PDA (seeds=[b"pool"], is close_authority on each stoken_ata)
//   3. token_prog_22    readonly
#[derive(Accounts)]
pub struct CloseDecoy<'info> {
    #[account(
        mut,
        constraint = relayer.key() == funder_pda.relayer @ SignitoError::Unauthorized
    )]
    pub relayer: Signer<'info>,

    #[account(
        seeds = [b"funder"],
        bump = funder_pda.bump,
    )]
    pub funder_pda: Account<'info, FunderState>,

    #[account(
        seeds = [b"pool"],
        bump = pool_pda.bump,
    )]
    pub pool_pda: Account<'info, PoolState>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, CloseDecoy<'info>>,
) -> Result<()> {
    require!(
        !ctx.remaining_accounts.is_empty(),
        SignitoError::InvalidAmount
    );
    require!(
        ctx.remaining_accounts.len() % 2 == 0,
        SignitoError::InvalidAmount
    );

    let pool_bump = ctx.accounts.pool_pda.bump;
    let pool_key = ctx.accounts.pool_pda.key();
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[pool_bump]]];

    let relayer_info = ctx.accounts.relayer.to_account_info();
    let pool_info = ctx.accounts.pool_pda.to_account_info();

    let count = ctx.remaining_accounts.len() / 2;

    for chunk in ctx.remaining_accounts.chunks(2) {
        let stoken_ata = &chunk[0];
        let user_state_info = &chunk[1];

        // Verify the user_state PDA derives correctly from this stoken_ata.
        // Prevents caller from passing arbitrary accounts to drain lamports.
        let (expected_user_state, _bump) = Pubkey::find_program_address(
            &[b"user_state", stoken_ata.key.as_ref()],
            ctx.program_id,
        );
        require_keys_eq!(
            *user_state_info.key,
            expected_user_state,
            SignitoError::Unauthorized
        );

        // Close the stoken_ata via Token-2022.
        // pool_pda is the close_authority (set at decoy_shield time).
        // Lamports go to relayer.
        invoke_signed(
            &spl_token_2022::instruction::close_account(
                &TOKEN_2022_ID,
                stoken_ata.key,
                relayer_info.key,
                &pool_key,
                &[],
            )
            .map_err(|_| error!(SignitoError::Overflow))?,
            &[
                stoken_ata.clone(),
                relayer_info.clone(),
                pool_info.clone(),
            ],
            pool_seeds,
        )?;

        // user_state PDA rent is recovered in a separate instruction
        // (close_account / on user unshield). Combining it here caused a
        // Solana runtime lamport-balance check failure after the Token-2022 CPI.
    }

    msg!(
        "CloseDecoy: {} decoy account pairs closed. Rent recovered to relayer.",
        count,
    );

    Ok(())
}
