use std::str::FromStr;

use ethers_core::types::U256;
use eyre::Context;
use sqlx::types::BigDecimal;

/// convert `U256` to BigDecimal
pub fn u256_to_big_decimal(value: &U256) -> eyre::Result<BigDecimal> {
    let value_as_str = value.to_string();

    BigDecimal::from_str(&value_as_str).wrap_err("failed to convert U256 to BigDecimal")
}
