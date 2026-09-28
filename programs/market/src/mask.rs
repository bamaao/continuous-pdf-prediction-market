use crate::MarketError;
use anchor_lang::prelude::*;

pub fn bit(bytes: &[u8], i: usize) -> bool {
    bytes
        .get(i / 8)
        .map(|b| b & (1 << (i % 8)) != 0)
        .unwrap_or(false)
}

/// Length, trailing-bit, and non-empty checks without an `n`-bool heap vec.
pub fn check(bytes: &[u8], n: usize) -> Result<()> {
    require!(n > 0, MarketError::EmptySet);
    let need = n.div_ceil(8);
    require!(bytes.len() == need, MarketError::BadMask);
    let mut any = false;
    for i in 0..n {
        if bit(bytes, i) {
            any = true;
        }
    }
    let rem = n % 8;
    if rem != 0 {
        let extra = bytes[need - 1] >> rem;
        require!(extra == 0, MarketError::BadMask);
    }
    require!(any, MarketError::EmptySet);
    Ok(())
}

pub fn decode(bytes: &[u8], n: usize) -> Result<Vec<bool>> {
    check(bytes, n)?;
    Ok((0..n).map(|i| bit(bytes, i)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_bit() {
        let m = decode(&[0b0000_0101], 4).unwrap();
        assert_eq!(m, vec![true, false, true, false]);
    }

    #[test]
    fn rejects_empty() {
        assert!(decode(&[0], 4).is_err());
    }

    #[test]
    fn rejects_high_bits() {
        assert!(decode(&[0b1000_0001], 4).is_err());
    }
}
