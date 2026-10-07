//! Local listing cover files. Not settlement, not $C_P$. Next.js does not store these.

use axum::body::Body;
use axum::extract::{Multipart, Path};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::domain::valid_pubkey;

const MAX_BYTES: usize = 2 * 1024 * 1024;

pub fn media_dir() -> PathBuf {
    PathBuf::from(std::env::var("MEDIA_DIR").unwrap_or_else(|_| "tmp/cpm-media".into()))
}

pub fn media_path_for(id: &str) -> Option<PathBuf> {
    if !valid_media_id(id) {
        return None;
    }
    Some(media_dir().join(id))
}

pub fn valid_media_id(id: &str) -> bool {
    let id = id.trim();
    let Some((stem, ext)) = id.rsplit_once('.') else {
        return false;
    };
    if stem.len() != 64 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    matches!(ext, "jpg" | "jpeg" | "png" | "webp")
}

pub fn sniff_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff {
        return Some("jpg");
    }
    if bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return Some("png");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

pub fn content_type(id: &str) -> &'static str {
    match id.rsplit_once('.').map(|(_, e)| e) {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

/// Relative URL stored on listings (`/v1/media/{id}`). Empty when no cover.
pub fn image_url(image_id: &str) -> String {
    let id = image_id.trim();
    if id.is_empty() || !valid_media_id(id) {
        String::new()
    } else {
        format!("/v1/media/{id}")
    }
}

pub async fn put_listing_media(mut multipart: Multipart) -> Result<impl IntoResponse, StatusCode> {
    let mut owner = String::new();
    let mut file: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        let name = field.name().unwrap_or("").to_string();
        let bytes = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        if name == "owner" {
            owner = String::from_utf8_lossy(&bytes).trim().to_string();
        } else if name == "file" || name == "image" {
            if bytes.len() > MAX_BYTES {
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
            file = Some(bytes.to_vec());
        }
    }
    if !valid_pubkey(&owner) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let bytes = file.ok_or(StatusCode::BAD_REQUEST)?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(StatusCode::BAD_REQUEST);
    }
    let ext = sniff_ext(&bytes).ok_or(StatusCode::UNSUPPORTED_MEDIA_TYPE)?;
    let hex = hex_sha256(&bytes);
    let id = format!("{hex}.{ext}");
    let dir = media_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = dir.join(&id);
    if !path.exists() {
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(Json(serde_json::json!({
        "id": id,
        "url": image_url(&id),
        "owner": owner,
    })))
}

pub async fn get_media(Path(id): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let path = media_path_for(&id).ok_or(StatusCode::NOT_FOUND)?;
    let bytes = tokio::fs::read(&path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let mut res = axum::http::Response::new(Body::from(bytes));
    let ct = HeaderValue::from_static(content_type(&id));
    res.headers_mut().insert(header::CONTENT_TYPE, ct);
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400, immutable"),
    );
    Ok(res)
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_escape() {
        assert!(!valid_media_id("../etc/passwd"));
        assert!(!valid_media_id("abc.jpg"));
        assert!(valid_media_id(&format!("{}.jpg", "a".repeat(64))));
    }

    #[test]
    fn sniffs_png() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&[0u8; 16]);
        assert_eq!(sniff_ext(&png), Some("png"));
        assert_eq!(sniff_ext(&[1, 2, 3]), None);
    }
}
