use crate::MarketError;
use anchor_lang::prelude::*;

pub fn decode(bytes: &[u8], n: usize) -> Result<Vec<bool>> {
    require!(n > 0, MarketError::EmptySet);
    let need = n.div_ceil(8);
    require!(bytes.len() == need, MarketError::BadMask);
    let mut out = vec![false; n];
    let mut any = false;
    for i in 0..n {
        if bytes[i / 8] & (1 << (i % 8)) != 0 {
            out[i] = true;
            any = true;
        }
    }
    let rem = n % 8;
    if rem != 0 {
        let extra = bytes[need - 1] >> rem;
        require!(extra == 0, MarketError::BadMask);
    }
    require!(any, MarketError::EmptySet);
    Ok(out)
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
