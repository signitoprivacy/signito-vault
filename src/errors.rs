use anchor_lang::prelude::*;

#[error_code]
pub enum SignitoError {
    #[msg("OTS preimage does not match. Vault code incorrect or wrong withdrawal step.")]
    InvalidOtsPreimage,

    #[msg("OTS chain exhausted. Call refresh_ots with a new chain tip.")]
    VaultExhausted,

    #[msg("Requested amount exceeds shielded balance.")]
    InsufficientFunds,

    #[msg("Amount must be greater than zero.")]
    InvalidAmount,

    #[msg("Ed25519 signature verification failed. Voucher is invalid.")]
    InvalidVoucherSig,

    #[msg("This voucher has expired.")]
    VoucherExpired,

    #[msg("This voucher has already been claimed.")]
    VoucherAlreadyClaimed,

    #[msg("This voucher was issued to a different address.")]
    RecipientMismatch,

    #[msg("Account still holds funds. Withdraw all SOL before closing.")]
    AccountNotEmpty,

    #[msg("Unauthorized. Delegate or ownership check failed.")]
    Unauthorized,

    #[msg("Arithmetic overflow.")]
    Overflow,

    #[msg("Pool not initialized. Call initialize_pool first.")]
    PoolNotInitialized,
}
