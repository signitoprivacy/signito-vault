use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    ed25519_program,
    hash::hashv,
    sysvar::instructions::{load_instruction_at_checked, ID as INSTRUCTIONS_ID},
};

use crate::errors::SignitoError;
use crate::state::AirsignEscrow;

// Binary voucher message format (57 bytes), signed offline by the issuer wallet:
//   [0]      domain separator: 0x53 ('S') -- prevents Phantom from treating this as a tx
//   [1..9]   amount: u64 LE (lamports)
//   [9..41]  recipient: Pubkey (32 bytes) -- SOL destination, tamper-proof via Ed25519
//   [41..57] nonce: [u8; 16]             -- sha256(nonce) was used as escrow PDA seed
//
// The 0x53 prefix ensures the first byte is always < 0x80 so Phantom's signMessage
// never misidentifies the binary message as a versioned Solana transaction.
// The issuer calls wallet.signMessage(voucher_msg) offline (no internet required).
// The 64-byte Ed25519 signature locks the recipient: any tampering invalidates the sig.
pub const AIRSIGN_VOUCHER_MSG_LEN: usize = 57;
pub const AIRSIGN_DOMAIN_SEP: u8 = 0x53; // 'S'

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ClaimAirsignArgs {
    // sha256(nonce from voucher_msg[40..56]) -- used to derive the escrow PDA
    pub nonce_hash: [u8; 32],
    // 56-byte binary voucher message signed by the issuer
    pub voucher_msg: [u8; AIRSIGN_VOUCHER_MSG_LEN],
    // Ed25519 signature (64 bytes) over voucher_msg by issuer
    pub sig: [u8; 64],
}

#[derive(Accounts)]
#[instruction(args: ClaimAirsignArgs)]
pub struct ClaimAirsign<'info> {
    // Relayer pays the transaction fee. Anyone can trigger a claim.
    #[account(mut)]
    pub relayer: Signer<'info>,

    // Wallet that issued the voucher; verified against escrow.issuer in handler.
    // NOT a signer -- only the relayer signs on-chain.
    /// CHECK: pubkey verified against escrow.issuer inside handler
    pub issuer: UncheckedAccount<'info>,

    // AirsignEscrow PDA created during mint_airsign.
    // close = relayer: after handler, remaining rent lamports go to relayer.
    #[account(
        mut,
        seeds = [b"airsign_escrow", args.nonce_hash.as_ref()],
        bump = airsign_escrow.bump,
        close = relayer,
    )]
    pub airsign_escrow: Account<'info, AirsignEscrow>,

    // Recipient: must match voucher_msg[8..40]. Receives the escrowed SOL.
    // NOT a signer -- no wallet connection required to claim.
    /// CHECK: verified against voucher_msg recipient bytes inside handler
    #[account(mut)]
    pub recipient: UncheckedAccount<'info>,

    // Instructions sysvar for Ed25519SigVerify precompile check.
    /// CHECK: validated by address constraint
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ClaimAirsign>, args: ClaimAirsignArgs) -> Result<()> {
    // Verify domain separator byte
    require!(args.voucher_msg[0] == AIRSIGN_DOMAIN_SEP, SignitoError::InvalidVoucherSig);

    // Parse binary voucher message (fields shifted by 1 due to domain separator)
    let amount = u64::from_le_bytes(args.voucher_msg[1..9].try_into().unwrap());
    let recipient_bytes: [u8; 32] = args.voucher_msg[9..41]
        .try_into()
        .map_err(|_| error!(SignitoError::InvalidVoucherSig))?;
    let recipient = Pubkey::from(recipient_bytes);
    let nonce = &args.voucher_msg[41..57];

    require!(amount > 0, SignitoError::InvalidAmount);

    // Verify nonce_hash = sha256(nonce) -- ties the voucher message to the escrow PDA
    let computed_hash = hashv(&[nonce]);
    require!(computed_hash.to_bytes() == args.nonce_hash, SignitoError::InvalidVoucherSig);

    // Verify issuer account matches escrow.issuer
    require!(
        ctx.accounts.issuer.key() == ctx.accounts.airsign_escrow.issuer,
        SignitoError::Unauthorized
    );

    // Verify amount in voucher matches escrow (prevents partial-claim tricks)
    require!(amount == ctx.accounts.airsign_escrow.amount, SignitoError::InvalidVoucherSig);

    // Verify recipient account matches voucher_msg bytes
    // This is the tamper-proof guarantee: Ed25519 sig covers recipient, so any change fails sig check below
    require!(
        recipient == ctx.accounts.recipient.key(),
        SignitoError::RecipientMismatch
    );

    // Verify Ed25519 signature via instructions sysvar.
    // Transaction MUST include an Ed25519SigVerify instruction at index 0.
    let ed25519_ix = load_instruction_at_checked(
        0,
        &ctx.accounts.instructions.to_account_info(),
    ).map_err(|_| error!(SignitoError::InvalidVoucherSig))?;

    verify_ed25519_ix(
        &ed25519_ix,
        ctx.accounts.issuer.key.as_ref(),
        &args.voucher_msg,
        &args.sig,
    )?;

    // Transfer escrowed SOL from escrow_pda to recipient.
    // Anchor's `close = relayer` will send remaining rent lamports to relayer after handler.
    {
        let escrow_info = ctx.accounts.airsign_escrow.to_account_info();
        let recipient_info = ctx.accounts.recipient.to_account_info();
        **escrow_info.try_borrow_mut_lamports()? = escrow_info
            .lamports()
            .checked_sub(amount)
            .ok_or(SignitoError::Overflow)?;
        **recipient_info.try_borrow_mut_lamports()? = recipient_info
            .lamports()
            .checked_add(amount)
            .ok_or(SignitoError::Overflow)?;
    }

    msg!(
        "AirSign claimed: {} lamports -> {}",
        amount,
        ctx.accounts.recipient.key,
    );

    Ok(())
}

