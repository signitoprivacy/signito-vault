use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, program::invoke_signed, pubkey, system_instruction};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::PoolState;

// Only the deployer (upgrade authority) can rotate the mint.
const DEPLOYER: Pubkey = pubkey!("BNzyXaTXopiCCffJ6Ee7XCvPiXwVxEVThteN8S7kBMge");

// getMintLen([ExtensionType.NonTransferable]) = 170 (verified via @solana/spl-token)
const MINT_LEN: usize = 170;

#[derive(Accounts)]
pub struct RotateMint<'info> {
    #[account(mut, address = DEPLOYER)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool"],
        bump = pool_pda.bump,
    )]
    pub pool_pda: Account<'info, PoolState>,

    /// CHECK: fresh keypair; will be initialized as new NonTransferable sSOL mint
    #[account(mut)]
    pub new_mint: Signer<'info>,

    pub system_program: Program<'info, System>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<RotateMint>) -> Result<()> {
    let pool_key = ctx.accounts.pool_pda.key();
    let bump = ctx.accounts.pool_pda.bump;

    let rent = Rent::get()?;
    let mint_lamports = rent.minimum_balance(MINT_LEN);

    // Create the new mint account
    invoke(
        &system_instruction::create_account(
            ctx.accounts.payer.key,
            ctx.accounts.new_mint.key,
            mint_lamports,
            MINT_LEN as u64,
            &TOKEN_2022_ID,
        ),
        &[
            ctx.accounts.payer.to_account_info(),
            ctx.accounts.new_mint.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    // Initialize NonTransferable extension BEFORE the mint.
    // sSOL becomes soulbound: shows in Phantom, cannot be sent to any wallet.
    invoke(
        &spl_token_2022::instruction::initialize_non_transferable_mint(
            &TOKEN_2022_ID,
            ctx.accounts.new_mint.key,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[ctx.accounts.new_mint.to_account_info()],
    )?;

    // Initialize mint: mint_authority = pool_pda, no freeze_authority needed
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[bump]]];
    invoke_signed(
        &spl_token_2022::instruction::initialize_mint2(
            &TOKEN_2022_ID,
            ctx.accounts.new_mint.key,
            &pool_key,
            None,
            9,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[ctx.accounts.new_mint.to_account_info()],
        pool_seeds,
    )?;

    let old_mint = ctx.accounts.pool_pda.mint_stoken;

    // Update pool_pda to point to the new NonTransferable mint
    ctx.accounts.pool_pda.mint_stoken = ctx.accounts.new_mint.key();

    msg!(
        "Mint rotated. Old: {}. New (NonTransferable): {}.",
        old_mint,
        ctx.accounts.new_mint.key()
    );

    Ok(())
}
