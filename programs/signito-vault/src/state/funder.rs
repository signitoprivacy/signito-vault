use anchor_lang::prelude::*;

// FunderPDA: program-owned account that holds operational SOL for funding fresh relayer wallets.
// Seeds: [b"funder"]
//
// admin: can withdraw, deposit, set relayer.
// relayer: can call fund_fresh_relayer and process_queue.
// SOL balance is tracked via account lamports, not a separate field.
#[account]
pub struct FunderState {
    pub admin: Pubkey,
    pub relayer: Pubkey,
    pub bump: u8,
}

impl FunderState {
    pub const LEN: usize = 8 + 32 + 32 + 1;
}
