use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{FunderState, PoolState};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct DecoyBurnArgs {
    pub amount: u64,
}

// Burn sSOL from N decoy accounts simultaneously, without releasing SOL from pool.
//
// Included in the same transaction as a real user's unshield or ZK send.
// Observer sees N+1 identical burns in one block and cannot identify which is real.
//
// pool_pda is the PermanentDelegate on all sSOL accounts (set at mint level by
// rotate_mint). It can burn from any account without the account owner signing.
// pool.total_deposited is NOT decremented -- no real SOL is leaving the pool.
//
// Decoy stoken_ata accounts are passed as remaining_accounts (variable count).
// Each must be writable and hold at least `amount` sSOL (minted via admin_mint).
//
// Only the authorized relayer can call this instruction.
//
// Fixed accounts:
//   0. relayer          writable signer (must match funder_pda.relayer)
//   1. funder_pda       readonly PDA (seeds=[b"funder"], auth check)
//   2. pool_pda         writable PDA (seeds=[b"pool"], burn authority)
//   3. mint_stoken      writable (shared sSOL mint)
//   4. token_prog_22    readonly
// remaining_accounts: decoy stoken_ata[] (all writable)
#[derive(Accounts)]
pub struct DecoyBurn<'info> {
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
        mut,
        seeds = [b"pool"],
        bump = pool_pda.bump,
        has_one = mint_stoken,
    )]
    pub pool_pda: Account<'info, PoolState>,

    /// CHECK: shared sSOL mint, validated via has_one
    #[account(mut, address = pool_pda.mint_stoken)]
    pub mint_stoken: UncheckedAccount<'info>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, DecoyBurn<'info>>,
    args: DecoyBurnArgs,
) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);
    require!(!ctx.remaining_accounts.is_empty(), SignitoError::InvalidAmount);

    let pool_bump = ctx.accounts.pool_pda.bump;
    let pool_key = ctx.accounts.pool_pda.key();
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[pool_bump]]];

    // Collect shared account infos as owned values before iterating remaining_accounts.
    // This avoids Anchor's lifetime invariance conflict between ctx.accounts and
    // ctx.remaining_accounts when both are used inside invoke_signed.
    let mint_info = ctx.accounts.mint_stoken.to_account_info();
    let pool_info = ctx.accounts.pool_pda.to_account_info();
    let mint_key = mint_info.key();
    let count = ctx.remaining_accounts.len();

    for decoy_ata in ctx.remaining_accounts.iter() {
        invoke_signed(
            &spl_token_2022::instruction::burn(
                &TOKEN_2022_ID,
                decoy_ata.key,
                &mint_key,
                &pool_key,
                &[],
                args.amount,
            )
            .map_err(|_| error!(SignitoError::Overflow))?,
            &[
                decoy_ata.clone(),
                mint_info.clone(),
                pool_info.clone(),
            ],
            pool_seeds,
        )?;
    }

    msg!(
        "DecoyBurn: {} phantom sSOL burned from {} decoy accounts. SOL remains in pool.",
        args.amount,
        count,
    );

    Ok(())
}
