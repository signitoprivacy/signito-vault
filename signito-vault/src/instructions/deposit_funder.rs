use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, system_instruction};

use crate::errors::SignitoError;
use crate::state::FunderState;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct DepositFunderArgs {
    pub amount: u64,
}

// Anyone can deposit SOL into FunderPDA to keep it funded.
#[derive(Accounts)]
pub struct DepositFunder<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,

    #[account(
        mut,
        seeds = [b"funder"],
        bump = funder_pda.bump,
    )]
    pub funder_pda: Account<'info, FunderState>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<DepositFunder>, args: DepositFunderArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);

    invoke(
        &system_instruction::transfer(
            ctx.accounts.depositor.key,
            ctx.accounts.funder_pda.to_account_info().key,
            args.amount,
        ),
        &[
            ctx.accounts.depositor.to_account_info(),
            ctx.accounts.funder_pda.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    msg!("FunderPDA: deposited {} lamports", args.amount);

    Ok(())
}
