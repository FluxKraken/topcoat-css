//! Exercise the actual Cargo -> build script -> proc macro contract, including
//! incremental source discovery and diagnostics from the Rust compiler.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn cargo(project: &Path, target: &Path) -> Output {
    Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--offline"])
        .current_dir(project)
        .env("CARGO_TARGET_DIR", target)
        .env_remove("TOPCOAT_CSS_MANIFEST")
        .env_remove("TOPCOAT_CSS_STYLESHEET")
        .output()
        .expect("run cargo")
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn cargo_rebuilds_styles_and_reports_actionable_macro_errors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = tempfile::tempdir().unwrap();
    let directory = project.path();
    fs::create_dir(directory.join("src")).unwrap();
    // Keep fixture artifacts across test runs without sharing the parent Cargo lock.
    let target = root.join("target/cargo-workflow-tests");
    fs::write(
        directory.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "css-workflow-fixture"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
styles = {{ package = "topcoat-css", path = {:?} }}
[build-dependencies]
topcoat-css-build = {{ path = {:?} }}
"#,
            root,
            root.join("build")
        ),
    )
    .unwrap();
    fs::write(
        directory.join("build.rs"),
        r#"
fn main() {
    topcoat_css_build::BuildConfig::new().macro_name("module_css").render().unwrap();
}
"#,
    )
    .unwrap();
    let good = r#"
use styles::css as module_css;
fn main() {
    let style = module_css! { .card { color: red; } };
    println!("{}", style.card);
    println!("{}", include_str!(env!("TOPCOAT_CSS_STYLESHEET")));
}
"#;
    let source = directory.join("src/main.rs");
    fs::write(&source, good).unwrap();
    let initial = success(cargo(directory, &target));
    assert!(initial.contains("card_tc"));
    assert!(initial.contains("color: red"));

    // The new source is intentionally not declared with `mod`: collection is lexical.
    fs::write(
        directory.join("src/added.rs"),
        "fn added() { styles::css! { .added { color: blue; } }; }",
    )
    .unwrap();
    let added = success(cargo(directory, &target));
    assert!(
        added.contains("added_tc"),
        "directory watches must detect new files"
    );
    fs::remove_file(directory.join("src/added.rs")).unwrap();
    let removed = success(cargo(directory, &target));
    assert!(
        !removed.contains("added_tc"),
        "deleted modules must leave the stylesheet"
    );

    fs::write(&source, good.replace("color: red", "color: green")).unwrap();
    let updated = success(cargo(directory, &target));
    assert!(updated.contains("color: green"));
    assert_ne!(initial.lines().next(), updated.lines().next());

    fs::write(&source, good.replace("style.card", "style.cadr")).unwrap();
    let typo = cargo(directory, &target);
    assert!(!typo.status.success());
    assert!(String::from_utf8_lossy(&typo.stderr).contains("no field `cadr`"));

    fs::write(&source, good.replace(".card {", ".user-card, .user_card {")).unwrap();
    let collision = cargo(directory, &target);
    assert!(!collision.status.success());
    assert!(String::from_utf8_lossy(&collision.stderr).contains("both produce Rust field"));

    fs::write(
        &source,
        "fn main() { let _ = styles::css! { .card { color: red; } }; }",
    )
    .unwrap();
    fs::write(directory.join("build.rs"), "fn main() {}\n").unwrap();
    let missing = cargo(directory, &target);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("css! requires a build script"));
}
