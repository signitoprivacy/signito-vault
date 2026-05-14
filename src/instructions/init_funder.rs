use anchor_lang::prelude::*;

use crate::state::FunderState;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct InitFunderArgs {
    pub relayer: Pubkey,
}

#[derive(Accounts)]
pub struct InitFunder<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = FunderState::LEN,
        seeds = [b"funder"],
        bump,
    )]
    pub funder_pda: Account<'info, FunderState>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitFunder>, args: InitFunderArgs) -> Result<()> {
    let funder = &mut ctx.accounts.funder_pda;
    funder.admin = ctx.accounts.admin.key();
    funder.relayer = args.relayer;
    funder.bump = ctx.bumps.funder_pda;

    msg!(
        "FunderPDA initialized. Admin: {}. Relayer: {}",
        ctx.accounts.admin.key(),
        args.relayer
    );

    Ok(())
}
