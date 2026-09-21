//! Compile the actual documented snippets in their required Cargo/build.rs context.
//! rustdoc's synthetic crates cannot match css! calls to scanned source locations.
use std::{fs, path::Path, process::Command};

fn code_blocks(document: &str, language: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = document.lines();
    while let Some(line) = lines.next() {
        if let Some(fence) = line.strip_prefix("```") {
            let body = lines
                .by_ref()
                .take_while(|line| *line != "```")
                .collect::<Vec<_>>()
                .join("\n");
            if fence.split([',', ' ']).next() == Some(language) {
                blocks.push(body);
            }
        }
    }
    blocks
}

#[test]
fn documented_application_compiles_with_both_build_scripts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs = include_str!("../src/lib.rs")
        .lines()
        .take_while(|line| line.starts_with("//!"))
        .map(|line| line.strip_prefix("//! ").unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let snippets = code_blocks(&docs, "rust");
    let [
        component,
        build,
        layout,
        main,
        syntax,
        literal,
        advanced_build,
    ] = snippets.as_slice()
    else {
        panic!("update the application fixture to exercise every documented Rust snippet");
    };
    // Keep the README's examples covered too, without copying their code into tests.
    assert_eq!(snippets, code_blocks(include_str!("../README.md"), "rust"));
    let manifests = code_blocks(&docs, "toml");
    assert_eq!(manifests, code_blocks(include_str!("../README.md"), "toml"));
    assert_eq!(manifests.len(), 1);
    let dependencies = manifests[0]
        .lines()
        .map(|line| {
            if line.starts_with("topcoat-css =") {
                format!("topcoat-css = {{ path = {root:?} }}")
            } else if line.starts_with("topcoat-css-build =") {
                format!("topcoat-css-build = {{ path = {:?} }}", root.join("build"))
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let project = tempfile::tempdir().unwrap();
    let directory = project.path();
    fs::create_dir(directory.join("src")).unwrap();
    fs::create_dir(directory.join("components")).unwrap();
    fs::write(
        directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"css-doc-examples\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n{dependencies}\n"
        ),
    )
    .unwrap();
    fs::write(directory.join("src/component.rs"), component).unwrap();
    fs::write(directory.join("src/layout.rs"), layout).unwrap();
    // These two snippets are function bodies. Include checks of the documented exports.
    fs::write(
        directory.join("src/syntax.rs"),
        format!("fn example() {{\n{syntax}\nlet _: &str = style.card;\nlet _: &str = style.user_name;\nlet _: &str = style.fade_in;\n}}\n"),
    )
    .unwrap();
    fs::write(
        directory.join("src/literal.rs"),
        format!("fn example() {{\n{literal}\nlet _: &str = style.card;\n}}\n"),
    )
    .unwrap();
    fs::write(
        directory.join("src/main.rs"),
        format!(
            "#![allow(dead_code)]\nmod component;\nmod layout;\nmod syntax;\nmod literal;\n{main}\n"
        ),
    )
    .unwrap();
    // Reuse dependency artifacts without taking the parent test runner's Cargo lock.
    let target = root.join("target/doc-example-tests");
    for script in [build, advanced_build] {
        fs::write(directory.join("build.rs"), script).unwrap();
        // Check, don't run: the documented main starts a server and loads an asset bundle.
        // Cargo can fetch the documentation's Topcoat/Tokio dependencies on the first run.
        let output = Command::new(env!("CARGO"))
            .args(["check", "--quiet", "--all-targets"])
            .current_dir(directory)
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("TOPCOAT_CSS_MANIFEST")
            .env_remove("TOPCOAT_CSS_STYLESHEET")
            .output()
            .expect("check the documented Cargo application");
        assert!(
            output.status.success(),
            "documented build script:\n{script}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
