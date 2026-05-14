use anchor_lang::prelude::*;

use crate::state::FunderState;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct SetRelayerArgs {
    pub new_relayer: Pubkey,
}

// Admin-only: rotate the authorized relayer pubkey stored in FunderPDA.
#[derive(Accounts)]
pub struct SetRelayer<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"funder"],
        bump = funder_pda.bump,
        has_one = admin,
    )]
    pub funder_pda: Account<'info, FunderState>,
}

pub fn handler(ctx: Context<SetRelayer>, args: SetRelayerArgs) -> Result<()> {
    let old = ctx.accounts.funder_pda.relayer;
    ctx.accounts.funder_pda.relayer = args.new_relayer;

    msg!(
        "FunderPDA: relayer rotated from {} to {}",
        old,
        args.new_relayer
    );

    Ok(())
}
