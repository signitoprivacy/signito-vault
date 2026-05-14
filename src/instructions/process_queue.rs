use anchor_lang::prelude::*;

use crate::errors::SignitoError;
use crate::state::{FunderState, PoolState};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ProcessQueueArgs {
    pub amount: u64,
}

// TX2 of the 2-TX StealthSend flow.
//
// Authorized relayer sends SOL from pool_pda to the recipient.
// The recipient address was received off-chain by the relayer server after TX1 confirmed.
//
// No account from TX1 (burn_and_queue) appears here: zero on-chain link between TX1 and TX2.
// On-chain trace from TX2: relayer -> pool_pda -> recipient. Nothing else.
//
// Authorization: relayer must match funder_pda.relayer (set by admin via set_relayer).
#[derive(Accounts)]
pub struct ProcessQueue<'info> {
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
    )]
    pub pool_pda: Account<'info, PoolState>,

    /// CHECK: SOL destination -- any valid address
    #[account(mut)]
    pub recipient: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<ProcessQueue>, args: ProcessQueueArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);

    let pool_info = ctx.accounts.pool_pda.to_account_info();
    let recipient_info = ctx.accounts.recipient.to_account_info();

    let pool_lamports = pool_info.lamports();
    require!(pool_lamports >= args.amount, SignitoError::InsufficientFunds);

    **pool_info.try_borrow_mut_lamports()? = pool_lamports
        .checked_sub(args.amount)
        .ok_or(SignitoError::Overflow)?;
    **recipient_info.try_borrow_mut_lamports()? = recipient_info
        .lamports()
        .checked_add(args.amount)
        .ok_or(SignitoError::Overflow)?;

    msg!(
        "ProcessQueue: {} lamports -> {}",
        args.amount,
        ctx.accounts.recipient.key,
    );

    Ok(())
}
