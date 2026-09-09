//! Extracts audio/video/photo/document content metadata for
//! `strategy = "audio"`/`"video"`/`"photos"`/`"documents"` `.sift.toml`
//! organize policies.
//!
//! This is the one place in Sift that reads file *content* rather than
//! only extension/mtime — see `classifier.rs` (extension only) and
//! `planner::extract_date_metadata` (mtime only) for the deliberately
//! narrower norm everywhere else. Every extractor here is read-only:
//! nothing in this module ever writes, moves, or deletes a file.

use crate::config::{AudioMetadata, DocumentMetadata, PhotoMetadata, VideoMetadata};
use std::path::Path;

/// Reads whatever common tag fields the file's format actually carries
/// (ID3v2, Vorbis comments, MP4 atoms, APE, ...) via `lofty`'s unified
/// accessor API, preferring the format's primary tag and falling back to
/// its first tag if there is no primary one. A field with no underlying
/// tag value is `None` — never guessed or defaulted. A file with no tag
/// at all yields an all-`None` `AudioMetadata` rather than an error, so
/// callers see a uniform "missing metadata" outcome either way.
pub fn extract_audio_metadata(path: &Path) -> Result<AudioMetadata, String> {
    use lofty::file::TaggedFileExt;
    use lofty::tag::{Accessor, ItemKey};

    let tagged_file =
        lofty::read_from_path(path).map_err(|e| format!("cannot read audio metadata: {e}"))?;
    let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
    else {
        return Ok(AudioMetadata::default());
    };
    Ok(AudioMetadata {
        title: tag.title().map(|s| s.into_owned()),
        artist: tag.artist().map(|s| s.into_owned()),
        album: tag.album().map(|s| s.into_owned()),
        album_artist: tag.get_string(ItemKey::AlbumArtist).map(str::to_string),
        genre: tag.genre().map(|s| s.into_owned()),
        year: tag.date().map(|d| d.year.to_string()),
        track: tag.track().map(|t| t.to_string()),
    })
}

/// QuickTime/MP4 `mvhd.creation_time` is seconds since 1904-01-01, not the
/// Unix epoch (1970-01-01) — this is the fixed offset between the two.
const QUICKTIME_TO_UNIX_EPOCH_SECS: u64 = 2_082_844_800;

/// Reads container-level metadata for `strategy = "video"`. Tries the
/// optional `ffprobe` backend first (broader format support, richer
/// fields: `{duration}`/`{fps}` only ever come from here); if `ffprobe`
/// isn't installed (spawning it fails) or fails for any reason (bad exit
/// code, unparseable output, no video stream), falls back to the
/// pure-Rust MP4/MOV-only parser this crate has always used — unchanged
/// from before `ffprobe` support existed, so a machine without `ffmpeg`
/// installed sees no difference at all.
pub fn extract_video_metadata(path: &Path) -> Result<VideoMetadata, String> {
    let search_path = std::env::var("PATH").unwrap_or_default();
    extract_video_metadata_with_ffprobe_search_path(path, &search_path)
}

/// Same as [`extract_video_metadata`], but resolves `ffprobe` against
/// `ffprobe_search_path` instead of reading the process's real `PATH`.
/// This is a deliberate test seam: it lets integration tests exercise the
/// "ffprobe not installed" fallback (pass `""`) and the two backends'
/// output side by side, deterministically, without mutating the test
/// process's actual `PATH` — which would race with any other test in the
/// same binary that legitimately needs `ffprobe` to be found.
pub fn extract_video_metadata_with_ffprobe_search_path(
    path: &Path,
    ffprobe_search_path: &str,
) -> Result<VideoMetadata, String> {
    match extract_video_metadata_via_ffprobe(path, ffprobe_search_path) {
        Ok(meta) => Ok(meta),
        Err(_) => extract_video_metadata_via_mp4_crate(path),
    }
}

/// The original, dependency-free extractor: width/height/codec fourcc
/// from the first video track, and the movie header's creation year, via
/// the pure-Rust `mp4` crate — MP4/MOV containers only. `creation_time` is
/// frequently left at `0` by encoders; that case renders as unavailable,
/// never guessed from the file's mtime (which would quietly blur the
/// `date`/`video` strategy boundary). Never sets `duration_seconds`/`fps`
/// — those are `ffprobe`-only fields.
fn extract_video_metadata_via_mp4_crate(path: &Path) -> Result<VideoMetadata, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read video metadata: {e}"))?;
    let size = file
        .metadata()
        .map_err(|e| format!("cannot read video metadata: {e}"))?
        .len();
    let reader = std::io::BufReader::new(file);
    let mp4 = mp4::Mp4Reader::read_header(reader, size)
        .map_err(|e| format!("cannot read video metadata: {e}"))?;

    let track = mp4
        .tracks()
        .values()
        .find(|t| matches!(t.track_type(), Ok(mp4::TrackType::Video)))
        .ok_or_else(|| "no video track found".to_string())?;

    let creation_time = mp4.moov.mvhd.creation_time;
    let year = creation_time
        .checked_sub(QUICKTIME_TO_UNIX_EPOCH_SECS)
        .map(|unix_secs| {
            crate::config::DateMetadata::from_unix_secs(unix_secs as i64)
                .year
                .to_string()
        });

    Ok(VideoMetadata {
        width: u32::from(track.width()),
        height: u32::from(track.height()),
        codec: track.box_type().ok().map(|fourcc| fourcc.to_string()),
        year,
        duration_seconds: None,
        fps: None,
    })
}

