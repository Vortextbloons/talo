//! Image attachments for chat messages.
//!
//! Bytes arrive inline from the composer, are validated here, and land in Pi's own working
//! directory (`agent-workspace/attachments/<conversation>/…`). That placement is deliberate:
//! the model gets the image as native content, and a model without image input can still open
//! the file with its own tools because the path sits inside its cwd. SQLite keeps only the
//! metadata (`services/storage/schema.sql`), so a projection rebuild never loses a picture.
use std::fs;
use std::path::{Path, PathBuf};

/// Pasting four screenshots in one turn is generous; more turns a message into a file dump.
pub const MAX_IMAGES: usize = 4;
/// One image after base64 decoding. Larger pictures are resized by Pi before they reach a model.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Everything attached to one turn, decoded.
pub const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
/// Past this size the UI shows a file chip instead of a picture.
pub const MAX_DISPLAY_BYTES: i64 = 6 * 1024 * 1024;

/// Raster formats every bundled provider accepts. SVG is excluded on purpose: it is a
/// document that can carry script, not a picture.
const MIME_EXTENSIONS: [(&str, &str); 4] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/webp", "webp"),
    ("image/gif", "gif"),
];

/// An image that passed validation and now exists on disk.
pub struct StoredImage {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub bytes: usize,
    pub path: PathBuf,
    /// Base64 without the `data:` prefix, ready for the worker's prompt params.
    pub data: String,
}

/// Rejects anything that is not a small raster image before a byte touches the disk.
pub fn validate<'a>(
    images: impl IntoIterator<Item = (&'a str, &'a str, Option<&'a str>)>,
) -> Result<Vec<(String, String, Vec<u8>, String)>, String> {
    let mut accepted = Vec::new();
    let mut total = 0usize;
    for (mime_type, data, name) in images {
        if accepted.len() >= MAX_IMAGES {
            return Err(format!("Attach up to {MAX_IMAGES} images per message"));
        }
        let mime = mime_type.trim().to_ascii_lowercase();
        let extension = MIME_EXTENSIONS
            .iter()
            .find(|(known, _)| *known == mime)
            .map(|(_, extension)| *extension)
            .ok_or("Images must be PNG, JPEG, WebP or GIF")?;
        let bytes = decode_base64(data).ok_or("That image could not be read")?;
        if bytes.is_empty() {
            return Err("That image is empty".into());
        }
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("Images must be 8 MB or smaller".into());
        }
        total += bytes.len();
        if total > MAX_TOTAL_BYTES {
            return Err("Those images are too large together".into());
        }
        accepted.push((mime, extension.to_string(), bytes, clean_name(name, extension)));
    }
    Ok(accepted)
}

/// Writes each validated image under Pi's workspace and returns what the worker and UI need.
pub fn store(
    workspace: &Path,
    conversation_id: &str,
    images: Vec<(String, String, Vec<u8>, String)>,
) -> Result<Vec<StoredImage>, String> {
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let directory = workspace.join("attachments").join(conversation_id);
    fs::create_dir_all(&directory).map_err(|error| format!("Could not store that image: {error}"))?;
    let mut stored = Vec::new();
    for (mime_type, extension, bytes, name) in images {
        let id = uuid::Uuid::new_v4().to_string();
        let path = directory.join(format!("{id}.{extension}"));
        fs::write(&path, &bytes).map_err(|error| format!("Could not store that image: {error}"))?;
        stored.push(StoredImage {
            id,
            name,
            mime_type,
            bytes: bytes.len(),
            path,
            data: encode_base64(&bytes),
        });
    }
    Ok(stored)
}

/// A native URL for an image already on disk, or `None` when it is too big to preview.
pub fn display_url(id: &str, bytes: i64) -> Option<String> {
    if bytes > MAX_DISPLAY_BYTES {
        return None;
    }
    Some(if cfg!(windows) { format!("http://attachment.localhost/{id}") } else { format!("attachment://localhost/{id}") })
}

