//! Optional system tray / menu bar app listing sift's watched folders.
//!
//! Entirely separate from the `sift` CLI binary: this is a thin UI shell
//! over the same `sift` library the CLI already uses
//! (`watch::registry::list`, `watch::cmd_watch_pause`/`cmd_watch_resume`)
//! — it never reimplements watch state transitions, it just calls the
//! exact same functions the CLI calls, so behavior can never drift
//! between the two. Never touches the watch daemon directly either: the
//! daemon polls the registry file on its own (see
//! `sift::watch::daemon::Daemon::reconcile`), so changing a watch's state
//! here is picked up the same way `sift watch pause`/`resume` already is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{
    AboutMetadata, CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use sift::watch::registry::{self, WatchState};

const REFRESH_INTERVAL: Duration = Duration::from_secs(3);

enum UserEvent {
    /// The tray icon itself was clicked/hovered/etc. Nothing in this app
    /// currently reacts to it directly (only the menu items do, via
    /// `MenuEvent`) — it's still routed through the event loop so the
    /// `tray-icon` crate's internal state machine stays correct.
    TrayIconEvent,
    MenuEvent(MenuEvent),
}

/// What clicking one dynamically-built menu item does. Looked up by the
/// `MenuId` `tray-icon`/`muda` hands back in `MenuEvent`, since the menu
/// itself is rebuilt from scratch on every refresh (cheap, and avoids
/// hand-rolled diffing against the registry).
enum Action {
    OpenFolder(PathBuf),
    /// `bool` is whether the watch is currently running (so this toggles
    /// to the opposite state).
    TogglePause(PathBuf, bool),
    /// `bool` is whether the watch is currently recursive (so this
    /// toggles to the opposite scope). Takes effect on the running
    /// daemon's very next reconcile — see
    /// `sift::watch::registry::set_recursive`'s doc comment.
    ToggleRecursive(PathBuf, bool),
    /// Runs one `sift organize --apply` pass on the folder right now,
    /// using whichever `--recursive` scope is currently registered for
    /// it — the same manual "reapply" a user would otherwise have to
    /// reach for a terminal to do.
    Reapply(PathBuf),
    /// Opens a native folder picker, then registers + starts a watch on
    /// whatever folder is chosen — same `--auto-apply`, non-recursive
    /// default `sift watch add <folder> --auto-apply` uses from the CLI.
    AddFolder,
    /// Unregisters a watch (`sift watch remove`'s exact function) —
    /// never touches any file, only the registry entry. Reversible by
    /// just adding the same folder back.
    Remove(PathBuf),
    Quit,
}

/// Acquires the same singleton lock `sift watch start` checks before
/// auto-launching this app (`sift::watch::registry::tray_lock_path()`),
/// so double-launching (manually, or via two watches starting in quick
/// succession) never ends up with two tray icons. The returned `File`
/// must be kept alive for the rest of the process — the OS releases the
/// lock automatically on exit, so there's no explicit unlock/drop needed.
fn acquire_singleton_lock_or_exit() -> std::fs::File {
    use sift::watch::registry;
    if let Err(e) = std::fs::create_dir_all(registry::watch_dir()) {
        eprintln!("sift-tray: cannot create watch dir: {e}");
        std::process::exit(1);
    }
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(registry::tray_lock_path())
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("sift-tray: cannot open lock file: {e}");
            std::process::exit(1);
        }
    };
    if file.try_lock().is_err() {
        eprintln!("sift-tray: another instance is already running, exiting.");
        std::process::exit(0);
    }
    file
}

