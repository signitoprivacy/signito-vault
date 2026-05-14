use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    program::{invoke, invoke_signed},
    system_instruction,
};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{PoolState, UserState};

// Token account size for:
//   ImmutableOwner         (0 bytes data + 4 TLV header = 4)  type=7
//   NonTransferableAccount (0 bytes data + 4 TLV header = 4, added auto by init_account3) type=13
// Base account: 165 bytes + 1 account_type byte = 166
// Total: 166 + 4 + 4 = 174
//
// Note: PermanentDelegate is a MINT-level extension (type=12) set on the mint by rotate_mint.
// It must NOT be set on individual token accounts.
const TOKEN_ACCOUNT_LEN: usize = 174;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ShieldArgs {
    pub ots_tip: [u8; 32],
    pub chain_depth: u8,
    pub amount: u64,
}

// Shield: deposit SOL into the shared pool and mint sSOL to user's token account.
//
// TWO-ACTOR DESIGN (no Phantom unknown-program warning):
//   owner         = freshWallet (server-controlled ephemeral keypair)
//                   Pays SOL: shield amount + account rents.
//                   Never held by the user. Phantom never signs a program ix.
//   display_owner = user's real connected wallet (NON-signer, readonly)
//                   Set as stoken_ata authority so sSOL appears in Phantom/Solflare.
//
// User only signs a plain SystemProgram.transfer to freshWallet (shown in Phantom
// as "Send X SOL"). Server then calls this instruction with freshWallet signing.
//
// stoken_ata is a FRESH KEYPAIR (random address, NOT wallet-derived).
// user_state PDA is derived from stoken_ata.key(), NOT from any wallet pubkey.
//
// pool_pda is set as PermanentDelegate on stoken_ata so it can burn sSOL in
// burn_and_queue / private_send without any wallet signature, and without
// needing a standard approve (which would require the authority to sign).
#[derive(Accounts)]
pub struct Shield<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: user's real wallet address. Set as stoken_ata authority so sSOL
    /// appears in Phantom/Solflare under the user's wallet. NOT a signer --
    /// the server-controlled freshWallet (owner) pays all SOL.
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

    // 1. Allocate space for stoken_ata (owner = freshWallet pays rent)
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

    // 2. Register ImmutableOwner extension (must be before initialize_account3)
    invoke(
        &spl_token_2022::instruction::initialize_immutable_owner(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[ctx.accounts.stoken_ata.to_account_info()],
    )?;

    // 3. Initialize the token account.
    //    authority = display_owner.key() so sSOL is visible in Phantom/Solflare
    //    under the user's real wallet address, even though display_owner did not sign.
    invoke(
        &spl_token_2022::instruction::initialize_account3(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
            ctx.accounts.mint_stoken.key,
            ctx.accounts.display_owner.key,
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.mint_stoken.to_account_info(),
        ],
    )?;

    // 5. Transfer SOL from owner (freshWallet) to pool_pda
    invoke(
        &system_instruction::transfer(ctx.accounts.owner.key, &pool_key, args.amount),
        &[
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
        ],
    )?;

    // 6. Mint sSOL to stoken_ata (pool_pda is mint authority, signs via PDA seeds)
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

    // No approve needed: pool_pda is PermanentDelegate (set in step 3) and can burn
    // sSOL at any time without the authority (display_owner) signing anything.

    msg!(
        "Shield: {} lamports. stoken_ata: {}. display_owner: {}. OTS depth: {}",
        args.amount,
        ctx.accounts.stoken_ata.key(),
        ctx.accounts.display_owner.key(),
        args.chain_depth,
    );

    Ok(())
}
