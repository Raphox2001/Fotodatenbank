//! Erkennen, Vorschaubilder und Metadaten von Bildern und Videos.
//! Gelesen wird nur die Quelldatei beim Import, es entstehen keine Zwischendateien.

use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};

use crate::error::{Error, Result};
use crate::vault::MediaKind;

pub const THUMB_SIZE: u32 = 480;

const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "webp", "gif", "bmp"];
const VIDEO_EXT: &[&str] = &["mp4", "mov", "m4v", "webm", "mkv", "avi"];

pub fn classify(path: &Path) -> Option<(MediaKind, String)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if IMAGE_EXT.contains(&ext.as_str()) {
        Some((MediaKind::Image, ext))
    } else if VIDEO_EXT.contains(&ext.as_str()) {
        Some((MediaKind::Video, ext))
    } else {
        None
    }
}

/// Löst Ordner rekursiv auf und liefert alle unterstützten Dateien.
/// Nicht unterstützte Dateien landen in `skipped`.
pub fn collect_files(paths: &[PathBuf], skipped: &mut Vec<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = paths.to_vec();
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
                entries.sort();
                entries.reverse();
                stack.extend(entries);
            }
        } else if p.is_file() {
            let hidden = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
            if hidden {
                continue;
            }
            if classify(&p).is_some() {
                out.push(p);
            } else {
                skipped.push(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            }
        }
    }
    out
}

/// MIME-Typ für die Anzeige im Browser. MOV mit H.264 spielt Chromium als MP4 ab.
pub fn display_mime(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "tif" | "tiff" | "bmp" => "image/png", // wird zur Anzeige verlustfrei nach PNG gewandelt
        "mp4" | "m4v" | "mov" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        _ => "application/octet-stream",
    }
}

/// Formate, die der Browser nicht direkt anzeigen kann.
pub fn needs_png_conversion(ext: &str) -> bool {
    matches!(ext, "tif" | "tiff" | "bmp")
}

fn decode_oriented(reader: ImageReader<impl std::io::BufRead + std::io::Seek>) -> Result<DynamicImage> {
    let mut reader = reader.with_guessed_format().map_err(|e| Error::Other(e.to_string()))?;
    reader.no_limits();
    let mut decoder = reader.into_decoder().map_err(img_err)?;
    let orientation = decoder.orientation().map_err(img_err)?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(img_err)?;
    img.apply_orientation(orientation);
    Ok(img)
}

fn img_err(e: image::ImageError) -> Error {
    Error::Other(format!("Bild kann nicht gelesen werden: {e}"))
}

pub struct ImageInfo {
    pub thumb: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn image_info(path: &Path) -> Result<ImageInfo> {
    let reader = ImageReader::new(BufReader::new(File::open(path)?));
    let img = decode_oriented(reader)?;
    Ok(ImageInfo { thumb: encode_thumb(&img)?, width: img.width(), height: img.height() })
}

fn encode_thumb(img: &DynamicImage) -> Result<Vec<u8>> {
    let small = img.thumbnail(THUMB_SIZE, THUMB_SIZE).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82)
        .encode_image(&small)
        .map_err(img_err)?;
    Ok(out)
}

/// Wandelt TIFF/BMP verlustfrei in PNG um (nur zur Anzeige, das Original bleibt).
pub fn to_display_png(data: &[u8]) -> Result<Vec<u8>> {
    let img = decode_oriented(ImageReader::new(Cursor::new(data)))?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).map_err(img_err)?;
    Ok(out.into_inner())
}

pub fn exif_date(path: &Path) -> Option<String> {
    let mut r = BufReader::new(File::open(path).ok()?);
    let exif = exif::Reader::new().read_from_container(&mut r).ok()?;
    let field = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .or_else(|| exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY))?;
    Some(field.display_value().to_string())
}

// ---------------------------------------------------------------------------
// Videos über ffmpeg/ffprobe

#[derive(Clone, Default)]
pub struct Tools {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
}

impl Tools {
    pub fn locate(resource_dir: Option<PathBuf>) -> Self {
        Self { ffmpeg: find_tool("ffmpeg", &resource_dir), ffprobe: find_tool("ffprobe", &resource_dir) }
    }
}

