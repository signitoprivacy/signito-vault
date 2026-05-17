use anchor_lang::prelude::*;
use anchor_lang::solana_program::{hash::hashv, program::invoke_signed};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{PoolState, UserState};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct BurnAndQueueArgs {
    pub ots_preimage: [u8; 32],
    pub amount: u64,
}

// TX1 of the 2-TX StealthSend flow.
//
// Burns sSOL from the user's stoken_ata (OTS-verified) and reduces pool accounting.
// Does NOT include a recipient address on-chain: the recipient is held off-chain
// by the Signito relayer server and submitted in TX2 (process_queue).
//
// Signed by the ephemeral fresh_wallet (funded by FunderPDA, discarded after TX1).
// Owner wallet does NOT appear anywhere in this instruction's accounts.
// Recipient wallet does NOT appear anywhere in this instruction's accounts.
//
// On-chain trace from TX1: fresh_wallet -> stoken_ata -> user_state -> pool_pda
// On-chain trace from TX2: relayer -> pool_pda -> recipient
// No common account between TX1 and TX2.
//
// Authorization: pool_pda is PermanentDelegate on new accounts (post-upgrade), or
// approved delegate on legacy accounts (pre-upgrade). Token-2022 accepts both.
#[derive(Accounts)]
pub struct BurnAndQueue<'info> {
    #[account(mut)]
    pub fresh_wallet: Signer<'info>,

    /// CHECK: sSOL token account; pool_pda must be PermanentDelegate or approved delegate
    #[account(mut)]
    pub stoken_ata: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"user_state", stoken_ata.key().as_ref()],
        bump = user_state.bump,
        has_one = stoken_ata,
    )]
    pub user_state: Account<'info, UserState>,

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

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,
}

pub fn handler<'info>(ctx: Context<'_, '_, '_, 'info, BurnAndQueue<'info>>, args: BurnAndQueueArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);

    let computed = hashv(&[args.ots_preimage.as_ref()]);
    let pool_bump = ctx.accounts.pool_pda.bump;
    let pool_key = ctx.accounts.pool_pda.key();
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[pool_bump]]];

    {
        let user_state = &mut ctx.accounts.user_state;

        require!(
            computed.to_bytes() == user_state.current_ots_hash,
            SignitoError::InvalidOtsPreimage
        );
        require!(user_state.chain_depth > 0, SignitoError::VaultExhausted);
        require!(
            args.amount <= user_state.deposited,
            SignitoError::InsufficientFunds
        );

        user_state.current_ots_hash = args.ots_preimage;
        user_state.chain_depth = user_state
            .chain_depth
            .checked_sub(1)
            .ok_or(SignitoError::Overflow)?;
        user_state.deposited = user_state
            .deposited
            .checked_sub(args.amount)
            .ok_or(SignitoError::Overflow)?;
    }

    {
        let pool = &mut ctx.accounts.pool_pda;
        pool.total_deposited = pool
            .total_deposited
            .checked_sub(args.amount)
            .ok_or(SignitoError::Overflow)?;
    }

    // All burns (real stoken_ata + decoys) are passed as remaining_accounts by the client.
    // The client shuffles the list so the real account appears at a random position.
    // This instruction iterates remaining_accounts in the provided order, so the block
    // explorer shows all burns interleaved with no fixed position for the real account.
    //
    // The fixed stoken_ata account above is used only for OTS verification and state
    // updates. The actual burn of stoken_ata happens here via remaining_accounts.
    require!(!ctx.remaining_accounts.is_empty(), SignitoError::InvalidAmount);

    let mint_info = ctx.accounts.mint_stoken.to_account_info();
    let pool_info = ctx.accounts.pool_pda.to_account_info();
    let mint_key = *mint_info.key;
    let total = ctx.remaining_accounts.len();

    for acct in ctx.remaining_accounts.iter() {
        invoke_signed(
            &spl_token_2022::instruction::burn(
                &TOKEN_2022_ID,
                acct.key,
                &mint_key,
                &pool_key,
                &[],
                args.amount,
            )
            .map_err(|_| error!(SignitoError::Overflow))?,
            &[
                acct.clone(),
                mint_info.clone(),
                pool_info.clone(),
            ],
            pool_seeds,
        )?;
    }

    msg!(
        "BurnAndQueue: {} lamports burned across {} accounts (real position randomized). OTS depth remaining: {}. Awaiting relay in TX2.",
        args.amount,
        total,
        ctx.accounts.user_state.chain_depth,
    );

    Ok(())
}