pub fn preview(db: &rusqlite::Connection, id: &str) -> Option<(String, Vec<u8>)> {
    use std::io::Read;
    let (path, mime, bytes): (String, String, i64) = db.query_row(
        "SELECT path,mime_type,bytes FROM chat_attachments WHERE id=?1",
        [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).ok()?;
    if bytes > MAX_DISPLAY_BYTES || !MIME_EXTENSIONS.iter().any(|(known, _)| *known == mime) { return None; }
    let mut content = Vec::new();
    fs::File::open(path).ok()?.take(MAX_DISPLAY_BYTES as u64 + 1).read_to_end(&mut content).ok()?;
    if content.len() > MAX_DISPLAY_BYTES as usize { return None; }
    Some((mime, content))
}

fn clean_name(name: Option<&str>, extension: &str) -> String {
    let trimmed = name.unwrap_or("").trim();
    if trimmed.is_empty() {
        return format!("image.{extension}");
    }
    let base: String = trimmed
        .chars()
        .filter(|character| !matches!(character, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .take(80)
        .collect();
    if base.is_empty() {
        format!("image.{extension}")
    } else {
        base
    }
}

pub fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0] as u32;
        let second = chunk.get(1).map(|value| *value as u32).unwrap_or(0);
        let third = chunk.get(2).map(|value| *value as u32).unwrap_or(0);
        let combined = (first << 16) | (second << 8) | third;
        out.push(ALPHABET[(combined >> 18) as usize & 63] as char);
        out.push(ALPHABET[(combined >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(combined >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[combined as usize & 63] as char } else { '=' });
    }
    out
}

pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut values = Vec::with_capacity(text.len() * 3 / 4 + 3);
    let mut padding = 0usize;
    let mut saw_padding = false;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                saw_padding = true;
                padding += 1;
                continue;
            }
            b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        };
        if saw_padding {
            return None;
        }
        values.push(value);
    }
    if padding > 2 || values.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(values.len() / 4 * 3 + 3);
    for chunk in values.chunks(4) {
        let mut combined = 0u32;
        for (index, value) in chunk.iter().enumerate() {
            combined |= (*value as u32) << (18 - 6 * index);
        }
        for index in 0..chunk.len() - 1 {
            out.push((combined >> (16 - 8 * index)) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_tail_length() {
        for length in 0..32usize {
            let bytes: Vec<u8> = (0..length).map(|index| (index * 37 % 256) as u8).collect();
            let encoded = encode_base64(&bytes);
            assert_eq!(decode_base64(&encoded), Some(bytes), "length {length}");
        }
    }

    #[test]
    fn decodes_without_padding_and_rejects_junk() {
        assert_eq!(decode_base64("aGk"), Some(b"hi".to_vec()));
        assert_eq!(decode_base64("aGk="), Some(b"hi".to_vec()));
        assert_eq!(decode_base64("aGk!!"), None);
    }

    #[test]
    fn validation_names_and_limits() {
        let ok = validate([("image/png", "aGk=", Some("../../shot.png"))]).unwrap();
        assert_eq!(ok.len(), 1);
        assert!(!ok[0].3.contains('/') && ok[0].3.ends_with("shot.png"));
        assert!(validate([("image/svg+xml", "aGk=", None)]).is_err());
        assert!(validate([("image/png", "aGk=", None), ("image/png", "aGk=", None),
            ("image/png", "aGk=", None), ("image/png", "aGk=", None), ("image/png", "aGk=", None)]).is_err());
    }

    #[test]
    fn stores_inside_the_workspace() {
        let root = std::env::temp_dir().join(format!("talo-attachments-{}", uuid::Uuid::new_v4()));
        let images = validate([("image/png", "aGk=", Some("shot.png"))]).unwrap();
        let stored = store(&root, "conversation-1", images).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].path.starts_with(root.join("attachments").join("conversation-1")));
        assert_eq!(fs::read(&stored[0].path).unwrap(), b"hi");
        assert_eq!(stored[0].data, "aGk=");
        assert!(display_url("image-1", 2).unwrap().ends_with("/image-1"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn previews_read_only_known_bounded_images_and_metadata_stays_scoped() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(crate::storage::SCHEMA).unwrap();
        db.execute("INSERT INTO chat_conversations(id,title,created_at,updated_at) VALUES('c','Chat','',''),('other','Other','','')", []).unwrap();
        let path = std::env::temp_dir().join(format!("talo-preview-{}", uuid::Uuid::new_v4()));
        fs::write(&path, b"image bytes").unwrap();
        for (id, conversation, generation, mime) in [("image", "c", "g", "image/png"), ("other-turn", "c", "old", "image/png"), ("other-chat", "other", "g", "image/png"), ("invalid", "c", "g", "text/html")] {
            db.execute("INSERT INTO chat_attachments(id,conversation_id,generation_id,name,mime_type,bytes,path,created_at) VALUES(?1,?2,?3,'image',?4,11,?5,'')", rusqlite::params![id,conversation,generation,mime,path.to_str().unwrap()]).unwrap();
        }
        assert_eq!(preview(&db, "image"), Some(("image/png".into(), b"image bytes".to_vec())));
        assert!(preview(&db, "../image").is_none());
        assert!(preview(&db, "invalid").is_none());
        let images = crate::storage::list_attachments(&db, "c", &["g"]).unwrap();
        assert_eq!(images.iter().map(|row| row.info.id.as_str()).collect::<Vec<_>>(), ["image", "invalid"]);
        assert!(crate::storage::list_attachments(&db, "c", &[]).unwrap().is_empty());
        fs::write(&path, vec![0; MAX_DISPLAY_BYTES as usize + 1]).unwrap();
        assert!(preview(&db, "image").is_none());
        fs::remove_file(&path).unwrap();
        assert!(preview(&db, "image").is_none());
    }
}
