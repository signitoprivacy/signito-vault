use anchor_lang::prelude::*;
use anchor_lang::solana_program::{hash::hashv, program::invoke_signed};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{AirsignEscrow, PoolState, UserState};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct MintAirsignArgs {
    pub ots_preimage: [u8; 32],
    pub amount: u64,
    pub nonce_hash: [u8; 32],
}

// MintAirsign: OTS-verified sSOL burn + SOL escrow creation for offline voucher.
//
// Privacy model: only relayer signs. stoken_ata is a random address (not wallet).
// Owner wallet does NOT appear in instruction accounts.
// SOL moves from pool_pda to airsign_escrow PDA.
#[derive(Accounts)]
#[instruction(args: MintAirsignArgs)]
pub struct MintAirsign<'info> {
    #[account(mut)]
    pub relayer: Signer<'info>,

    /// CHECK: user's stoken_ata (random address); owner wallet NOT in accounts
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

    #[account(
        init,
        payer = relayer,
        space = AirsignEscrow::LEN,
        seeds = [b"airsign_escrow", args.nonce_hash.as_ref()],
        bump,
    )]
    pub airsign_escrow: Account<'info, AirsignEscrow>,

    /// CHECK: Token-2022 program
    #[account(address = TOKEN_2022_ID)]
    pub token_program_2022: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<MintAirsign>, args: MintAirsignArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);

    let computed = hashv(&[args.ots_preimage.as_ref()]);
    let pool_bump = ctx.accounts.pool_pda.bump;
    let pool_key = ctx.accounts.pool_pda.key();
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[pool_bump]]];

    let issuer_key: Pubkey;

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

        // Verify pool_pda is delegate on stoken_ata
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

        // Read issuer (stoken_ata authority at offset 32..64)
        let mut issuer_bytes = [0u8; 32];
        issuer_bytes.copy_from_slice(&data[32..64]);
        issuer_key = Pubkey::from(issuer_bytes);
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

    // Transfer SOL from pool_pda to airsign_escrow
    {
        let pool_info = ctx.accounts.pool_pda.to_account_info();
        let escrow_info = ctx.accounts.airsign_escrow.to_account_info();
        **pool_info.try_borrow_mut_lamports()? = pool_info
            .lamports()
            .checked_sub(args.amount)
            .ok_or(SignitoError::Overflow)?;
        **escrow_info.try_borrow_mut_lamports()? = escrow_info
            .lamports()
            .checked_add(args.amount)
            .ok_or(SignitoError::Overflow)?;
    }

    let escrow = &mut ctx.accounts.airsign_escrow;
    escrow.issuer = issuer_key;
    escrow.amount = args.amount;
    escrow.nonce_hash = args.nonce_hash;
    escrow.bump = ctx.bumps.airsign_escrow;

    msg!(
        "AirSign minted: {} lamports in escrow {}. OTS depth: {}",
        args.amount,
        ctx.accounts.airsign_escrow.key(),
        ctx.accounts.user_state.chain_depth,
    );

    Ok(())
}
