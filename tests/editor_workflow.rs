//! Keep one language server alive while editing with missing or stale build output.
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

struct Analyzer {
    process: Child,
    messages: Receiver<Value>,
    id: u64,
}

impl Drop for Analyzer {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl Analyzer {
    fn start(directory: &Path, target: &Path) -> Self {
        let mut process = Command::new(
            std::env::var_os("RUST_ANALYZER").unwrap_or_else(|| "rust-analyzer".into()),
        )
        .current_dir(directory)
        .env("CARGO_TARGET_DIR", target)
        .env_remove("TOPCOAT_CSS_MANIFEST")
        .env_remove("TOPCOAT_CSS_STYLESHEET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start rust-analyzer");
        let stdout = process.stdout.take().unwrap();
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 {
                        return;
                    }
                    if header == "\r\n" {
                        break;
                    }
                    if let Some(value) = header.strip_prefix("Content-Length:") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    return;
                }
                if sender.send(serde_json::from_slice(&body).unwrap()).is_err() {
                    return;
                }
            }
        });
        Self {
            process,
            messages,
            id: 0,
        }
    }

    fn send(&mut self, message: Value) {
        let body = message.to_string();
        let input = self.process.stdin.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        input.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn receive(&mut self, deadline: Instant) -> Value {
        let message = self
            .messages
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("rust-analyzer response before timeout");
        if message.get("method").is_some() && message.get("id").is_some() {
            // This client supplies initialization options rather than dynamic configuration.
            self.send(json!({"jsonrpc": "2.0", "id": message["id"], "result": null}));
        }
        message
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            self.id += 1;
            self.send(json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params}));
            loop {
                let message = self.receive(deadline);
                if message.get("method").is_none() && message["id"] == self.id {
                    if message["error"]["code"] == -32801 {
                        // A source change can cancel an in-flight analysis request.
                        std::thread::sleep(Duration::from_millis(100));
                        break;
                    }
                    assert!(message.get("error").is_none(), "{method}: {message}");
                    return message["result"].clone();
                }
            }
        }
    }

    fn initialize(&mut self, root_uri: &str) {
        self.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {"experimental": {"serverStatusNotification": true}},
                "initializationOptions": {"checkOnSave": false}
            }),
        );
        self.notify("initialized", json!({}));
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let message = self.receive(deadline);
            if message["method"] == "experimental/serverStatus"
                && message["params"]["quiescent"] == true
            {
                break;
            }
        }
    }

    fn expand(&mut self, uri: &str, source: &str, name: &str) -> String {
        let offset = source.find(name).unwrap();
        let preceding = &source[..offset];
        let line = preceding.bytes().filter(|b| *b == b'\n').count();
        let character = preceding
            .rsplit('\n')
            .next()
            .unwrap()
            .encode_utf16()
            .count();
        let result = self.request(
            "rust-analyzer/expandMacro",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        );
        result["expansion"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: {result}"))
            .into()
    }
}

#[test]
#[ignore = "requires rust-analyzer on PATH (or RUST_ANALYZER)"]
fn rust_analyzer_edits_without_build_environment_or_restarts() {
    check_live_edits(false);
}

#[test]
#[ignore = "requires rust-analyzer on PATH (or RUST_ANALYZER)"]
fn rust_analyzer_edits_with_stale_manifest_without_restarts() {
    check_live_edits(true);
}