fn main() {
    let _singleton_lock = acquire_singleton_lock_or_exit();
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |_event| {
        let _ = proxy.send_event(UserEvent::TrayIconEvent);
    }));
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::MenuEvent(event));
    }));

    let mut tray: Option<TrayIcon> = None;
    let mut actions: HashMap<MenuId, Action> = HashMap::new();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + REFRESH_INTERVAL);

        match event {
            Event::NewEvents(StartCause::Init) => {
                let (menu, new_actions) = build_menu();
                actions = new_actions;
                tray = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu))
                        .with_tooltip("sift — watched folders")
                        .with_icon(folder_icon())
                        .build()
                        .expect("failed to build tray icon"),
                );
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                refresh(&tray, &mut actions);
            }
            Event::UserEvent(UserEvent::MenuEvent(event)) => {
                if let Some(action) = actions.get(&event.id) {
                    match action {
                        Action::OpenFolder(path) => open_in_file_manager(path),
                        Action::TogglePause(path, currently_running) => {
                            let path_str = path.to_string_lossy().to_string();
                            if *currently_running {
                                sift::watch::cmd_watch_pause(path_str);
                            } else {
                                sift::watch::cmd_watch_resume(path_str);
                            }
                        }
                        Action::ToggleRecursive(path, currently_recursive) => {
                            let path_str = path.to_string_lossy().to_string();
                            sift::watch::cmd_watch_set_recursive(path_str, !*currently_recursive);
                        }
                        Action::Reapply(path) => {
                            // Use whatever --recursive scope is currently
                            // registered for this watch, same as the
                            // daemon itself would.
                            let recursive = registry::find(path)
                                .ok()
                                .flatten()
                                .map(|e| e.recursive)
                                .unwrap_or(false);
                            sift::planner::cmd_organize(
                                path.to_string_lossy().to_string(),
                                true,
                                false,
                                false,
                                recursive,
                            );
                        }
                        Action::AddFolder => {
                            // Blocks the event loop briefly while the
                            // native dialog is open — fine for a
                            // deliberate, infrequent user action like this.
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                let path_str = folder.to_string_lossy().to_string();
                                if sift::watch::cmd_watch_add(path_str.clone(), true, false) {
                                    // Watch itself never sweeps pre-existing
                                    // files once started — by design, it
                                    // only ever reacts to filesystem events
                                    // from here on (see `watch`'s own
                                    // module docs). Picking a folder in this
                                    // dialog is this app's one deliberate
                                    // authorization gesture (same contract
                                    // as `--auto-apply` on the CLI), so —
                                    // unlike `sift watch add` on the CLI,
                                    // which leaves this to a separate
                                    // explicit `sift organize --apply` —
                                    // run one real organize pass on
                                    // whatever's already there before
                                    // starting the watch, so a folder full
                                    // of existing files doesn't sit
                                    // unorganized until something new
                                    // happens to land in it.
                                    sift::planner::cmd_organize(
                                        path_str.clone(),
                                        true,
                                        false,
                                        false,
                                        false,
                                    );
                                    sift::watch::cmd_watch_start(path_str);
                                }
                            }
                        }
                        Action::Remove(path) => {
                            sift::watch::cmd_watch_remove(path.to_string_lossy().to_string());
                        }
                        Action::Quit => {
                            tray.take();
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                    }
                    refresh(&tray, &mut actions);
                }
            }
            _ => {}
        }
    });
}

/// Rebuilds the menu from the registry's current state and swaps it into
/// the live tray icon. Called on every periodic tick and immediately
/// after any action, so the menu never shows stale state for more than
/// one tick.
fn refresh(tray: &Option<TrayIcon>, actions: &mut HashMap<MenuId, Action>) {
    let Some(tray) = tray else { return };
    let (menu, new_actions) = build_menu();
    tray.set_menu(Some(Box::new(menu)));
    *actions = new_actions;
}

/// Reads `sift::watch::registry::list()` — the same call
/// `sift watch list` makes — and renders "Add folder...", one submenu per
/// watched folder (name + state indicator, "Open folder", a Pause/Resume
/// toggle, and — separated below its own divider, to make an accidental
/// click less likely — "Remove"), then a separator and "Quit". Returns
/// the id → action map needed to interpret `MenuEvent`s from this menu.
fn build_menu() -> (Menu, HashMap<MenuId, Action>) {
    let menu = Menu::new();
    let mut actions = HashMap::new();

    let add_item = MenuItem::new("Add folder…", true, None);
    actions.insert(add_item.id().clone(), Action::AddFolder);
    let _ = menu.append(&add_item);
    let _ = menu.append(&PredefinedMenuItem::separator());

    let watches = registry::list().unwrap_or_default();

    if watches.is_empty() {
        let item = MenuItem::new("No watched folders", false, None);
        let _ = menu.append(&item);
    } else {
        for entry in &watches {
            let indicator = if entry.config_error.is_some() {
                "⚠️"
            } else {
                match entry.state {
                    WatchState::Running => "🟢",
                    WatchState::Paused => "⏸️",
                    WatchState::Stopped => "⚪",
                }
            };
            let name = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.path.display().to_string());
            let submenu = Submenu::new(format!("{indicator} {name}"), true);

            let open_item = MenuItem::new("Open folder", true, None);
            actions.insert(
                open_item.id().clone(),
                Action::OpenFolder(entry.path.clone()),
            );
            let _ = submenu.append(&open_item);

            let reapply_item = MenuItem::new("Reapply now", true, None);
            actions.insert(
                reapply_item.id().clone(),
                Action::Reapply(entry.path.clone()),
            );
            let _ = submenu.append(&reapply_item);

            let running = entry.state == WatchState::Running;
            let toggle_label = if running { "Pause" } else { "Resume" };
            let toggle_item = MenuItem::new(toggle_label, true, None);
            actions.insert(
                toggle_item.id().clone(),
                Action::TogglePause(entry.path.clone(), running),
            );
            let _ = submenu.append(&toggle_item);

            let _ = submenu.append(&PredefinedMenuItem::separator());

            // Rebuilt from scratch on every refresh (like the rest of
            // this menu), so `checked` just reflects the registry as of
            // right now — no manual `set_checked` bookkeeping needed.
            let recursive_item = CheckMenuItem::new("Recursive", true, entry.recursive, None);
            actions.insert(
                recursive_item.id().clone(),
                Action::ToggleRecursive(entry.path.clone(), entry.recursive),
            );
            let _ = submenu.append(&recursive_item);

            let _ = submenu.append(&PredefinedMenuItem::separator());

            let remove_item = MenuItem::new("Remove", true, None);
            actions.insert(remove_item.id().clone(), Action::Remove(entry.path.clone()));
            let _ = submenu.append(&remove_item);

            let _ = menu.append(&submenu);
        }
    }

    let _ = menu.append(&PredefinedMenuItem::separator());
    // A native "About" item — the OS shows its own About dialog (icon,
    // name, version, description) when clicked, so there's no `Action`
    // to route for it here.
    let _ = menu.append(&PredefinedMenuItem::about(None, Some(about_metadata())));
    let quit_item = MenuItem::new("Quit", true, None);
    actions.insert(quit_item.id().clone(), Action::Quit);
    let _ = menu.append(&quit_item);

    (menu, actions)
}

