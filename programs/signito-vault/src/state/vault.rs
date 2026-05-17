use anchor_lang::prelude::*;

// AirsignEscrow: program-owned PDA that holds SOL locked by mint_airsign.
// Seeds: [b"airsign_escrow", nonce_hash (32 bytes)]
//
// Created by mint_airsign (sSOL burned, SOL moved from pool_pda to here).
// Closed by claim_airsign (SOL sent to recipient, rent returned to relayer).
// The PDA itself acts as the anti-replay mechanism: once closed it cannot be recreated
// with the same seeds because the nonce is consumed and the escrow no longer exists.
#[account]
pub struct AirsignEscrow {
    pub issuer: Pubkey,
    pub amount: u64,
    pub nonce_hash: [u8; 32],
    pub bump: u8,
}

impl AirsignEscrow {
    pub const LEN: usize = 8 + 32 + 8 + 32 + 1;
}
