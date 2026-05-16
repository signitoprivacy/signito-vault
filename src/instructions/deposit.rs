use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    program::{invoke, invoke_signed},
    system_instruction,
};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{PoolState, UserState};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct DepositArgs {
    pub amount: u64,
}

// Deposit: add more SOL to an existing user position and mint more sSOL.
//
// TWO-ACTOR DESIGN (mirrors shield):
//   owner         = freshDepositWallet (server-controlled ephemeral keypair)
//                   Pays the deposit SOL. User only signs a plain SystemProgram.transfer.
//   display_owner = user's real connected wallet (NON-signer)
//                   Verified against stoken_ata authority to confirm vault ownership.
//
// Authorization: stoken_ata.authority must equal display_owner.key().
// This works for both new vaults (authority = display_owner set during shield) and
// legacy vaults (authority = original user wallet, pass display_owner = userWallet).
#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: user's real wallet; must match stoken_ata authority. NOT a signer.
    pub display_owner: UncheckedAccount<'info>,

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

    /// CHECK: user's existing stoken_ata (random address from shield)
    #[account(mut)]
    pub stoken_ata: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"user_state", stoken_ata.key().as_ref()],
        bump = user_state.bump,
        has_one = stoken_ata,
    )]
    pub user_state: Account<'info, UserState>,

    pub system_program: Program<'info, System>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<Deposit>, args: DepositArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);

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

    let pool_bump = ctx.accounts.pool_pda.bump;
    let pool_key = ctx.accounts.pool_pda.key();
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[pool_bump]]];

    {
        let pool = &mut ctx.accounts.pool_pda;
        pool.total_deposited = pool
            .total_deposited
            .checked_add(args.amount)
            .ok_or(SignitoError::Overflow)?;
    }

    {
        let user_state = &mut ctx.accounts.user_state;
        user_state.deposited = user_state
            .deposited
            .checked_add(args.amount)
            .ok_or(SignitoError::Overflow)?;
    }

    // Transfer SOL from owner (freshDepositWallet) to pool
    invoke(
        &system_instruction::transfer(ctx.accounts.owner.key, &pool_key, args.amount),
        &[
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    // Mint sSOL to the user's existing stoken_ata.
    // pool_pda is mint_authority (set during initialize_pool with freeze_authority=None).
    // No freeze/thaw needed: the mint has no freeze_authority.
    invoke_signed(
        &spl_token_2022::instruction::mint_to(
            &TOKEN_2022_ID,
            ctx.accounts.mint_stoken.key,
            ctx.accounts.stoken_ata.key,
            &pool_key,
            &[],
            args.amount,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.mint_stoken.to_account_info(),
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
        ],
        pool_seeds,
    )?;

    msg!(
        "Deposit: {} lamports added. Total: {}",
        args.amount,
        ctx.accounts.user_state.deposited
    );

    Ok(())
}
