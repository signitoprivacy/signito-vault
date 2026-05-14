use anchor_lang::prelude::*;

use crate::errors::SignitoError;
use crate::state::UserState;

// CloseAccount: close an empty user_state PDA and return rent to display_owner.
//
// TWO-ACTOR DESIGN (mirrors shield/deposit):
//   owner         = freshWallet (server-controlled, stored ownerKeypair from DB)
//   display_owner = user's real wallet (NON-signer, mut); must match stoken_ata authority;
//                   receives the rent lamports when user_state is closed.
//
// stoken_ata must have zero sSOL balance (all burned before calling this).
#[derive(Accounts)]
pub struct CloseAccount<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: user's real wallet; must match stoken_ata authority; receives rent. NOT a signer.
    #[account(mut)]
    pub display_owner: UncheckedAccount<'info>,

    /// CHECK: user's stoken_ata (random address)
    pub stoken_ata: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"user_state", stoken_ata.key().as_ref()],
        bump = user_state.bump,
        has_one = stoken_ata,
        constraint = user_state.deposited == 0 @ SignitoError::AccountNotEmpty,
        close = display_owner,
    )]
    pub user_state: Account<'info, UserState>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<CloseAccount>) -> Result<()> {
    // Verify stoken_ata is owned by display_owner (authority at offset 32..64)
    {
        let data = ctx.accounts.stoken_ata.data.borrow();
        require!(data.len() >= 64, SignitoError::Unauthorized);
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&data[32..64]);
        require!(
            Pubkey::from(key_bytes) == ctx.accounts.display_owner.key(),
            SignitoError::Unauthorized
        );
    }

    msg!("Account closed. Rent returned to display_owner.");
    Ok(())
}
