use crate::domain::{Category, Entry};
use std::collections::HashMap;

#[derive(Default)]
pub struct CategoryDB {
    map: HashMap<&'static str, Category>,
}

impl CategoryDB {
    pub fn new() -> Self {
        use Category::*;
        let mut map = HashMap::new();
        for ext in ["pdf", "doc", "docx", "txt", "md", "rtf"] {
            map.insert(ext, Document);
        }
        for ext in ["jpg", "jpeg", "png", "gif", "bmp", "tiff", "webp"] {
            map.insert(ext, Image);
        }
        for ext in ["mp4", "mov", "avi", "mkv", "webm"] {
            map.insert(ext, Video);
        }
        for ext in ["mp3", "wav", "flac", "m4a", "aac"] {
            map.insert(ext, Audio);
        }
        for ext in ["zip", "tar", "gz", "bz2", "7z", "rar"] {
            map.insert(ext, Archive);
        }
        for ext in ["obj", "fbx", "gltf", "glb", "stl", "3mf", "step"] {
            map.insert(ext, ThreeD);
        }
        for ext in ["tmp", "swp", "swo"] {
            map.insert(ext, Junk);
        }
        map.insert("log", Junk);
        map.insert("bak", Junk);
        Self { map }
    }
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
        let cat = self.map.get(ext.as_str()).cloned();
        entry.classified_as = cat.or(Some(Category::Unknown));
    }
}
