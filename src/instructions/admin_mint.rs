use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{FunderState, PoolState};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AdminMintArgs {
    pub amount: u64,
}

// Mint phantom sSOL to a decoy token account without requiring SOL deposit.
//
// Used to pre-fill or restore decoy accounts for the mix layer.
// pool_pda is the mint authority and signs via PDA seeds.
// pool.total_deposited is NOT updated -- this sSOL has no SOL backing.
// Only the authorized relayer can call this instruction.
//
// Accounts:
//   0. relayer          writable signer (must match funder_pda.relayer)
//   1. funder_pda       readonly PDA (seeds=[b"funder"], auth check)
//   2. pool_pda         writable PDA (seeds=[b"pool"], mint authority)
//   3. mint_stoken      writable (shared sSOL mint)
//   4. dest_stoken_ata  writable (decoy token account to receive sSOL)
//   5. token_prog_22    readonly
#[derive(Accounts)]
pub struct AdminMint<'info> {
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

    /// CHECK: destination sSOL token account (decoy); must be initialized
    #[account(mut)]
    pub dest_stoken_ata: UncheckedAccount<'info>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<AdminMint>, args: AdminMintArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);

    let pool_bump = ctx.accounts.pool_pda.bump;
    let pool_key = ctx.accounts.pool_pda.key();
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[pool_bump]]];

    invoke_signed(
        &spl_token_2022::instruction::mint_to(
            &TOKEN_2022_ID,
            ctx.accounts.mint_stoken.key,
            ctx.accounts.dest_stoken_ata.key,
            &pool_key,
            &[],
            args.amount,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.mint_stoken.to_account_info(),
            ctx.accounts.dest_stoken_ata.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
        ],
        pool_seeds,
    )?;

    msg!(
        "AdminMint: {} phantom sSOL minted to decoy account {}",
        args.amount,
        ctx.accounts.dest_stoken_ata.key,
    );

    Ok(())
}
