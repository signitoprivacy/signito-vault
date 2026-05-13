use anchor_lang::prelude::*;

// Shared pool PDA: seeds = [b"pool"]
// Holds ALL users' SOL. One address for the entire protocol.
// mint_stoken is the shared sSOL mint (same CA for all users).
#[account]
pub struct PoolState {
    pub mint_stoken: Pubkey,
    pub total_deposited: u64,
    pub bump: u8,
}

impl PoolState {
    pub const LEN: usize = 8 + 32 + 8 + 1;
}
