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

// Deposit: add more SOL to an existing user's pool position and mint more sSOL.
// Owner must sign (they are transferring SOL). Owner is visible in this tx,
// same as the initial shield -- this is acceptable for deposit operations.
#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

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

    // Verify stoken_ata is owned by this wallet (authority at offset 32..64)
    {
        let data = ctx.accounts.stoken_ata.data.borrow();
        require!(data.len() >= 64, SignitoError::Unauthorized);
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&data[32..64]);
        require!(
            Pubkey::from(key_bytes) == ctx.accounts.owner.key(),
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

    invoke(
        &system_instruction::transfer(ctx.accounts.owner.key, &pool_key, args.amount),
        &[
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    let is_frozen = ctx
        .accounts
        .stoken_ata
        .data
        .borrow()
        .get(108)
        .copied()
        == Some(2);

    if is_frozen {
        invoke_signed(
            &spl_token_2022::instruction::thaw_account(
                &TOKEN_2022_ID,
                ctx.accounts.stoken_ata.key,
                ctx.accounts.mint_stoken.key,
                &pool_key,
                &[],
            )
            .map_err(|_| error!(SignitoError::Overflow))?,
            &[
                ctx.accounts.stoken_ata.to_account_info(),
                ctx.accounts.mint_stoken.to_account_info(),
                ctx.accounts.pool_pda.to_account_info(),
            ],
            pool_seeds,
        )?;
    }

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

    // Re-approve pool_pda as delegate after minting more sSOL
    invoke(
        &spl_token_2022::instruction::approve(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
            &pool_key,
            ctx.accounts.owner.key,
            &[],
            u64::MAX,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
            ctx.accounts.owner.to_account_info(),
        ],
    )?;

    invoke_signed(
        &spl_token_2022::instruction::freeze_account(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
            ctx.accounts.mint_stoken.key,
            &pool_key,
            &[],
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.mint_stoken.to_account_info(),
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
