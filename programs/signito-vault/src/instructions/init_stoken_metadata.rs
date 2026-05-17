use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    pubkey,
    sysvar,
};

use crate::constants::{SSOL_NAME, SSOL_SYMBOL, SSOL_URI};
use crate::state::PoolState;

const MPL_METADATA_ID: Pubkey = pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");

#[derive(Accounts)]
pub struct InitStokenMetadata<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        seeds = [b"pool"],
        bump = pool_pda.bump,
    )]
    pub pool_pda: Account<'info, PoolState>,

    /// CHECK: sSOL mint -- must match pool_pda.mint_stoken
    #[account(mut, address = pool_pda.mint_stoken)]
    pub mint_stoken: UncheckedAccount<'info>,

    /// CHECK: Metaplex metadata PDA for the sSOL mint (created by this instruction)
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,

    /// CHECK: Metaplex Token Metadata program
    #[account(address = MPL_METADATA_ID)]
    pub mpl_token_metadata: UncheckedAccount<'info>,

    /// CHECK: Sysvar rent
    #[account(address = sysvar::rent::ID)]
    pub rent: UncheckedAccount<'info>,

    /// CHECK: Sysvar instructions -- required by Metaplex createV1
    #[account(address = sysvar::instructions::ID)]
    pub sysvar_instructions: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<InitStokenMetadata>) -> Result<()> {
    let bump = ctx.accounts.pool_pda.bump;
    let mint_key = ctx.accounts.mint_stoken.key();
    let metadata_key = ctx.accounts.metadata.key();
    let pool_key = ctx.accounts.pool_pda.key();
    let payer_key = ctx.accounts.payer.key();

    let ix_data = build_create_v1_data(SSOL_NAME, SSOL_SYMBOL, SSOL_URI);

    // Metaplex createV1 account order (derived from mpl-token-metadata SDK for fungible tokens).
    // For null optional accounts (masterEdition, splTokenProgram), Kinobi uses the program ID
    // as a placeholder. account[1] = masterEdition placeholder, account[8] = splTokenProgram
    // placeholder. The Metaplex program reads the mint's owner to detect Token-2022.
    let accounts = vec![
        AccountMeta::new(metadata_key, false),             // [0] metadata (mut)
        AccountMeta::new_readonly(MPL_METADATA_ID, false), // [1] masterEdition placeholder (null)
        AccountMeta::new(mint_key, false),                 // [2] mint (mut)
        AccountMeta::new_readonly(pool_key, true),         // [3] authority = pool_pda (PDA signer)
        AccountMeta::new(payer_key, true),                 // [4] payer (mut, signer)
        AccountMeta::new_readonly(payer_key, false),       // [5] updateAuthority
        AccountMeta::new_readonly(anchor_lang::solana_program::system_program::ID, false), // [6]
        AccountMeta::new_readonly(sysvar::instructions::ID, false), // [7] sysvarInstructions
        AccountMeta::new_readonly(MPL_METADATA_ID, false), // [8] splTokenProgram placeholder (null)
    ];

    let ix = Instruction {
        program_id: MPL_METADATA_ID,
        accounts,
        data: ix_data,
    };

    let seeds: &[&[u8]] = &[b"pool", &[bump]];

    invoke_signed(
        &ix,
        &[
            ctx.accounts.metadata.to_account_info(),
            ctx.accounts.mpl_token_metadata.to_account_info(), // masterEdition placeholder
            ctx.accounts.mint_stoken.to_account_info(),
            ctx.accounts.pool_pda.to_account_info(),
            ctx.accounts.payer.to_account_info(),
            ctx.accounts.payer.to_account_info(),              // updateAuthority (same as payer)
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.sysvar_instructions.to_account_info(),
            ctx.accounts.mpl_token_metadata.to_account_info(), // splTokenProgram placeholder
        ],
        &[seeds],
    )?;

    msg!(
        "sSOL token metadata initialized. Mint: {}. Name: {}. Symbol: {}.",
        mint_key,
        SSOL_NAME,
        SSOL_SYMBOL,
    );

    Ok(())
}

// Build Metaplex createV1 instruction data (borsh-encoded).
// Format matches mpl-token-metadata SDK createFungible with TokenStandard::Fungible.
// discriminator=42, createV1Discriminator=0, name, symbol, uri, sellerFeeBasisPoints=0,
// creators=None, primarySaleHappened=false, isMutable=true, tokenStandard=2 (Fungible),
// collection=None, uses=None, collectionDetails=None, ruleSet=None, decimals=Some(9),
// printSupply=None
fn build_create_v1_data(name: &str, symbol: &str, uri: &str) -> Vec<u8> {
    let mut data = Vec::new();

    // Discriminant: [42, 0]
    data.push(42u8);
    data.push(0u8);

    // name (borsh String: u32_le len + bytes)
    let nb = name.as_bytes();
    data.extend_from_slice(&(nb.len() as u32).to_le_bytes());
    data.extend_from_slice(nb);

    // symbol
    let sb = symbol.as_bytes();
    data.extend_from_slice(&(sb.len() as u32).to_le_bytes());
    data.extend_from_slice(sb);

    // uri
    let ub = uri.as_bytes();
    data.extend_from_slice(&(ub.len() as u32).to_le_bytes());
    data.extend_from_slice(ub);

    // sellerFeeBasisPoints: u16 = 0
    data.extend_from_slice(&0u16.to_le_bytes());

    // creators: Option<Vec<Creator>> = None
    data.push(0u8);

    // primarySaleHappened: bool = false
    data.push(0u8);

    // isMutable: bool = true
    data.push(1u8);

    // tokenStandard: u8 = 2 (Fungible)
    data.push(2u8);

    // collection: Option<Collection> = None
    data.push(0u8);

    // uses: Option<Uses> = None
    data.push(0u8);

    // collectionDetails: Option<CollectionDetails> = None
    data.push(0u8);

    // ruleSet: Option<Pubkey> = None
    data.push(0u8);

    // decimals: Option<u8> = Some(9)
    data.push(1u8);
    data.push(9u8);

    // printSupply: Option<PrintSupply> = None
    data.push(0u8);

    data
}
