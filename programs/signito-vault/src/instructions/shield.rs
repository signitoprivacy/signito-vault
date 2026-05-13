use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    program::{invoke, invoke_signed},
    system_instruction,
};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{PoolState, UserState};

const TOKEN_ACCOUNT_LEN: usize = 165;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ShieldArgs {
    pub ots_tip: [u8; 32],
    pub chain_depth: u8,
    pub amount: u64,
}

// Shield: deposit SOL into the shared pool and mint sSOL to user's token account.
//
// stoken_ata is a FRESH KEYPAIR (random address, not derived from owner wallet).
// user_state is derived from stoken_ata.key(), NOT from owner.key().
// This means private_send transactions will not include owner.key() in their accounts.
//
// The stoken_ata authority is set to owner.key() so it shows up in Phantom/Solflare.
// pool_pda is approved as delegate so it can burn sSOL in private_send without owner sig.
#[derive(Accounts)]
pub struct Shield<'info> {
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

    /// CHECK: fresh keypair; created and initialized as sSOL token account in handler
    #[account(mut)]
    pub stoken_ata: Signer<'info>,

    #[account(
        init,
        payer = owner,
        space = UserState::LEN,
        seeds = [b"user_state", stoken_ata.key().as_ref()],
        bump,
    )]
    pub user_state: Account<'info, UserState>,

    pub system_program: Program<'info, System>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<Shield>, args: ShieldArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);
    require!(
        args.chain_depth > 0 && args.chain_depth <= 64,
        SignitoError::InvalidAmount
    );

    let pool_bump = ctx.accounts.pool_pda.bump;
    let pool_key = ctx.accounts.pool_pda.key();
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[pool_bump]]];

    {
        let user_state = &mut ctx.accounts.user_state;
        user_state.stoken_ata = ctx.accounts.stoken_ata.key();
        user_state.current_ots_hash = args.ots_tip;
        user_state.chain_depth = args.chain_depth;
        user_state.deposited = args.amount;
        user_state.bump = ctx.bumps.user_state;
    }

    {
        let pool = &mut ctx.accounts.pool_pda;
        pool.total_deposited = pool
            .total_deposited
            .checked_add(args.amount)
            .ok_or(SignitoError::Overflow)?;
    }

    let rent = Rent::get()?;
    let account_lamports = rent.minimum_balance(TOKEN_ACCOUNT_LEN);

    invoke(
        &system_instruction::create_account(
            ctx.accounts.owner.key,
            ctx.accounts.stoken_ata.key,
            account_lamports,
            TOKEN_ACCOUNT_LEN as u64,
            &TOKEN_2022_ID,
        ),
        &[
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    // authority = owner.key() so sSOL is visible in Phantom/Solflare
    invoke(
        &spl_token_2022::instruction::initialize_account3(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
            ctx.accounts.mint_stoken.key,
            ctx.accounts.owner.key,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.mint_stoken.to_account_info(),
        ],
    )?;

    // Transfer SOL from owner to pool_pda
    invoke(
        &system_instruction::transfer(ctx.accounts.owner.key, &pool_key, args.amount),
        &[
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    // Mint sSOL (pool_pda is mint authority)
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

    // Approve pool_pda as delegate so it can burn sSOL in private_send without owner sig
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

    // Freeze stoken_ata (pool_pda is freeze authority) -- prevents standard wallet transfers
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
        "Shield: {} lamports into pool. stoken_ata: {}. OTS depth: {}",
        args.amount,
        ctx.accounts.stoken_ata.key(),
        args.chain_depth
    );

    Ok(())
}
