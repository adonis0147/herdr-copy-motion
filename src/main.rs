mod snapshot;
#[cfg(test)]
mod tests;
mod viewer;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{fs::OpenOptionsExt, net::UnixStream},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const PLUGIN_ID: &str = "herdr-copy-motion";
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    let result = match env::args().nth(1).as_deref() {
        Some("toggle") => toggle(),
        Some("view") => view(),
        _ => Err("usage: herdr-copy-motion <toggle|view>".into()),
    };
    if let Err(error) = result {
        eprintln!("herdr-copy-motion: {error}");
        std::process::exit(1);
    }
}

fn toggle() -> Result<()> {
    let pane_id = env::var("HERDR_PANE_ID")?;
    let marker = viewer_marker(&pane_id)?;
    if let Ok(terminal_id) = fs::read_to_string(&marker) {
        let current = rpc("pane.get", json!({"pane_id": pane_id}))?;
        if current["pane"]["terminal_id"].as_str() == Some(terminal_id.as_str()) {
            rpc("plugin.pane.close", json!({"pane_id": pane_id}))?;
            let _ = fs::remove_file(marker);
            return Ok(());
        }
        fs::remove_file(&marker)?;
    }
    let snapshot = capture(&pane_id)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = state_dir()?.join(format!("snapshot-{}-{stamp}.json", std::process::id()));
    let opened = (|| -> Result<Value> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        serde_json::to_writer(&mut file, &snapshot)?;
        rpc(
            "plugin.pane.open",
            json!({
            "plugin_id": PLUGIN_ID, "entrypoint": "viewer", "placement": "overlay",
            "focus": true,
            "env": {"COPY_MOTION_SNAPSHOT": path.to_str().ok_or("non-UTF-8 snapshot path")?}
            }),
        )
    })();
    if opened.is_err() {
        let _ = fs::remove_file(path);
    }
    opened?;
    Ok(())
}

fn viewer_marker(pane_id: &str) -> Result<PathBuf> {
    // Encode IDs rather than interpreting their separators as path components.
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hash = DefaultHasher::new();
    env::var("HERDR_SOCKET_PATH")?.hash(&mut hash);
    let name: String = pane_id.bytes().map(|b| format!("{b:02x}")).collect();
    Ok(state_dir()?.join(format!("viewer-{:x}-{name}", hash.finish())))
}

fn state_dir() -> Result<PathBuf> {
    let dir = PathBuf::from(env::var("HERDR_PLUGIN_STATE_DIR")?);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn rpc(method: &str, params: Value) -> Result<Value> {
    let mut stream = UnixStream::connect(env::var("HERDR_SOCKET_PATH")?)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    serde_json::to_writer(
        &mut stream,
        &json!({"id": "copy-motion", "method": method, "params": params}),
    )?;
    stream.write_all(b"\n")?;
    let mut response = String::new();
    BufReader::new(stream.take(32 * 1024 * 1024)).read_line(&mut response)?;
    let value: Value = serde_json::from_str(&response)?;
    if let Some(error) = value.get("error") {
        return Err(io::Error::other(format!("{method}: {error}")).into());
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| "missing API result".into())
}

fn capture(pane_id: &str) -> Result<Snapshot> {
    // Reject snapshots captured while the layout or scroll viewport changes.
    for _ in 0..3 {
        let before = rpc("pane.get", json!({"pane_id": pane_id}))?;
        let pane = &before["pane"];
        let layout = rpc("pane.layout", json!({"pane_id": pane_id}))?;
        let rect = layout["layout"]["panes"]
            .as_array()
            .and_then(|panes| panes.iter().find(|p| p["pane_id"] == pane_id))
            .ok_or("source pane is missing from layout")?["rect"]
            .clone();
        let read = rpc(
            "pane.read",
            json!({
                "pane_id": pane_id, "source": "visible", "format": "ansi", "strip_ansi": false
            }),
        )?;
        let after = rpc("pane.get", json!({"pane_id": pane_id}))?;
        let after_layout = rpc("pane.layout", json!({"pane_id": pane_id}))?;
        if pane["scroll"] != after["pane"]["scroll"] || layout != after_layout {
            continue;
        }
        if read["read"]["format"] != "ansi" || read["read"]["truncated"] != false {
            return Err(
                "Herdr did not return a complete ANSI snapshot; refusing a text-only fallback"
                    .into(),
            );
        }
        let rows = dimension(&pane["scroll"]["viewport_rows"])?;
        let outer_rows = dimension(&rect["height"])?;
        let outer_cols = dimension(&rect["width"])?;
        let ansi = read["read"]["text"]
            .as_str()
            .ok_or("snapshot text missing")?;
        // shortcut: infer unexported default blank columns from symmetric borders; exact padding needs a host size API.
        let cols = outer_cols
            .saturating_sub(outer_rows.saturating_sub(rows))
            .max(snapshot::exported_width(ansi))
            .max(1);
        if cols > 4096 {
            return Err("unsupported snapshot width".into());
        }
        return Ok(Snapshot {
            rows,
            cols,
            ansi: ansi.to_owned(),
        });
    }
    Err("pane changed during capture; try again when output is idle".into())
}

fn dimension(value: &Value) -> Result<u16> {
    let n = value.as_u64().ok_or("pane dimensions unavailable")?;
    if n == 0 || n > 4096 {
        return Err("unsupported pane dimensions".into());
    }
    Ok(n as u16)
}

fn view() -> Result<()> {
    let path = PathBuf::from(env::var("COPY_MOTION_SNAPSHOT")?);
    let data = fs::read(&path)?;
    fs::remove_file(&path)?;
    let snapshot: Snapshot = serde_json::from_slice(&data)?;
    if snapshot.rows == 0 || snapshot.cols == 0 || snapshot.rows > 4096 || snapshot.cols > 4096 {
        return Err("invalid snapshot dimensions".into());
    }
    let pane_id = env::var("HERDR_PANE_ID")?;
    let marker = viewer_marker(&pane_id)?;
    let current = rpc("pane.get", json!({"pane_id": pane_id}))?;
    let terminal_id = current["pane"]["terminal_id"]
        .as_str()
        .ok_or("viewer terminal ID missing")?;
    fs::write(&marker, terminal_id)?;
    let result = viewer::run(snapshot);
    let _ = fs::remove_file(marker);
    result
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    rows: u16,
    cols: u16,
    ansi: String,
}
