//! Platform-specific background daemon spawning. Only Unix is implemented
//! for this MVP; other platforms get an explicit, honest error instead of
//! a pretend/partial implementation.

#[cfg(unix)]
pub fn spawn_detached_daemon(log_path: &std::path::Path) -> Result<u32, String> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let exe = std::env::current_exe().map_err(|e| format!("cannot locate sift binary: {e}"))?;
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
