pub fn is_sensitive_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["token", "secret", "password", "key", "env", "credential"]
        .iter()
        .any(|frag| lower.contains(frag))
}
