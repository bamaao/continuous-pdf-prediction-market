use super::{normalize_i18n, normalize_source_locale, valid_pubkey, DomainError, I18nMap, Listing};

pub const APP_PENDING: u8 = 0;
pub const APP_APPROVED: u8 = 1;
pub const APP_REJECTED: u8 = 2;
pub const APP_DUPLICATE: u8 = 3;

#[derive(Clone, Debug)]
pub struct ListingApplication {
    pub id: i64,
    pub applicant: String,
    pub family: u8,
    pub title: String,
    pub tags: Vec<String>,
    pub event: String,
    pub description: String,
    pub topic: String,
    pub tag: String,
    pub blocked_regions: Vec<String>,
    pub dup_key: String,
    pub status: u8,
    pub reviewer: String,
    pub reason: String,
    pub created_at: i64,
    pub reviewed_at: i64,
    pub compose_json: String,
    pub market: String,
    pub source_locale: String,
    pub i18n: I18nMap,
    pub image_id: String,
}

#[derive(Clone, Debug)]
pub struct ReviewLog {
    pub id: i64,
    pub application_id: i64,
    pub reviewer: String,
    pub action: String,
    pub reason: String,
    pub created_at: i64,
}

impl ListingApplication {
    pub fn submit(
        applicant: impl Into<String>,
        family: u8,
        title: impl Into<String>,
        tags: Vec<String>,
        event: impl Into<String>,
        description: impl Into<String>,
        topic: impl Into<String>,
        tag: impl Into<String>,
        blocked_regions: Vec<String>,
        compose_json: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let applicant = applicant.into().trim().to_string();
        if !valid_pubkey(&applicant) {
            return Err(DomainError::Invalid("applicant pubkey"));
        }
        if family > 4 {
            return Err(DomainError::Invalid("unknown family"));
        }
        let listing = Listing::new(
            "pending",
            title,
            tags,
            topic,
            tag,
            description,
            event,
        )?;
        let blocked_regions = normalize_regions(blocked_regions)?;
        let compose_json = normalize_compose(compose_json)?;
        let dup_key = duplicate_key(family, &listing.title, &listing.event);
        Ok(Self {
            id: 0,
            applicant,
            family,
            title: listing.title,
            tags: listing.tags,
            event: listing.event,
            description: listing.description,
            topic: listing.topic,
            tag: listing.tag,
            blocked_regions,
            dup_key,
            status: APP_PENDING,
            reviewer: String::new(),
            reason: String::new(),
            created_at: 0,
            reviewed_at: 0,
            compose_json,
            market: String::new(),
            source_locale: "en".into(),
            i18n: I18nMap::new(),
            image_id: String::new(),
        })
    }

    pub fn with_locale(mut self, source_locale: impl Into<String>, i18n: I18nMap) -> Result<Self, DomainError> {
        self.source_locale = normalize_source_locale(&source_locale.into())?;
        self.i18n = normalize_i18n(i18n)?;
        Ok(self)
    }

    pub fn with_image(mut self, image_id: impl Into<String>) -> Self {
        self.image_id = image_id.into().trim().to_string();
        self
    }

    pub fn is_blocking_duplicate(&self) -> bool {
        self.status == APP_PENDING || self.status == APP_APPROVED
    }
}

pub fn duplicate_key(family: u8, title: &str, event: &str) -> String {
    format!(
        "{}|{}|{}",
        family,
        fold_key(title),
        fold_key(event)
    )
}

fn fold_key(raw: &str) -> String {
    raw.split_whitespace()
        .map(|w| {
            w.chars()
                .map(|c| if c.is_ascii_alphabetic() { c.to_ascii_lowercase() } else { c })
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_compose(raw: impl Into<String>) -> Result<String, DomainError> {
    let raw = raw.into();
    let t = raw.trim();
    if t.is_empty() {
        return Ok("{}".into());
    }
    if t.len() > 16_384 {
        return Err(DomainError::Invalid("compose too large"));
    }
    let v: serde_json::Value = serde_json::from_str(t).map_err(|_| DomainError::Invalid("compose json"))?;
    if !v.is_object() {
        return Err(DomainError::Invalid("compose object"));
    }
    Ok(t.to_string())
}

pub fn normalize_regions(raw: Vec<String>) -> Result<Vec<String>, DomainError> {
    let mut out = Vec::new();
    for s in raw {
        let t = s.trim().to_ascii_uppercase();
        if t.is_empty() {
            continue;
        }
        if t.len() < 2 || t.len() > 6 || !t.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return Err(DomainError::Invalid("region code"));
        }
        if !out.iter().any(|x: &String| x == &t) {
            out.push(t);
        }
        if out.len() > 32 {
            return Err(DomainError::Invalid("too many regions"));
        }
    }
    Ok(out)
}

pub fn region_blocked(blocked: &[String], country: Option<&str>) -> bool {
    let Some(c) = country.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let c = c.to_ascii_uppercase();
    blocked.iter().any(|r| r == &c || c.starts_with(r) || r.starts_with(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALLET: &str = "So11111111111111111111111111111111111111112";

    #[test]
    fn submit_builds_dup_key_and_regions() {
        let row = ListingApplication::submit(
            WALLET,
            1,
            " US CPI YoY ",
            vec!["Macro".into()],
            " US CPI YoY first print ",
            "First official print. Revisions do not settle.",
            "US_CPI_YOY",
            "2026-03",
            vec!["cn".into(), "US".into()],
            r#"{"op":"create_gaussian","close_in":86400}"#,
        )
        .unwrap();
        assert_eq!(row.dup_key, "1|us cpi yoy|us cpi yoy first print");
        assert_eq!(row.blocked_regions, vec!["CN", "US"]);
        assert_eq!(row.status, APP_PENDING);
        assert!(row.compose_json.contains("create_gaussian"));
        assert!(row.market.is_empty());
    }

    #[test]
    fn compose_must_be_object() {
        assert!(ListingApplication::submit(
            WALLET,
            1,
            "US CPI YoY",
            vec!["macro".into()],
            "US CPI YoY first print",
            "First official print. Revisions do not settle.",
            "US_CPI_YOY",
            "2026-03",
            vec![],
            "[1]",
        )
        .is_err());
    }

    #[test]
    fn same_event_collides() {
        let a = duplicate_key(0, "Arsenal vs Chelsea", "Premier League");
        let b = duplicate_key(0, "arsenal vs chelsea", "premier league");
        assert_eq!(a, b);
        assert_ne!(a, duplicate_key(1, "Arsenal vs Chelsea", "Premier League"));
    }

    #[test]
    fn geo_hides_blocked_country() {
        assert!(region_blocked(&["CN".into()], Some("cn")));
        assert!(!region_blocked(&["CN".into()], Some("US")));
        assert!(!region_blocked(&["CN".into()], None));
    }
}
