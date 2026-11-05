use crate::Result;
use serde_json::{Value, json};
use std::{
    env,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

pub(crate) fn rpc(method: &str, params: Value) -> Result<Value> {
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

pub(crate) fn read_ansi(pane_id: &str) -> Result<String> {
    ansi_text(rpc(
        "pane.read",
        json!({
            "pane_id": pane_id, "source": "visible", "format": "ansi", "strip_ansi": false
        }),
    )?)
}

pub(crate) fn ansi_text(read: Value) -> Result<String> {
    if read["read"]["format"] != "ansi" || read["read"]["truncated"] != false {
        return Err(
            "Herdr did not return a complete ANSI snapshot; refusing a text-only fallback".into(),
        );
    }
    Ok(read["read"]["text"]
        .as_str()
        .ok_or("snapshot text missing")?
        .to_owned())
}
