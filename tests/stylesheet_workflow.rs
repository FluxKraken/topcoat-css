//! Exercise stylesheet declarations with Topcoat's actual asset parser and binary scanner.
use std::{fs, path::Path, process::Command};

#[test]
fn stylesheet_embeds_generated_file_with_topcoat_012() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = tempfile::tempdir().unwrap();
    let directory = project.path();
    fs::create_dir(directory.join("src")).unwrap();
    let target = root.join("target/stylesheet-workflow-tests");

    // Cover both the default entrypoint and genuinely renamed Cargo dependencies.
    for topcoat in ["topcoat", "my_topcoat"] {
        fs::write(
            directory.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "css-stylesheet-fixture"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
{topcoat} = {{ package = "topcoat", version = "=0.12.0", default-features = false, features = ["asset"] }}
styles = {{ package = "topcoat-css", path = {root:?} }}
[build-dependencies]
topcoat-css-build = {{ path = {:?} }}
"#,
                root.join("build")
            ),
        )
        .unwrap();
        let argument = if topcoat == "topcoat" { "" } else { topcoat };
        fs::write(
            directory.join("src/main.rs"),
            format!(
                r#"
use {topcoat}::asset::{{Asset, RawAsset}};
const SHEET: Asset = styles::stylesheet!({argument});
fn main() {{
    let classes = styles::css! {{ .card {{ color: red; }} }};
    let binary = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    let assets = RawAsset::find_in_binary(&binary);
    let sheet = assets.iter().find(|asset| asset.id() == SHEET.id()).unwrap();
    assert_eq!(sheet.path(), env!("TOPCOAT_CSS_STYLESHEET"));
    let css = std::fs::read_to_string(sheet.path()).unwrap();
    assert_eq!(css, include_str!(env!("TOPCOAT_CSS_STYLESHEET")));
    assert!(css.contains(classes.card));
    println!("{{}}", sheet.path());
}}
"#
            ),
        )
        .unwrap();

        // A custom output directory must be honored, including after an incremental
        // rebuild changes the environment. Spaces exercise literal path handling.
        for output_dir in ["css output", "moved css output"] {
            fs::write(
                directory.join("build.rs"),
                format!(
                    r#"
fn main() {{
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    topcoat_css_build::BuildConfig::new().out_dir(out.join({output_dir:?})).render().unwrap();
}}
"#
                ),
            )
            .unwrap();
            let output = Command::new(env!("CARGO"))
                .args(["run", "--quiet"])
                .current_dir(directory)
                .env("CARGO_TARGET_DIR", &target)
                .env_remove("TOPCOAT_CSS_MANIFEST")
                .env_remove("TOPCOAT_CSS_STYLESHEET")
                .output()
                .expect("run the Topcoat stylesheet fixture");
            assert!(
                output.status.success(),
                "{topcoat}, {output_dir}:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let path = String::from_utf8(output.stdout).unwrap();
            assert!(Path::new(path.trim()).ends_with(Path::new(output_dir).join("topcoat.css")));
        }
    }
}
