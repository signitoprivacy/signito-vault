use anchor_lang::prelude::*;

use crate::errors::SignitoError;
use crate::state::FunderState;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WithdrawFunderArgs {
    pub amount: u64,
}

// Admin-only: withdraw SOL from FunderPDA back to admin wallet.
#[derive(Accounts)]
pub struct WithdrawFunder<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"funder"],
        bump = funder_pda.bump,
        has_one = admin,
    )]
    pub funder_pda: Account<'info, FunderState>,
}

pub fn handler(ctx: Context<WithdrawFunder>, args: WithdrawFunderArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);

    let funder_info = ctx.accounts.funder_pda.to_account_info();
    let admin_info = ctx.accounts.admin.to_account_info();

    let funder_lamports = funder_info.lamports();
    require!(funder_lamports >= args.amount, SignitoError::InsufficientFunds);

    **funder_info.try_borrow_mut_lamports()? = funder_lamports
        .checked_sub(args.amount)
        .ok_or(SignitoError::Overflow)?;
    **admin_info.try_borrow_mut_lamports()? = admin_info
        .lamports()
        .checked_add(args.amount)
        .ok_or(SignitoError::Overflow)?;

    msg!("FunderPDA: withdrawn {} lamports to admin", args.amount);

    Ok(())
}
