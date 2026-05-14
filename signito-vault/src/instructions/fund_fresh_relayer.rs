use anchor_lang::prelude::*;

use crate::errors::SignitoError;
use crate::state::FunderState;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct FundFreshRelayerArgs {
    pub amount: u64,
}

// Relayer-only: transfer SOL from FunderPDA to a fresh ephemeral wallet.
// The fresh wallet will sign TX1 (burn_and_queue) and then return remaining
// SOL to FunderPDA via a standard system transfer (handled off-chain by server).
#[derive(Accounts)]
pub struct FundFreshRelayer<'info> {
    #[account(
        mut,
        constraint = relayer.key() == funder_pda.relayer @ SignitoError::Unauthorized
    )]
    pub relayer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"funder"],
        bump = funder_pda.bump,
    )]
    pub funder_pda: Account<'info, FunderState>,

    /// CHECK: ephemeral fresh wallet to receive gas SOL
    #[account(mut)]
    pub fresh_wallet: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<FundFreshRelayer>, args: FundFreshRelayerArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);

    let funder_info = ctx.accounts.funder_pda.to_account_info();
    let fresh_wallet_info = ctx.accounts.fresh_wallet.to_account_info();

    let funder_lamports = funder_info.lamports();
    require!(funder_lamports >= args.amount, SignitoError::InsufficientFunds);

    **funder_info.try_borrow_mut_lamports()? = funder_lamports
        .checked_sub(args.amount)
        .ok_or(SignitoError::Overflow)?;
    **fresh_wallet_info.try_borrow_mut_lamports()? = fresh_wallet_info
        .lamports()
        .checked_add(args.amount)
        .ok_or(SignitoError::Overflow)?;

    msg!(
        "FunderPDA: funded fresh wallet {} with {} lamports",
        ctx.accounts.fresh_wallet.key,
        args.amount
    );

    Ok(())
}
