use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    program::{invoke, invoke_signed},
    system_instruction,
};

use crate::constants::TOKEN_2022_ID;
use crate::errors::SignitoError;
use crate::state::{FunderState, PoolState, UserState};

// sSOL token account size -- same layout as real accounts created by shield.
// ImmutableOwner (4) + NonTransferableAccount (4) + base (166) = 174 bytes.
const TOKEN_ACCOUNT_LEN: usize = 174;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct DecoyShieldArgs {
    pub ots_tip: [u8; 32],
    pub chain_depth: u8,
    pub amount: u64,
}

// Create a decoy stoken_ata + user_state PDA that is structurally identical
// to a real shield account, without depositing any SOL into the pool.
//
// Included in the same transaction as a real user's shield.
// Observer sees N+1 new sSOL accounts created in one block and cannot
// identify which belongs to the real user.
//
// The decoy stoken_ata has a real Solana wallet pubkey as its authority
// (display_owner), chosen from a pool of active chain addresses.
// The user_state PDA is initialised with plausible dummy OTS data.
// pool.total_deposited is NOT incremented -- no real SOL enters the pool.
// Only the authorized relayer can call this instruction.
//
// Accounts:
//   0. relayer          writable signer (must match funder_pda.relayer, pays rent)
//   1. funder_pda       readonly PDA (seeds=[b"funder"], auth check)
//   2. display_owner    readonly (random real Solana wallet pubkey, non-signer)
//   3. pool_pda         writable PDA (seeds=[b"pool"])
//   4. mint_stoken      writable (shared sSOL mint)
//   5. stoken_ata       writable signer (fresh keypair, becomes decoy token account)
//   6. user_state       writable (Anchor init PDA, seeds=[b"user_state", stoken_ata])
//   7. system_program   readonly
//   8. token_prog_22    readonly
#[derive(Accounts)]
pub struct DecoyShield<'info> {
    #[account(
        mut,
        constraint = relayer.key() == funder_pda.relayer @ SignitoError::Unauthorized
    )]
    pub relayer: Signer<'info>,

    #[account(
        seeds = [b"funder"],
        bump = funder_pda.bump,
    )]
    pub funder_pda: Account<'info, FunderState>,

    /// CHECK: random real Solana wallet, set as stoken_ata authority so the
    /// decoy account looks identical to a real user account. Non-signer.
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

    /// CHECK: fresh keypair; allocated and initialised as sSOL token account in handler
    #[account(mut)]
    pub stoken_ata: Signer<'info>,

    #[account(
        init,
        payer = relayer,
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

pub fn handler(ctx: Context<DecoyShield>, args: DecoyShieldArgs) -> Result<()> {
    require!(args.amount > 0, SignitoError::InvalidAmount);
    require!(
        args.chain_depth > 0 && args.chain_depth <= 64,
        SignitoError::InvalidAmount
    );

    let pool_bump = ctx.accounts.pool_pda.bump;
    let pool_key = ctx.accounts.pool_pda.key();
    let pool_seeds: &[&[&[u8]]] = &[&[b"pool", &[pool_bump]]];

    // Initialise user_state with plausible dummy OTS data.
    // Structurally identical to a real user_state -- indistinguishable on-chain.
    {
        let user_state = &mut ctx.accounts.user_state;
        user_state.stoken_ata = ctx.accounts.stoken_ata.key();
        user_state.current_ots_hash = args.ots_tip;
        user_state.chain_depth = args.chain_depth;
        user_state.deposited = args.amount;
        user_state.bump = ctx.bumps.user_state;
    }

    // pool.total_deposited intentionally NOT updated -- no real SOL entering pool.

    let rent = Rent::get()?;
    let account_lamports = rent.minimum_balance(TOKEN_ACCOUNT_LEN);

    // 1. Allocate space for stoken_ata (relayer pays rent, not a freshWallet)
    invoke(
        &system_instruction::create_account(
            ctx.accounts.relayer.key,
            ctx.accounts.stoken_ata.key,
            account_lamports,
            TOKEN_ACCOUNT_LEN as u64,
            &TOKEN_2022_ID,
        ),
        &[
            ctx.accounts.relayer.to_account_info(),
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

    // 3. Initialize the token account with pool_pda as temporary owner.
    //    We will set close_authority = pool_pda, then transfer ownership to display_owner.
    //    This lets pool_pda close the account later and reclaim rent without display_owner signing.
    invoke(
        &spl_token_2022::instruction::initialize_account3(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
            ctx.accounts.mint_stoken.key,
            &pool_key, // temporary owner: pool_pda
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.mint_stoken.to_account_info(),
        ],
    )?;

    // 3b. Set pool_pda as close_authority before transferring ownership.
    //     pool_pda is currently the account owner, so it signs this via PDA seeds.
    //     This authority persists after ownership is transferred to display_owner.
    invoke_signed(
        &spl_token_2022::instruction::set_authority(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
            Some(&pool_key),
            spl_token_2022::instruction::AuthorityType::CloseAccount,
            &pool_key, // current owner signs
            &[],
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
        ],
        pool_seeds,
    )?;

    // 3c. Transfer account ownership to display_owner (random real wallet).
    //     pool_pda signs as current owner. After this, the on-chain account looks identical
    //     to a real user account: owner = display_owner. But pool_pda retains close_authority
    //     (set above -- owner changes do not affect close_authority in Token-2022).
    invoke_signed(
        &spl_token_2022::instruction::set_authority(
            &TOKEN_2022_ID,
            ctx.accounts.stoken_ata.key,
            Some(ctx.accounts.display_owner.key),
            spl_token_2022::instruction::AuthorityType::AccountOwner,
            &pool_key, // current owner signs
            &[],
        )
        .map_err(|_| error!(SignitoError::Overflow))?,
        &[
            ctx.accounts.stoken_ata.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
        ],
        pool_seeds,
    )?;

    // 4. Mint phantom sSOL to decoy stoken_ata.
    //    pool_pda is mint authority (signs via PDA seeds).
    //    No SOL enters the pool -- purely phantom supply for the mix layer.
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

    msg!(
        "DecoyShield: decoy stoken_ata {} created. display_owner: {}. OTS depth: {}. amount: {}",
        ctx.accounts.stoken_ata.key(),
        ctx.accounts.display_owner.key(),
        args.chain_depth,
        args.amount,
    );

    Ok(())
}
