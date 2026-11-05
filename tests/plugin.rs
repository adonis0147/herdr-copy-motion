use serde_json::{Value, json};
use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Host {
    root: PathBuf,
    socket: PathBuf,
    done: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Host {
    fn new(mut handle: impl FnMut(Value) -> Value + Send + 'static) -> Self {
        let root = std::env::temp_dir().join(format!(
            "copy-motion-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let socket = root.join("socket");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let finished = done.clone();
        let worker = thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut line = String::new();
                        BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        let request: Value = serde_json::from_str(&line).unwrap();
                        let response = handle(request);
                        writeln!(stream, "{response}").unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("mock socket: {e}"),
                }
            }
        });
        Self {
            root,
            socket,
            done,
            thread: Some(worker),
        }
    }

    fn toggle(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_herdr-copy-motion"))
            .arg("toggle")
            .env("HERDR_SOCKET_PATH", &self.socket)
            .env("HERDR_PANE_ID", "w1:p1")
            .env("HERDR_PLUGIN_STATE_DIR", &self.root)
            .output()
            .unwrap()
    }

    fn viewer_marker(&self) -> PathBuf {
        let mut hash = DefaultHasher::new();
        self.socket.to_str().unwrap().hash(&mut hash);
        let pane: String = b"w1:p1".iter().map(|b| format!("{b:02x}")).collect();
        self.root.join(format!("viewer-{:x}-{pane}", hash.finish()))
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        let result = self.thread.take().unwrap().join();
        fs::remove_dir_all(&self.root).unwrap();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn reply(request: &Value, result: Value) -> Value {
    json!({"id": request["id"], "result": result})
}

#[test]
fn captures_before_opening_with_and_without_borders() {
    for (horizontal_inset, vertical_inset) in [(0, 0), (2, 2), (0, 1)] {
        let host = Host::new(move |request| {
            let result = match request["method"].as_str().unwrap() {
                "pane.get" => json!({"pane": {"pane_id": "w1:p1", "scroll": {"viewport_rows": 3}}}),
                "pane.layout" => {
                    assert_eq!(request["params"]["pane_id"], "w1:p1");
                    json!({"layout": {"panes": [{"pane_id": "w1:p1", "rect": {"width": 8 + horizontal_inset, "height": 3 + vertical_inset}}]}})
                }
                "pane.read" => {
                    assert_eq!(request["params"]["format"], "ansi");
                    assert_eq!(request["params"]["source"], "visible");
                    json!({"read": {"format": "ansi", "text": "\u{1b}[42m        \u{1b}[0m", "truncated": false}})
                }
                "plugin.pane.open" => {
                    assert_eq!(request["params"]["placement"], "overlay");
                    assert!(request["params"].get("target_pane_id").is_none());
                    let path = request["params"]["env"]["COPY_MOTION_SNAPSHOT"]
                        .as_str()
                        .unwrap();
                    let snapshot: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                    assert_eq!(snapshot["cols"], 8);
                    assert_eq!(snapshot["rows"], 3);
                    assert_eq!(snapshot["viewport_rows"], 3);
                    assert_eq!(snapshot["viewport_top"], 0);
                    assert_eq!(snapshot["ansi"], "\u{1b}[42m        \u{1b}[0m");
                    assert_eq!(
                        fs::metadata(path).unwrap().permissions().mode() & 0o777,
                        0o600
                    );
                    json!({"type": "plugin_pane_opened"})
                }
                method => panic!("unexpected request: {method}"),
            };
            reply(&request, result)
        });
        let output = host.toggle();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn captures_visible_before_opening_and_defers_history_until_the_source_is_hidden() {
    for fail in [false, true] {
        let host = Host::new(move |request| {
            let result = match request["method"].as_str().unwrap() {
                "pane.get" => json!({"pane": {"terminal_id": "source-terminal", "scroll": {
                    "viewport_rows": 3, "offset_from_bottom": 1, "max_offset_from_bottom": 5
                }}}),
                "pane.layout" => {
                    json!({"layout": {"panes": [{"pane_id": "w1:p1", "rect": {"width": 8, "height": 3}}]}})
                }
                "pane.read" => {
                    assert_eq!(request["params"]["source"], "visible");
                    if fail {
                        return json!({"id": request["id"], "error": {"message": "read failed"}});
                    }
                    json!({"read": {"format": "ansi", "text": "4\n5\n6\n", "truncated": false}})
                }
                "plugin.pane.open" => {
                    assert!(!fail);
                    let path = request["params"]["env"]["COPY_MOTION_SNAPSHOT"]
                        .as_str()
                        .unwrap();
                    let s: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                    assert_eq!(s["rows"], 3);
                    assert_eq!(s["viewport_top"], 0);
                    assert_eq!(s["ansi"], "4\n5\n6\n");
                    assert_eq!(s["source"]["terminal_id"], "source-terminal");
                    assert_eq!(s["source"]["scroll"]["max_offset_from_bottom"], 5);
                    json!({})
                }
                method => panic!("unexpected request: {method}"),
            };
            reply(&request, result)
        });
        let output = host.toggle();
        assert_eq!(
            output.status.success(),
            !fail,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if fail {
            assert!(String::from_utf8_lossy(&output.stderr).contains("read failed"));
        }
    }
}

#[test]
fn refuses_plaintext_export_instead_of_losing_colors() {
    let host = Host::new(|request| {
        let result = match request["method"].as_str().unwrap() {
            "pane.get" => json!({"pane": {"scroll": {"viewport_rows": 3}}}),
            "pane.layout" => {
                json!({"layout": {"panes": [{"pane_id": "w1:p1", "rect": {"width": 8, "height": 3}}]}})
            }
            "pane.read" => {
                json!({"read": {"format": "text", "text": "lost colors", "truncated": false}})
            }
            method => panic!("must not open viewer: {method}"),
        };
        reply(&request, result)
    });
    let output = host.toggle();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("text-only fallback"));
}

#[test]
fn toggling_viewer_queues_quit_instead_of_interrupting_history_restoration() {
    let host = Host::new(|request| {
        assert_eq!(request["params"]["pane_id"], "w1:p1");
        let result = match request["method"].as_str().unwrap() {
            "pane.get" => json!({"pane": {"terminal_id": "viewer-terminal"}}),
            "pane.send-keys" => {
                assert_eq!(request["params"]["keys"], json!(["q"]));
                json!({})
            }
            method => panic!("unexpected request: {method}"),
        };
        reply(&request, result)
    });
    let marker = host.viewer_marker();
    fs::write(&marker, b"viewer-terminal").unwrap();
    assert!(host.toggle().status.success());
    // The viewer removes its own marker when the queued quit is handled.
    assert!(marker.exists());
}

#[test]
fn stale_marker_cannot_close_a_different_terminal() {
    let host = Host::new(|request| match request["method"].as_str().unwrap() {
        "pane.get" => reply(
            &request,
            json!({"pane": {"terminal_id": "different-terminal"}}),
        ),
        "pane.layout" => {
            json!({"id": request["id"], "error": {"code": "test_stop", "message": "capture reached safely"}})
        }
        method => panic!("must not close another terminal: {method}"),
    });
    let marker = host.viewer_marker();
    fs::write(&marker, b"old-terminal").unwrap();
    let output = host.toggle();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("capture reached safely"));
    assert!(!marker.exists());
}

#[test]
fn corrupt_viewer_marker_does_not_open_a_duplicate_overlay() {
    let host = Host::new(|request| panic!("must reject the marker before any API call: {request}"));
    fs::write(host.viewer_marker(), [0xff]).unwrap();
    assert!(!host.toggle().status.success());
    assert!(host.viewer_marker().exists());
}

#[test]
fn replaced_source_terminal_retries_capture_before_opening() {
    let mut gets = 0;
    let mut reads = 0;
    let host = Host::new(move |request| {
        let result = match request["method"].as_str().unwrap() {
            "pane.get" => {
                gets += 1;
                json!({"pane": {"terminal_id": if gets == 1 { "old" } else { "new" }, "scroll": {"viewport_rows": 3}}})
            }
            "pane.layout" => {
                json!({"layout": {"panes": [{"pane_id": "w1:p1", "rect": {"width": 8, "height": 3}}]}})
            }
            "pane.read" => {
                reads += 1;
                json!({"read": {"format": "ansi", "truncated": false, "text": if reads == 1 { "old" } else { "new" }}})
            }
            "plugin.pane.open" => {
                assert_eq!(reads, 2);
                let path = request["params"]["env"]["COPY_MOTION_SNAPSHOT"]
                    .as_str()
                    .unwrap();
                let snapshot: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                assert_eq!(snapshot["ansi"], "new");
                assert_eq!(snapshot["source"]["terminal_id"], "new");
                json!({})
            }
            method => panic!("unexpected request: {method}"),
        };
        reply(&request, result)
    });
    let output = host.toggle();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn failed_launch_removes_private_snapshot() {
    let host = Host::new(|request| {
        let result = match request["method"].as_str().unwrap() {
            "pane.get" => json!({"pane": {"scroll": {"viewport_rows": 3}}}),
            "pane.layout" => {
                json!({"layout": {"panes": [{"pane_id": "w1:p1", "rect": {"width": 8, "height": 3}}]}})
            }
            "pane.read" => {
                json!({"read": {"format": "ansi", "text": "private", "truncated": false}})
            }
            "plugin.pane.open" => {
                return json!({"id": request["id"], "error": {"code": "ui_busy", "message": "modal active"}});
            }
            method => panic!("unexpected request: {method}"),
        };
        reply(&request, result)
    });
    assert!(!host.toggle().status.success());
    assert!(fs::read_dir(&host.root).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("snapshot-")
    }));
}
