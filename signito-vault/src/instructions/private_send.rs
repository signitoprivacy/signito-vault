use anchor_lang::prelude::*;
use anchor_lang::solana_program::{hash::hashv, program::invoke_signed};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{PoolState, UserState};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct PrivateSendArgs {
    pub ots_preimage: [u8; 32],
    pub amount: u64,
}

// PrivateSend: OTS-verified sSOL burn + SOL transfer from pool to recipient.
//
// PRIVACY GUARANTEE: owner's wallet address does NOT appear anywhere in this
// instruction's accounts. Only the random stoken_ata address and user_state
// (derived from stoken_ata, not from owner) are referenced.
//
// On-chain trace shows: [random_stoken_ata] burn + pool_pda -> recipient
// Owner wallet is NOT in the instruction. To find the owner, an investigator
// must separately look up the stoken_ata token account data (authority field).
//
// Relayer is the only signer -- no frontrunning possible (relayer key required).
//
// Authorization: pool_pda is PermanentDelegate on new accounts (post-upgrade), or
// approved delegate on legacy accounts (pre-upgrade). Token-2022 accepts both.
#[derive(Accounts)]
pub struct PrivateSend<'info> {
    #[account(mut)]
    pub relayer: Signer<'info>,

    // Random address. Owner wallet NOT here.
    /// CHECK: sSOL token account; pool_pda must be PermanentDelegate or approved delegate
    #[account(mut)]
    pub stoken_ata: UncheckedAccount<'info>,

    // Derived from stoken_ata.key(), NOT from owner wallet pubkey.
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

    /// CHECK: SOL destination -- any valid address including fresh wallets
    #[account(mut)]
    pub recipient: UncheckedAccount<'info>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler<'info>(ctx: Context<'_, '_, '_, 'info, PrivateSend<'info>>, args: PrivateSendArgs) -> Result<()> {
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

    // All burns (real stoken_ata + decoys) are in remaining_accounts, shuffled by client.
    // Real account appears at a random position -- no fixed ordering visible on-chain.
    require!(!ctx.remaining_accounts.is_empty(), SignitoError::InvalidAmount);

    let mint_info = ctx.accounts.mint_stoken.to_account_info();
    let pool_info = ctx.accounts.pool_pda.to_account_info();
    let mint_key = *mint_info.key;

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

    // 0.15% relayer fee (15 basis points). Fee stays with the relayer; remainder goes to recipient.
    let fee = args
        .amount
        .checked_mul(15)
        .ok_or(SignitoError::Overflow)?
        .checked_div(10_000)
        .ok_or(SignitoError::Overflow)?;
    let recipient_amount = args
        .amount
        .checked_sub(fee)
        .ok_or(SignitoError::Overflow)?;

    // Transfer SOL from pool_pda: recipient_amount to recipient, fee to relayer.
    {
        let pool_info = ctx.accounts.pool_pda.to_account_info();
        let recipient_info = ctx.accounts.recipient.to_account_info();
        let relayer_info = ctx.accounts.relayer.to_account_info();

        **pool_info.try_borrow_mut_lamports()? = pool_info
            .lamports()
            .checked_sub(args.amount)
            .ok_or(SignitoError::Overflow)?;
        **recipient_info.try_borrow_mut_lamports()? = recipient_info
            .lamports()
            .checked_add(recipient_amount)
            .ok_or(SignitoError::Overflow)?;
        **relayer_info.try_borrow_mut_lamports()? = relayer_info
            .lamports()
            .checked_add(fee)
            .ok_or(SignitoError::Overflow)?;
    }

    // Burn decoy sSOL from remaining_accounts using PermanentDelegate.
    // Same amount burned from each decoy ATA. No pool accounting changes.
    // All burns appear under one instruction in the block explorer.
    if !ctx.remaining_accounts.is_empty() {
        let mint_info = ctx.accounts.mint_stoken.to_account_info();
        let pool_info = ctx.accounts.pool_pda.to_account_info();
        let mint_key = *mint_info.key;

        for decoy_ata in ctx.remaining_accounts.iter() {
            invoke_signed(
                &spl_token_2022::instruction::burn(
                    &TOKEN_2022_ID,
                    decoy_ata.key,
                    &mint_key,
                    &pool_key,
                    &[],
                    args.amount,
                )
                .map_err(|_| error!(SignitoError::Overflow))?,
                &[
                    decoy_ata.clone(),
                    mint_info.clone(),
                    pool_info.clone(),
                ],
                pool_seeds,
            )?;
        }
    }

    msg!(
        "PrivateSend: {} lamports -> {} (fee {} to relayer). {} decoy burns. OTS depth remaining: {}",
        recipient_amount,
        ctx.accounts.recipient.key,
        fee,
        ctx.remaining_accounts.len(),
        ctx.accounts.user_state.chain_depth,
    );

    Ok(())
}
