use crate::domain::*;
use chrono::Utc;
use directories::ProjectDirs;
use serde_json;
use std::fs;
use std::path::PathBuf;

pub fn cmd_history() {
    let hist_dir = get_history_dir();
    let mut items = vec![];
    if let Ok(files) = fs::read_dir(&hist_dir) {
        for f in files.flatten() {
            let fp = f.path();
            if fp.extension().is_some_and(|e| e == "json") {
                if let Ok(s) = fs::read_to_string(&fp) {
                    if let Ok(item) = serde_json::from_str::<HistoryItem>(&s) {
                        items.push(item);
                    }
                }
            }
        }
    }
    for h in items {
        println!("{} {} actions {}", h.id, h.timestamp, h.actions.len());
    }
}

pub fn cmd_undo(id: String) {
    let hist_dir = get_history_dir();
    let fp = hist_dir.join(format!("{}.json", id));
    let data = match fs::read_to_string(&fp) {
        Ok(s) => s,
        Err(_) => {
            println!("record not found");
            return;
        }
    };
    let mut hist: HistoryItem = match serde_json::from_str(&data) {
        Ok(h) => h,
        Err(_) => {
            println!("history parse error");
            return;
        }
    };
    let mut any = false;
    for (i, act) in hist.actions.iter().enumerate() {
        if act.op == Op::Move && act.undoable {
            // Only reverse if src missing, dst present
            if !act.src.exists() && act.dst.as_ref().map(|d| d.exists()).unwrap_or(false) {
                match std::fs::rename(act.dst.as_ref().unwrap(), &act.src) {
                    Ok(_) => {
                        println!(
                            "Undid: {} -> {}",
                            act.dst.as_ref().unwrap().display(),
                            act.src.display()
                        );
                        any = true;
                        // update outcome
                        if let Some(out) = hist.outcomes.get_mut(i) {
                            out.result = Ok(());
                        }
                    }
                    Err(e) => println!("Undo failed {}: {}", act.src.display(), e),
                }
            } else {
                println!("Cannot safely undo move {}", act.src.display());
            }
        } else if act.op == Op::Trash {
            println!("Undo for trash not supported: {}", act.src.display());
        }
    }
    if any {
        record_history(&hist);
    }
}

pub fn record_history(item: &HistoryItem) {
    let hist_dir = get_history_dir();
    let fp = hist_dir.join(format!("{}.json", item.id));
    let tmp = fp.with_extension("tmp");
    let s = serde_json::to_string_pretty(item).expect("history ser");
    std::fs::write(&tmp, &s).expect("write hist tmp");
    std::fs::rename(&tmp, &fp).expect("atomic hist move");
}

pub fn new_history_id() -> String {
    let ts = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("hist-{}", ts)
}

fn get_history_dir() -> PathBuf {
    if let Some(p) = ProjectDirs::from("org", "flokin", "sift") {
        let d = p.data_local_dir().to_path_buf().join("history");
        std::fs::create_dir_all(&d).ok();
        d
    } else {
        PathBuf::from(".sift-history")
    }
}
