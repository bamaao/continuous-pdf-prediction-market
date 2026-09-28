use super::DomainError;

#[derive(Clone, Debug)]
pub struct Fill {
    pub owner: String,
    pub market: String,
    pub set_hash: String,
    pub kind: String,
    pub mask: String,
    pub skellam_kind: Option<u8>,
    pub a: Option<i64>,
    pub b: Option<i64>,
}

impl Fill {
    pub fn new(owner: impl Into<String>, market: impl Into<String>) -> Result<Self, DomainError> {
        let owner = owner.into();
        let market = market.into();
        if owner.trim().is_empty() || market.trim().is_empty() {
            return Err(DomainError::Invalid("fill owner and market required"));
        }
        Ok(Self {
            owner: owner.trim().to_string(),
            market: market.trim().to_string(),
            set_hash: String::new(),
            kind: "mask".into(),
            mask: String::new(),
            skellam_kind: None,
            a: None,
            b: None,
        })
    }
}
