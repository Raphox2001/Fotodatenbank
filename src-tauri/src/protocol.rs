//! `vault://`-Protokoll: liefert entschlüsselte Bilder/Videos aus dem RAM an die
//! Oberfläche. Pfade: `/t-<id>` (Vorschau), `/m-<id>` (Original bzw. Anzeige-PNG).
//! Videos werden in Bereichen (HTTP Range) ausgeliefert, damit man spulen kann.

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

use crate::media;
use crate::vault::{MediaKind, ObjKind};
use crate::AppState;

const MAX_RANGE: u64 = 4 * 1024 * 1024;

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(code).header(header::CACHE_CONTROL, "no-store").body(Vec::new()).unwrap()
}

fn ok(mime: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-store")
        .body(body)
        .unwrap()
}

fn parse_range(value: &str, len: u64) -> Option<(u64, u64)> {
    let spec = value.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let (start, end) = if a.is_empty() {
        let suffix: u64 = b.parse().ok()?;
        (len.saturating_sub(suffix), len.saturating_sub(1))
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { len.saturating_sub(1) } else { b.parse::<u64>().ok()?.min(len.saturating_sub(1)) };
        (start, end)
    };
    (start <= end && start < len).then_some((start, end))
}

pub fn handle(app: &AppHandle, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let state = app.state::<AppState>();
    let Some(session) = state.current() else { return status(StatusCode::FORBIDDEN) };

    let path = req.uri().path().trim_start_matches('/');
    let (thumb, id) = match path.split_once('-') {
        Some(("t", id)) => (true, id),
        Some(("m", id)) => (false, id),
        _ => return status(StatusCode::NOT_FOUND),
    };
    // Index-Sperre vor dem Zugriff auf den Tresor wieder freigeben.
    let item = session.index.read().unwrap().media.iter().find(|m| m.id == id).cloned();
    let Some(item) = item else { return status(StatusCode::NOT_FOUND) };

    if thumb {
        return match session.vault.read().unwrap().read_all(id, ObjKind::Thumb) {
            Ok(data) => ok("image/jpeg", data.to_vec()),
            Err(_) => status(StatusCode::NOT_FOUND),
        };
    }

    if item.kind == MediaKind::Image {
        if media::needs_png_conversion(&item.ext) {
            return match session.display_png(id) {
                Ok(png) => ok("image/png", png.to_vec()),
                Err(_) => status(StatusCode::UNSUPPORTED_MEDIA_TYPE),
            };
        }
        return match session.vault.read().unwrap().read_all(id, ObjKind::Original) {
            Ok(data) => ok(media::display_mime(&item.ext), data.to_vec()),
            Err(_) => status(StatusCode::INTERNAL_SERVER_ERROR),
        };
    }

    // Video: Wiedergabe zählt als Bedienung (keine Auto-Sperre mitten im Film).
    state.touch();
    let mut reader = match session.vault.read().unwrap().open_reader(id, ObjKind::Original) {
        Ok(r) => r,
        Err(_) => return status(StatusCode::NOT_FOUND),
    };
    let len = reader.len();
    let (start, mut end) = req
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_range(v, len))
        .unwrap_or((0, len.saturating_sub(1)));
    end = end.min(start + MAX_RANGE - 1);
    let body = match reader.read_range(start, end + 1) {
        Ok(b) => b.to_vec(),
        Err(_) => return status(StatusCode::INTERNAL_SERVER_ERROR),
    };
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, media::display_mime(&item.ext))
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
        .header(header::CONTENT_LENGTH, body.len().to_string())
        .header(header::CACHE_CONTROL, "no-store")
        .body(body)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-200", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
    }
}
