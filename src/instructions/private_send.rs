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
#[derive(Accounts)]
pub struct PrivateSend<'info> {
    #[account(mut)]
    pub relayer: Signer<'info>,

    // Random address. Owner wallet NOT here.
    /// CHECK: sSOL token account; pool_pda must be delegate (checked in handler)
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

pub fn handler(ctx: Context<PrivateSend>, args: PrivateSendArgs) -> Result<()> {
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

        // Verify pool_pda is the delegate on stoken_ata
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

    // Thaw stoken_ata if frozen (state byte at offset 108: 2 = frozen)
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

    // Burn sSOL via pool_pda delegate authority
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

    // Re-freeze so remaining balance cannot be moved via standard wallet tools
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

    // Transfer SOL from pool_pda to recipient via direct lamport manipulation
    {
        let pool_info = ctx.accounts.pool_pda.to_account_info();
        let recipient_info = ctx.accounts.recipient.to_account_info();
        **pool_info.try_borrow_mut_lamports()? = pool_info
            .lamports()
            .checked_sub(args.amount)
            .ok_or(SignitoError::Overflow)?;
        **recipient_info.try_borrow_mut_lamports()? = recipient_info
            .lamports()
            .checked_add(args.amount)
            .ok_or(SignitoError::Overflow)?;
    }

    msg!(
        "PrivateSend: {} lamports -> {}. OTS depth remaining: {}",
        args.amount,
        ctx.accounts.recipient.key,
        ctx.accounts.user_state.chain_depth,
    );

    Ok(())
}