/// "Sift, version x.y.z" and a short description for the native About
/// dialog, using `sift::VERSION` (the actual `sift` package version, not
/// this UI's own) so it can never drift out of sync.
///
/// Author attribution goes in `copyright` rather than `authors` or
/// `comments`: those two are unsupported on macOS's About panel (see
/// `muda::AboutMetadata`'s own field docs), while `copyright` renders
/// verbatim on all three platforms this app ships for.
fn about_metadata() -> AboutMetadata {
    AboutMetadata {
        name: Some("Sift".to_string()),
        version: Some(sift::VERSION.to_string()),
        comments: Some("Local-first, safe CLI for organizing files.".to_string()),
        copyright: Some("A project by Sérgio Cardoso — www.sergiocardoso.dev".to_string()),
        website: Some("https://github.com/sergiocardoso/sift".to_string()),
        website_label: Some("GitHub".to_string()),
        icon: Some(about_dialog_icon()),
        ..Default::default()
    }
}

/// Opens `path` in the OS's native file manager — `open` on macOS,
/// `explorer` on Windows, `xdg-open` on Linux/other Unix. No new
/// dependency: these are the three standard system commands, dispatched
/// by `cfg(target_os)`.
fn open_in_file_manager(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// Sift's branding, embedded at compile time (via `include_bytes!` — no
/// network fetch, no external asset lookup at runtime) from `assets/`:
/// the folder-and-sparkle mark alone for the tray icon (square, since
/// that's what a tray icon slot needs), and the wordmark (mark + "Sift"
/// wordtype) for the About dialog, which has room to show the full
/// brand. Both were pre-processed (trimmed, backgrounds made
/// transparent, downsized) once ahead of time rather than shipping
/// full-resolution source art and resizing it in-process on every
/// launch.
const TRAY_ICON_PNG: &[u8] = include_bytes!("../assets/icon-tray.png");
const ABOUT_ICON_PNG: &[u8] = include_bytes!("../assets/icon-about.png");

/// Decodes an embedded PNG into `(rgba_bytes, width, height)` via the
/// `image` crate (built with only the `png` feature — this app only ever
/// decodes the two PNGs above, never arbitrary user-supplied images).
fn decode_png(bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let img = image::load_from_memory(bytes)
        .expect("embedded icon PNG is a fixed, known-good asset")
        .into_rgba8();
    let (width, height) = img.dimensions();
    (img.into_raw(), width, height)
}

/// The tray icon itself (`tray_icon::Icon`).
fn folder_icon() -> Icon {
    let (rgba, width, height) = decode_png(TRAY_ICON_PNG);
    Icon::from_rgba(rgba, width, height).expect("folder_icon: embedded PNG is always valid")
}

/// The same logo, sized for the About dialog and as the distinct
/// `tray_icon::menu::Icon` type `AboutMetadata::icon` needs — same
/// `from_rgba` shape, just a different concrete type from the `muda`
/// crate `tray_icon::menu` re-exports.
fn about_dialog_icon() -> tray_icon::menu::Icon {
    let (rgba, width, height) = decode_png(ABOUT_ICON_PNG);
    tray_icon::menu::Icon::from_rgba(rgba, width, height)
        .expect("about_dialog_icon: embedded PNG is always valid")
}
