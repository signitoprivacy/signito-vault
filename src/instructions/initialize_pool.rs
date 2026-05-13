use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, system_instruction};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::PoolState;

const MINT_LEN: usize = 82;

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = PoolState::LEN,
        seeds = [b"pool"],
        bump,
    )]
    pub pool_pda: Account<'info, PoolState>,

    /// CHECK: fresh keypair; created and initialized as shared sSOL mint in handler
    #[account(mut)]
    pub mint_stoken: Signer<'info>,

    pub system_program: Program<'info, System>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<InitializePool>) -> Result<()> {
    let bump = ctx.bumps.pool_pda;
    let pool_key = ctx.accounts.pool_pda.key();

    {
        let pool = &mut ctx.accounts.pool_pda;
        pool.mint_stoken = ctx.accounts.mint_stoken.key();
        pool.total_deposited = 0;
        pool.bump = bump;
    }

    let rent = Rent::get()?;
    let mint_lamports = rent.minimum_balance(MINT_LEN);

    invoke(
        &system_instruction::create_account(
            ctx.accounts.payer.key,
            ctx.accounts.mint_stoken.key,
            mint_lamports,
            MINT_LEN as u64,
            &TOKEN_2022_ID,
        ),
        &[
            ctx.accounts.payer.to_account_info(),
            ctx.accounts.mint_stoken.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    // mint_authority = freeze_authority = pool_pda
    invoke(
        &spl_token_2022::instruction::initialize_mint2(
            &TOKEN_2022_ID,
            ctx.accounts.mint_stoken.key,
            &pool_key,
            Some(&pool_key),
            9,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[ctx.accounts.mint_stoken.to_account_info()],
    )?;

    msg!(
        "Pool initialized. PDA: {}. Shared sSOL mint: {}",
        pool_key,
        ctx.accounts.mint_stoken.key()
    );

    Ok(())
}
