//! Platform-specific background daemon spawning. Only Unix is implemented
//! for this MVP; other platforms get an explicit, honest error instead of
//! a pretend/partial implementation.

/// Finds the `sift` CLI binary to relaunch as the detached daemon.
///
/// This can't just be `std::env::current_exe()`: this function (via
/// `watch::cmd_watch_start`/`cmd_watch_resume`) is called from *any*
/// process linking this library, not only the `sift` CLI itself —
/// notably `sift-tray`, whose "Add folder"/"Resume" actions call the
/// exact same `cmd_watch_*` functions the CLI does (deliberately, to
/// never duplicate watch logic). If `current_exe()` were used blindly,
/// clicking those in `sift-tray` would try to relaunch *`sift-tray`*
/// with `watch daemon run` arguments it doesn't understand — the watch
/// state would flip to "running" in the registry, but no real daemon
/// would ever come up, and nothing would actually get organized until
/// someone ran `sift watch start` from a terminal by hand.
///
/// So: if the currently running executable is already named `sift`,
/// it's used as-is (the common case — the CLI calling this on itself).
/// Otherwise, look for a sibling binary literally named `sift` next to
/// the current executable (covers `sift-tray` sitting next to `sift` in
/// the same `target/release/` or install directory), then fall back to
/// resolving `sift` on `PATH`.
fn find_sift_binary() -> Result<std::path::PathBuf, String> {
    let current = std::env::current_exe().map_err(|e| format!("cannot locate sift binary: {e}"))?;
    if current.file_stem().and_then(|s| s.to_str()) == Some("sift") {
        return Ok(current);
    }
    if let Some(sibling) = current.parent().map(|dir| dir.join("sift")) {
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("sift");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(
        "cannot locate the \"sift\" binary (not the current executable, no sibling next to it, \
         not on PATH)"
            .to_string(),
    )
}

#[cfg(unix)]
pub fn spawn_detached_daemon(log_path: &std::path::Path) -> Result<u32, String> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let exe = find_sift_binary()?;
    let log_out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| format!("cannot open daemon log: {e}"))?;
    let log_err = log_out
        .try_clone()
        .map_err(|e| format!("cannot duplicate daemon log handle: {e}"))?;

    let mut cmd = Command::new(exe);
    cmd.args(["watch", "daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_out))
        .stderr(Stdio::from(log_err));
    // SAFETY: `setsid(2)` is async-signal-safe and takes no arguments; it
    // is called in the forked child, before exec, which is the documented
    // safe window for `pre_exec`. This detaches the daemon into its own
    // session so it survives the invoking terminal closing — without it,
    // `watch start` would effectively require keeping a foreground
    // terminal open, which the product explicitly must not require.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot spawn daemon: {e}"))?;
    Ok(child.id())
}

#[cfg(not(unix))]
pub fn spawn_detached_daemon(_log_path: &std::path::Path) -> Result<u32, String> {
    Err(
        "the background watch daemon is only supported on Unix-like platforms in this version"
            .to_string(),
    )
}
