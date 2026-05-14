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
#[derive(Accounts)]
pub struct BurnAndQueue<'info> {
    #[account(mut)]
    pub fresh_wallet: Signer<'info>,

    /// CHECK: sSOL token account; pool_pda must be delegate (verified in handler)
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

pub fn handler(ctx: Context<BurnAndQueue>, args: BurnAndQueueArgs) -> Result<()> {
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

        // Verify pool_pda is the delegate on stoken_ata.
        // Token account layout: delegate_option at [72..76], delegate pubkey at [76..108]
        let data = ctx.accounts.stoken_ata.data.borrow();
        require!(data.len() >= 108, SignitoError::Unauthorized);

        let delegate_option = u32::from_le_bytes(
            data[72..76]
                .try_into()
                .map_err(|_| error!(SignitoError::Unauthorized))?,
        );
        require!(delegate_option == 1, SignitoError::Unauthorized);

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&data[76..108]);
        require!(
            Pubkey::from(key_bytes) == pool_key,
            SignitoError::Unauthorized
        );
        drop(data);

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

    // Burn sSOL via pool_pda delegate authority.
    // NonTransferable mint has no freeze authority -- no thaw needed.
    invoke_signed(
        &spl_token_2022::instruction::burn(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
            ctx.accounts.mint_stoken.key,
            &pool_key,
            &[],
            args.amount,
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
        "BurnAndQueue: {} lamports burned. OTS depth remaining: {}. Awaiting relay in TX2.",
        args.amount,
        ctx.accounts.user_state.chain_depth,
    );

    Ok(())
}