// Verify the Ed25519SigVerify instruction at index 0 matches expected pubkey/msg/sig.
//
// Ed25519SigVerify data layout:
//   [0]    num_signatures (u8, must be 1)
//   [1]    padding (u8, must be 0)
//   Per-sig 14-byte header:
//     [0..2]   sig_offset (u16 LE)
//     [2..4]   sig_ix_index (u16 LE)
//     [4..6]   pubkey_offset (u16 LE)
//     [6..8]   pubkey_ix_index (u16 LE)
//     [8..10]  msg_offset (u16 LE)
//     [10..12] msg_size (u16 LE)
//     [12..14] msg_ix_index (u16 LE)
//   Payload: [sig(64)] [pubkey(32)] [msg(N)]
fn verify_ed25519_ix(
    ix: &anchor_lang::solana_program::instruction::Instruction,
    expected_pubkey: &[u8],
    expected_msg: &[u8],
    expected_sig: &[u8],
) -> Result<()> {
    require!(ix.program_id == ed25519_program::ID, SignitoError::InvalidVoucherSig);

    let data = &ix.data;
    require!(data.len() >= 16, SignitoError::InvalidVoucherSig);
    require!(data[0] == 1, SignitoError::InvalidVoucherSig);

    let h = &data[2..];
    let sig_offset    = u16::from_le_bytes([h[0], h[1]]) as usize;
    let pubkey_offset = u16::from_le_bytes([h[4], h[5]]) as usize;
    let msg_offset    = u16::from_le_bytes([h[8], h[9]]) as usize;
    let msg_size      = u16::from_le_bytes([h[10], h[11]]) as usize;

    require!(data.len() >= sig_offset.saturating_add(64), SignitoError::InvalidVoucherSig);
    require!(data.len() >= pubkey_offset.saturating_add(32), SignitoError::InvalidVoucherSig);
    require!(data.len() >= msg_offset.saturating_add(msg_size), SignitoError::InvalidVoucherSig);
    require!(msg_size == expected_msg.len(), SignitoError::InvalidVoucherSig);

    require!(&data[sig_offset..sig_offset + 64] == expected_sig, SignitoError::InvalidVoucherSig);
    require!(&data[pubkey_offset..pubkey_offset + 32] == expected_pubkey, SignitoError::InvalidVoucherSig);
    require!(&data[msg_offset..msg_offset + msg_size] == expected_msg, SignitoError::InvalidVoucherSig);

    Ok(())
}

#[event]
pub struct AirsignClaimed {
    pub issuer: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
    pub claimed_at: i64,
}
