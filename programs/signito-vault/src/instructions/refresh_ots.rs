use anchor_lang::prelude::*;

use crate::errors::SignitoError;
use crate::state::UserState;

// RefreshOts: reset the OTS chain on an existing user_state.
//
// TWO-ACTOR DESIGN (mirrors shield/deposit):
//   owner         = freshWallet (server-controlled, stored ownerKeypair from DB)
//   display_owner = user's real wallet (NON-signer); must match stoken_ata authority
//
// Server retrieves stored ownerKeypair for this vault, signs as owner.
// User never signs a program instruction.
#[derive(Accounts)]
pub struct RefreshOts<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: user's real wallet; must match stoken_ata authority. NOT a signer.
    pub display_owner: UncheckedAccount<'info>,

    /// CHECK: user's stoken_ata (random address). Used for PDA derivation only.
    pub stoken_ata: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"user_state", stoken_ata.key().as_ref()],
        bump = user_state.bump,
        has_one = stoken_ata,
    )]
    pub user_state: Account<'info, UserState>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct RefreshOtsArgs {
    pub new_ots_tip: [u8; 32],
    pub new_chain_depth: u8,
}

pub fn handler(ctx: Context<RefreshOts>, args: RefreshOtsArgs) -> Result<()> {
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

    require!(
        args.new_chain_depth > 0 && args.new_chain_depth <= 64,
        SignitoError::InvalidAmount
    );

    let user_state = &mut ctx.accounts.user_state;
    user_state.current_ots_hash = args.new_ots_tip;
    user_state.chain_depth = args.new_chain_depth;

    msg!(
        "OTS refreshed. New depth: {}",
        args.new_chain_depth
    );

    Ok(())
}
