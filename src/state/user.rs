use anchor_lang::prelude::*;

// Per-user state PDA: seeds = [b"user_state", stoken_ata.key()]
//
// Critically: derived from stoken_ata (random fresh keypair address),
// NOT from the user's wallet pubkey. This means the user's wallet address
// does NOT appear in private_send instruction accounts -- only stoken_ata
// (random address) and user_state (derived from it) appear.
//
// OTS chain (same scheme as before):
//   H0  = PBKDF2(vaultCode, walletAddress, 100_000, SHA-256)
//   H32 = SHA-256^32(H0)  -- stored as current_ots_hash (tip)
//   Each transfer reveals H_{n-1}, program verifies SHA-256(H_{n-1}) == H_n
//   chain_depth counts remaining uses.
//   Refresh: call refresh_ots with new tip from next generation suffix.
#[account]
pub struct UserState {
    pub stoken_ata: Pubkey,
    pub current_ots_hash: [u8; 32],
    pub chain_depth: u8,
    pub deposited: u64,
    pub bump: u8,
}

impl UserState {
    pub const LEN: usize = 8 + 32 + 32 + 1 + 8 + 1;
}
