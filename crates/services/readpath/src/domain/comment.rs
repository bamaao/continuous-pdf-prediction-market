use super::DomainError;

const AUTHOR_MIN: usize = 32;
const AUTHOR_MAX: usize = 44;
const BODY_MAX: usize = 2000;
const B58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

#[derive(Clone, Debug)]
pub struct Comment {
    pub id: i64,
    pub market: String,
    pub author: String,
    pub body: String,
    pub created_at: i64,
}

impl Comment {
    pub fn new(market: impl Into<String>, author: impl Into<String>, body: impl Into<String>) -> Result<Self, DomainError> {
        let market = market.into().trim().to_string();
        let author = author.into().trim().to_string();
        let body = body.into().trim().to_string();
        if market.is_empty() {
            return Err(DomainError::Invalid("market required"));
        }
        if !valid_pubkey(&author) {
            return Err(DomainError::Invalid("author pubkey"));
        }
        if body.is_empty() {
            return Err(DomainError::Invalid("comment required"));
        }
        if body.chars().count() > BODY_MAX {
            return Err(DomainError::Invalid("comment too long"));
        }
        Ok(Self {
            id: 0,
            market,
            author,
            body,
            created_at: 0,
        })
    }
}

pub fn valid_pubkey(s: &str) -> bool {
    let n = s.len();
    (AUTHOR_MIN..=AUTHOR_MAX).contains(&n) && s.bytes().all(|c| B58.contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALLET: &str = "So11111111111111111111111111111111111111112";

    #[test]
    fn comment_requires_indexed_identity() {
        assert!(Comment::new("", WALLET, "hello").is_err());
        assert!(Comment::new("m", "short", "hello").is_err());
        assert!(Comment::new("m", WALLET, "  ").is_err());
        assert!(Comment::new("m", WALLET, "0".repeat(2001)).is_err());
    }

    #[test]
    fn comment_accepts_wallet_and_trimmed_body() {
        let row = Comment::new(" Seed111 ", format!(" {WALLET} "), "  first note  ").unwrap();
        assert_eq!(row.market, "Seed111");
        assert_eq!(row.author, WALLET);
        assert_eq!(row.body, "first note");
    }
}
