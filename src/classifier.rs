use crate::domain::{Category, Entry};
use std::collections::HashMap;

pub struct CategoryDB {
    map: HashMap<&'static str, Category>,
}

impl CategoryDB {
    pub fn new() -> Self {
        use Category::*;
        let mut map = HashMap::new();
        for ext in [
            "jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "tiff", "tif", "heic", "heif",
            "avif", "ico",
        ] {
            map.insert(ext, Image);
        }
        for ext in [
            "stl", "obj", "3mf", "step", "stp", "blend", "blend1", "fbx", "glb", "gltf", "dae",
            "ply",
        ] {
            map.insert(ext, ThreeD);
        }
        for ext in [
            "js", "jsx", "mjs", "cjs", "ts", "tsx", "py", "rs", "go", "php", "dart", "sh", "bash",
            "zsh", "fish", "lua", "java", "c", "h", "cc", "cpp", "cxx", "hpp", "cs", "rb", "swift",
            "kt", "kts", "scala", "vue", "svelte",
        ] {
            map.insert(ext, Code);
        }
        for ext in [
            "json", "jsonl", "yaml", "yml", "toml", "csv", "tsv", "xml", "sql", "sqlite",
            "sqlite3", "db", "parquet", "ndjson",
        ] {
            map.insert(ext, Data);
        }
        for ext in [
            "pdf", "txt", "md", "markdown", "rtf", "doc", "docx", "odt", "xls", "xlsx", "ods",
            "ppt", "pptx", "odp", "epub", "mobi",
        ] {
            map.insert(ext, Document);
        }
        for ext in ["mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "wma"] {
            map.insert(ext, Audio);
        }
        for ext in [
            "mp4", "mov", "mkv", "avi", "webm", "m4v", "mpg", "mpeg", "wmv",
        ] {
            map.insert(ext, Video);
        }
        for ext in [
            "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "tgz", "tbz2", "txz",
        ] {
            map.insert(ext, Archive);
        }
        for ext in ["tmp", "swp", "swo"] {
            map.insert(ext, Junk);
        }
        Self { map }
    }
}

impl Default for CategoryDB {
    fn default() -> Self {
        Self::new()
    }
}

impl CategoryDB {
    /// Classifies an ordinary file by extension (case-insensitive), never by
    /// content. A directory, symlink, or already-protected entry is not an
    /// ordinary classifiable file at all and gets `Unknown`; an ordinary
    /// file with no recognized extension gets the conservative `Other`
    /// fallback rather than `Unknown`, so organize still has somewhere safe
    /// to put it.
    pub fn classify(&self, entry: &mut Entry) {
        if entry.is_symlink || entry.is_dir || entry.protected {
            entry.classified_as = Some(Category::Unknown);
            return;
        }
        let ext = entry
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let cat = self.map.get(ext.as_str()).copied();
        entry.classified_as = Some(cat.unwrap_or(Category::Other));
    }
}