/// Resolves `ffprobe` as a plain filename in each `:`-separated directory
/// of `search_path`, mirroring how a shell would find it on `PATH` — done
/// explicitly (rather than letting `Command::new("ffprobe")` search the
/// process's real `PATH`) so `ffprobe_search_path` can be overridden for
/// tests. Returns `None` (never spawns anything) if no directory has an
/// `ffprobe` file.
fn find_ffprobe(search_path: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(search_path)
        .map(|dir| dir.join("ffprobe"))
        .find(|candidate| candidate.is_file())
}

/// Reads container metadata by shelling out to `ffprobe -show_format
/// -show_streams -print_format json` and parsing its output with
/// `serde_json` (already a dependency — no new crate needed). The file
/// path is passed as a separate `Command` argument, never interpolated
/// into a shell string, so there is no shell-injection surface. Returns
/// `Err` (never partial data) if `ffprobe` isn't on `PATH`, exits
/// non-zero, or the JSON doesn't contain a decodable video stream —
/// `extract_video_metadata` treats any `Err` here as "try the fallback
/// parser instead", never as a hard failure on its own.
fn extract_video_metadata_via_ffprobe(
    path: &Path,
    ffprobe_search_path: &str,
) -> Result<VideoMetadata, String> {
    let ffprobe =
        find_ffprobe(ffprobe_search_path).ok_or_else(|| "ffprobe not found on PATH".to_string())?;
    let output = std::process::Command::new(ffprobe)
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe unavailable: {e}"))?;
    if !output.status.success() {
        return Err(format!("ffprobe exited with {}", output.status));
    }
    let root: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("ffprobe output: {e}"))?;

    let video_stream = root["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["codec_type"] == "video")
        .ok_or_else(|| "no video stream found".to_string())?;

    let width = video_stream["width"]
        .as_u64()
        .ok_or_else(|| "no video width reported".to_string())?;
    let height = video_stream["height"]
        .as_u64()
        .ok_or_else(|| "no video height reported".to_string())?;
    // `codec_tag_string` (e.g. "avc1"), never `codec_name` (e.g. "h264") —
    // this is what keeps `{codec}` rendering identically regardless of
    // which backend produced it, so a template written before `ffmpeg`
    // was installed never silently points somewhere new afterward.
    let codec = video_stream["codec_tag_string"]
        .as_str()
        .map(str::to_string);

    let creation_time = root["format"]["tags"]["creation_time"]
        .as_str()
        .or_else(|| video_stream["tags"]["creation_time"].as_str());
    let year = creation_time.and_then(parse_creation_year);

    let duration_seconds = root["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|secs| secs.floor() as u64);
    let fps = video_stream["r_frame_rate"]
        .as_str()
        .and_then(parse_frame_rate);

    Ok(VideoMetadata {
        width: width as u32,
        height: height as u32,
        codec,
        year,
        duration_seconds,
        fps,
    })
}

/// Extracts the 4-digit calendar year from the start of an ISO 8601
/// timestamp (`ffprobe`'s `creation_time` shape, e.g.
/// `"2024-06-15T10:30:00.000000Z"`), without pulling in a date-parsing
/// dependency for this one field. Anything not shaped like that is
/// treated as unavailable rather than guessed at.
fn parse_creation_year(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.len() >= 5 && bytes[..4].iter().all(u8::is_ascii_digit) && bytes[4] == b'-' {
        Some(s[..4].to_string())
    } else {
        None
    }
}

/// Parses `ffprobe`'s `r_frame_rate` shape (a rational as text, e.g.
/// `"25/1"` or `"30000/1001"`) into a rounded whole-number fps.
fn parse_frame_rate(s: &str) -> Option<u32> {
    let (num, den) = s.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    if den == 0.0 {
        return None;
    }
    Some((num / den).round() as u32)
}

/// Reads EXIF metadata (camera make/model, capture date) via the
/// pure-Rust `kamadak-exif` crate, which auto-detects the container
/// (JPEG, TIFF, HEIF/HEIC, PNG, WebP) from its bytes. A file with no EXIF
/// segment at all (common for PNG/WebP, and for a JPEG stripped of
/// metadata) yields an all-`None` `PhotoMetadata` rather than an error —
/// the same uniform "missing metadata" outcome `extract_audio_metadata`
/// gives for an untagged audio file.
pub fn extract_photo_metadata(path: &Path) -> Result<PhotoMetadata, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read photo metadata: {e}"))?;
    let mut reader = std::io::BufReader::new(&file);
    let exif = match exif::Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(exif::Error::NotFound(_)) => return Ok(PhotoMetadata::default()),
        Err(e) => return Err(format!("cannot read photo metadata: {e}")),
    };

    let make = exif_ascii(&exif, exif::Tag::Make);
    let model = exif_ascii(&exif, exif::Tag::Model);
    let camera = combine_make_model(make, model);

    let (year, month, day) = exif_ascii(&exif, exif::Tag::DateTimeOriginal)
        .and_then(|s| parse_exif_datetime(&s))
        .map_or((None, None, None), |(y, m, d)| (Some(y), Some(m), Some(d)));

    Ok(PhotoMetadata {
        camera,
        year,
        month,
        day,
    })
}