fn check_live_edits(stale_manifest: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = tempfile::tempdir().unwrap();
    // Canonicalize for macOS's /var -> /private/var symlink and file URI matching.
    let directory = fs::canonicalize(project.path()).unwrap();
    fs::create_dir(directory.join("src")).unwrap();
    fs::write(
        directory.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "css-live-editor-fixture"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
styles = {{ package = "topcoat-css", path = {root:?} }}
direct_styles = {{ package = "topcoat-css-macro", path = {:?} }}
"#,
            root.join("macro")
        ),
    )
    .unwrap();
    if stale_manifest {
        // Model build output from before any CSS was added. Rebuilding this
        // fixture leaves the editor with a valid but outdated manifest.
        fs::write(
            directory.join("editor-manifest.json"),
            r#"{"version":1,"modules":[]}"#,
        )
        .unwrap();
        fs::write(
            directory.join("build.rs"),
            r#"
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=TOPCOAT_CSS_MANIFEST={}/editor-manifest.json", env!("CARGO_MANIFEST_DIR"));
}
"#,
        )
        .unwrap();
    }
    let source = |css: &str| {
        format!(
            r#"
extern crate self as topcoat;
#[macro_export]
macro_rules! asset {{ ($path:expr) => {{ $path }}; }}
pub mod asset {{ pub use crate::asset; }}
const SHEET: &str = styles::stylesheet!();
const RENAMED_SHEET: &str = styles::stylesheet!(crate);
const DIRECT_SHEET: &str = direct_styles::stylesheet!();
const DIRECT_RENAMED_SHEET: &str = direct_styles::stylesheet!(crate);
fn main() {{
    let _ = styles::css! {{ {css} }};
    let _ = direct_styles::css! {{ {css} }};
}}
"#
        )
    };
    let initial = source(".card { color: red; }");
    fs::write(directory.join("src/main.rs"), &initial).unwrap();
    let root_uri = format!("file://{}", directory.display());
    let uri = format!("{root_uri}/src/main.rs");
    let mut analyzer = Analyzer::start(&directory, &root.join("target/live-editor-tests"));
    analyzer.initialize(&root_uri);
    analyzer.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "rust", "version": 1, "text": initial
        }}),
    );

    for name in [
        "styles::stylesheet!()",
        "styles::stylesheet!(crate)",
        "direct_styles::stylesheet!()",
        "direct_styles::stylesheet!(crate)",
    ] {
        let expansion = analyzer.expand(&uri, &initial, name);
        assert!(!expansion.contains("compile_error"), "{expansion}");
        assert!(!expansion.contains("TOPCOAT_CSS"), "{expansion}");
    }
    for (index, (css, expected)) in [
        (".card { color: red; }", "card"),
        ("", ""),
        (".card { color red; }", "invalid CSS"),
        (".new-card { color: blue; }", "new_card"),
        (r#"".literal, .type { color: green; }""#, "r#type"),
        (".last { display: grid; }", "last"),
    ]
    .into_iter()
    .enumerate()
    {
        let text = source(css);
        analyzer.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": index + 2},
                "contentChanges": [{"text": text}]
            }),
        );
        // Mix saved and unsaved edits, all in the same server process.
        if index % 2 == 0 {
            fs::write(directory.join("src/main.rs"), &text).unwrap();
            analyzer.notify(
                "textDocument/didSave",
                json!({"textDocument": {"uri": uri}}),
            );
        }
        // The direct dependency bypasses the facade's cfg(rust_analyzer) reexport.
        // Both entrypoints must recover without consulting missing or stale output.
        for name in ["styles::css!", "direct_styles::css!"] {
            let expansion = analyzer.expand(&uri, &text, name);
            assert!(
                !expansion.contains("TOPCOAT_CSS_MANIFEST"),
                "{name}: {expansion}"
            );
            assert!(
                !expansion.contains("CSS build manifest"),
                "{name}: {expansion}"
            );
            assert!(
                !expansion.contains("requires a build script"),
                "{name}: {expansion}"
            );
            if expected == "invalid CSS" {
                assert!(expansion.contains(expected), "{name}: {expansion}");
            } else {
                assert!(!expansion.contains("compile_error"), "{name}: {expansion}");
                assert!(expansion.contains(expected), "{name}: {expansion}");
                if expected.is_empty() {
                    assert!(!expansion.contains("pub card"), "{name}: {expansion}");
                }
            }
        }
    }
}