fn find_tool(name: &str, resource_dir: &Option<PathBuf>) -> Option<PathBuf> {
    let exe = format!("{name}.exe");
    let mut candidates = Vec::new();
    if let Some(r) = resource_dir {
        candidates.push(r.join("bin").join(&exe));
    }
    if let Ok(cur) = std::env::current_exe() {
        if let Some(d) = cur.parent() {
            candidates.push(d.join("bin").join(&exe));
            candidates.push(d.join(&exe));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(&exe)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn hidden_command(program: &Path) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.stdin(Stdio::null()).stderr(Stdio::null());
    c
}

pub struct VideoInfo {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    pub taken_at: Option<String>,
}

pub fn video_info(tools: &Tools, path: &Path) -> VideoInfo {
    let mut info = VideoInfo { width: None, height: None, duration: None, taken_at: None };
    let Some(ffprobe) = &tools.ffprobe else { return info };
    let out = hidden_command(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .arg("stream=width,height:stream_side_data=rotation:stream_tags=rotate:format=duration:format_tags=creation_time")
        .args(["-of", "json"])
        .arg(path)
        .output();
    let Ok(out) = out else { return info };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return info };
    let s = &v["streams"][0];
    info.width = s["width"].as_u64().map(|x| x as u32);
    info.height = s["height"].as_u64().map(|x| x as u32);
    let rotation = s["side_data_list"][0]["rotation"]
        .as_i64()
        .or_else(|| s["tags"]["rotate"].as_str().and_then(|r| r.parse().ok()))
        .unwrap_or(0);
    if rotation.abs() == 90 || rotation.abs() == 270 {
        std::mem::swap(&mut info.width, &mut info.height);
    }
    info.duration = v["format"]["duration"].as_str().and_then(|d| d.parse().ok());
    info.taken_at = v["format"]["tags"]["creation_time"]
        .as_str()
        .map(|t| t.replace('T', " ").chars().take(19).collect());
    info
}

pub fn video_thumb(tools: &Tools, path: &Path, duration: Option<f64>) -> Option<Vec<u8>> {
    let ffmpeg = tools.ffmpeg.as_ref()?;
    let seek = match duration {
        Some(d) if d > 3.0 => "1",
        _ => "0",
    };
    let scale = format!("scale={THUMB_SIZE}:{THUMB_SIZE}:force_original_aspect_ratio=decrease");
    let out = hidden_command(ffmpeg)
        .args(["-v", "error", "-ss", seek, "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &scale, "-f", "image2pipe", "-vcodec", "mjpeg", "-q:v", "4", "-"])
        .output()
        .ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fdb-media-{}-{name}", crate::crypto::random_id()))
    }

    #[test]
    fn images_thumbs_and_tiff_display() {
        let img = image::RgbImage::from_fn(1200, 800, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]));
        for ext in ["jpg", "png", "tiff", "webp", "bmp"] {
            let p = tmp(&format!("bild.{ext}"));
            img.save(&p).unwrap();
            assert_eq!(classify(&p).unwrap().0, MediaKind::Image);
            let info = image_info(&p).unwrap();
            assert_eq!((info.width, info.height), (1200, 800), "{ext}");
            let t = image::load_from_memory(&info.thumb).unwrap();
            assert_eq!((t.width(), t.height()), (THUMB_SIZE, 320));
            if needs_png_conversion(ext) {
                let png = to_display_png(&std::fs::read(&p).unwrap()).unwrap();
                let back = image::load_from_memory(&png).unwrap().to_rgb8();
                assert_eq!(back, img, "PNG-Anzeige muss verlustfrei sein");
            }
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn video_probe_and_thumb() {
        let tools = Tools::locate(None);
        let Some(ffmpeg) = &tools.ffmpeg else {
            eprintln!("ffmpeg nicht gefunden, Test übersprungen");
            return;
        };
        let p = tmp("clip.mp4");
        let ok = hidden_command(ffmpeg)
            .args(["-v", "error", "-f", "lavfi", "-i", "testsrc=duration=5:size=640x360:rate=25", "-pix_fmt", "yuv420p"])
            .arg(&p)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let info = video_info(&tools, &p);
        assert_eq!((info.width, info.height), (Some(640), Some(360)));
        assert!((info.duration.unwrap() - 5.0).abs() < 0.2);
        let thumb = video_thumb(&tools, &p, info.duration).expect("Vorschaubild");
        let t = image::load_from_memory(&thumb).unwrap();
        assert_eq!(t.width(), THUMB_SIZE);
        std::fs::remove_file(p).unwrap();
    }
}