/// Reads one EXIF tag's raw ASCII value (no `Display`-formatted quoting
/// or units — just the text), trimmed of surrounding whitespace/padding.
fn exif_ascii(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
    let field = exif.get_field(tag, exif::In::PRIMARY)?;
    let exif::Value::Ascii(strings) = &field.value else {
        return None;
    };
    let s = String::from_utf8_lossy(strings.first()?).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Combines EXIF `Make`/`Model` into one camera name, deduplicating when
/// `Model` already repeats `Make` (common — e.g. Canon writes Model as
/// `"Canon EOS R5"`, which would otherwise become `"Canon Canon EOS R5"`).
fn combine_make_model(make: Option<String>, model: Option<String>) -> Option<String> {
    match (make, model) {
        (Some(make), Some(model)) => {
            if model
                .to_ascii_lowercase()
                .contains(&make.to_ascii_lowercase())
            {
                Some(model)
            } else {
                Some(format!("{make} {model}"))
            }
        }
        (Some(make), None) => Some(make),
        (None, Some(model)) => Some(model),
        (None, None) => None,
    }
}

/// Parses EXIF's `DateTimeOriginal` shape (`"YYYY:MM:DD HH:MM:SS"`,
/// colons instead of hyphens in the date part) into zero-padded
/// `(year, month, day)` strings, without pulling in a date-parsing
/// dependency for this one field. Anything not shaped like that is
/// treated as unavailable rather than guessed at.
fn parse_exif_datetime(s: &str) -> Option<(String, String, String)> {
    let date_part = s.split(' ').next()?;
    let mut parts = date_part.splitn(3, ':');
    let year = parts.next()?;
    let month = parts.next()?;
    let day = parts.next()?;
    if year.len() == 4
        && year.bytes().all(|b| b.is_ascii_digit())
        && month.len() == 2
        && month.bytes().all(|b| b.is_ascii_digit())
        && day.len() == 2
        && day.bytes().all(|b| b.is_ascii_digit())
    {
        Some((year.to_string(), month.to_string(), day.to_string()))
    } else {
        None
    }
}

/// Reads document metadata for `strategy = "documents"`: PDF's `/Info`
/// dictionary, or an Office file's (docx/xlsx/pptx) `docProps/core.xml`
/// — dispatched by extension, since the two are unrelated formats with
/// no shared magic-byte sniffing worth doing here. An unrecognized
/// extension is a clear error, never a silent guess.
pub fn extract_document_metadata(path: &Path) -> Result<DocumentMetadata, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => extract_pdf_metadata(path),
        "docx" | "xlsx" | "pptx" => extract_office_metadata(path),
        other => Err(format!("unsupported document format \".{other}\"")),
    }
}

/// Reads `Author`/`Title`/`CreationDate` from a PDF's `/Info` dictionary
/// via the pure-Rust `lopdf` crate. A PDF with no `/Info` entry at all
/// (valid, if uncommon) yields an all-`None` `DocumentMetadata` rather
/// than an error — the same uniform "missing metadata" outcome every
/// other extractor in this module gives.
fn extract_pdf_metadata(path: &Path) -> Result<DocumentMetadata, String> {
    let doc =
        lopdf::Document::load(path).map_err(|e| format!("cannot read document metadata: {e}"))?;
    let info = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|obj| obj.as_reference().ok())
        .and_then(|id| doc.get_object(id).ok())
        .and_then(|obj| obj.as_dict().ok());
    let Some(info) = info else {
        return Ok(DocumentMetadata::default());
    };

    let author = pdf_info_string(info, b"Author");
    let title = pdf_info_string(info, b"Title");
    let (year, month, day) = pdf_info_string(info, b"CreationDate")
        .and_then(|s| parse_pdf_date(&s))
        .map_or((None, None, None), |(y, m, d)| (Some(y), Some(m), Some(d)));

    Ok(DocumentMetadata {
        author,
        title,
        year,
        month,
        day,
    })
}

/// Reads one `/Info` dictionary entry as text, decoding PDF's two string
/// encodings: UTF-16BE with a leading `FE FF` byte-order mark (used for
/// non-Latin1 text), or plain bytes otherwise (PDFDocEncoding is a
/// superset of Latin-1 for the ASCII range every real-world value in
/// practice uses — a lossy UTF-8 decode is accurate for that common case
/// and never panics on the rest).
fn pdf_info_string(dict: &lopdf::Dictionary, key: &[u8]) -> Option<String> {
    let bytes = dict.get(key).ok()?.as_str().ok()?;
    let text = if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Parses a PDF date string (`"D:YYYYMMDDHHmmSS..."`, optionally with a
/// timezone suffix Sift doesn't need) into `(year, month, day)`. Anything
/// not shaped like that is treated as unavailable rather than guessed at.
fn parse_pdf_date(s: &str) -> Option<(String, String, String)> {
    let s = s.strip_prefix("D:")?;
    let bytes = s.as_bytes();
    if bytes.len() >= 8 && bytes[..8].iter().all(u8::is_ascii_digit) {
        Some((
            s[0..4].to_string(),
            s[4..6].to_string(),
            s[6..8].to_string(),
        ))
    } else {
        None
    }
}

/// Reads `dc:creator`/`dc:title`/`dcterms:created` from `docProps/core.xml`
/// inside an Office Open XML file (docx/xlsx/pptx are all plain zip
/// archives with this same core-properties part) via the pure-Rust `zip`
/// and `roxmltree` crates. A file with no `docProps/core.xml` part at all
/// yields an all-`None` `DocumentMetadata` rather than an error, same as
/// every other "no metadata present" case in this module; a corrupt/
/// unreadable archive is a real error.
fn extract_office_metadata(path: &Path) -> Result<DocumentMetadata, String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("cannot read document metadata: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("cannot read document metadata: {e}"))?;
    let mut core_xml = String::new();
    {
        use std::io::Read;
        let mut entry = match archive.by_name("docProps/core.xml") {
            Ok(entry) => entry,
            Err(zip::result::ZipError::FileNotFound) => return Ok(DocumentMetadata::default()),
            Err(e) => return Err(format!("cannot read document metadata: {e}")),
        };
        entry
            .read_to_string(&mut core_xml)
            .map_err(|e| format!("cannot read document metadata: {e}"))?;
    }
    let xml = roxmltree::Document::parse(&core_xml)
        .map_err(|e| format!("cannot read document metadata: {e}"))?;

    const DC: &str = "http://purl.org/dc/elements/1.1/";
    const DCTERMS: &str = "http://purl.org/dc/terms/";
    let author = core_properties_text(&xml, DC, "creator");
    let title = core_properties_text(&xml, DC, "title");
    let (year, month, day) = core_properties_text(&xml, DCTERMS, "created")
        .and_then(|s| parse_w3cdtf_date(&s))
        .map_or((None, None, None), |(y, m, d)| (Some(y), Some(m), Some(d)));

    Ok(DocumentMetadata {
        author,
        title,
        year,
        month,
        day,
    })
}

/// Finds one `docProps/core.xml` element by its (namespace, local name)
/// and returns its trimmed text content, or `None` if absent/empty.
fn core_properties_text(doc: &roxmltree::Document, ns: &str, local_name: &str) -> Option<String> {
    let text = doc
        .descendants()
        .find(|n| {
            n.is_element()
                && n.tag_name().name() == local_name
                && n.tag_name().namespace() == Some(ns)
        })?
        .text()?
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Parses a W3CDTF date (`dcterms:created`'s shape, e.g.
/// `"2023-11-15T09:00:00Z"`) into `(year, month, day)`. Anything not
/// shaped like that is treated as unavailable rather than guessed at.
fn parse_w3cdtf_date(s: &str) -> Option<(String, String, String)> {
    let date_part = s.split('T').next()?;
    let mut parts = date_part.splitn(3, '-');
    let year = parts.next()?;
    let month = parts.next()?;
    let day = parts.next()?;
    if year.len() == 4
        && year.bytes().all(|b| b.is_ascii_digit())
        && month.len() == 2
        && month.bytes().all(|b| b.is_ascii_digit())
        && day.len() == 2
        && day.bytes().all(|b| b.is_ascii_digit())
    {
        Some((year.to_string(), month.to_string(), day.to_string()))
    } else {
        None
    }
}
