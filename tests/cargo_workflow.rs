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

    // The same invocation can render independent values without changing its
    // classes or mutating the stylesheet. Also exercise a renamed dependency.
    let dynamic = r#"
use styles::css as module_css;
const STATIC: &str = module_css! { .plain { display: block; } }.plain;
fn header(bg: String, fg: &str) -> (&'static str, String) {
    let mut evaluations = 0;
    let colors = module_css! {
        .header {
            --header-bg: $({ evaluations += 1; bg.as_str() });
            --header-fg: $(fg);
            padding: $(format!("{}px", 16));
            margin: calc($(2) * 1px);
        }
        .header_text { color: var(--header-fg); }
        .style { color: red; }
        .__topcoat_css_style { color: blue; }
    };
    assert_eq!(evaluations, 1);
    assert!(bg.starts_with("hsl(")); // Interpolation borrows rather than moving bg.
    let _: &str = colors.header_text;
    let _: &str = colors.style;
    let _: &str = colors.__topcoat_css_style;
    assert_eq!(colors.clone().style(), colors.style());
    (colors.header, colors.style().to_owned())
}
fn main() {
    let (first_class, first_style) = header("hsl(200 100% 50%)".to_owned(), "white");
    let (second_class, second_style) = header("hsl(60 80% 80%)".to_owned(), "black");
    assert_eq!(first_class, second_class);
    assert_ne!(first_style, second_style);
    let css = include_str!(env!("TOPCOAT_CSS_STYLESHEET"));
    for inline in [&first_style, &second_style] {
        for declaration in inline.split(';').filter(|s| !s.is_empty()) {
            let (name, _) = declaration.split_once(':').unwrap();
            assert!(css.contains(&format!("var({name})")), "{}", css);
        }
    }
    assert!(first_style.contains(":white;"));
    assert!(second_style.contains(":black;"));
    assert!(first_style.contains(":16px;"));
    assert!(first_style.contains(":2;"));
    assert!(STATIC.starts_with("plain_tc"));
    println!("{}", first_class);
    println!("{}", first_style);
    println!("{}", second_style);
}
"#;
    fs::write(&source, dynamic).unwrap();
    let interpolated = success(cargo(directory, &target));
    assert!(interpolated.contains("hsl(200 100% 50%)"));
    assert!(interpolated.contains("hsl(60 80% 80%)"));

    fs::write(&source, dynamic.replace("$(fg)", "$(missing_color)")).unwrap();
    let undefined = cargo(directory, &target);
    assert!(!undefined.status.success());
    assert!(
        String::from_utf8_lossy(&undefined.stderr).contains("cannot find value `missing_color`")
    );

    fs::write(&source, good).unwrap();

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

#[test]
#[ignore = "requires rust-analyzer on PATH"]
fn rust_analyzer_checks_fields_without_source_locations() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = tempfile::tempdir().unwrap();
    let directory = project.path();
    let target = root.join("target/editor-workflow-tests");
    fs::create_dir(directory.join("src")).unwrap();
    fs::write(
        directory.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "css-editor-fixture"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
styles = {{ package = "topcoat-css", path = {root:?} }}
[build-dependencies]
topcoat-css-build = {{ path = {:?} }}
"#,
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
    let good = r##"
use styles::css as module_css;
const CARD: &str = module_css! { .card { color: red; } }.card;
fn main() {
    let style = module_css! { .card { color: red; } };
    assert_ne!(CARD, style.card);
    let literal = module_css!(r#".user-card, .type { color: red; }"#);
    let _: &str = literal.user_card;
    let _: &str = literal.r#type;
    let _ = module_css! { :global(body) { color: red; } };
}
"##;
    let source = directory.join("src/main.rs");
    fs::write(&source, good).unwrap();
    success(cargo(directory, &target));

    let diagnostics = || {
        Command::new("rust-analyzer")
            .arg("diagnostics")
            .arg(directory)
            .args(["--severity", "error"])
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("TOPCOAT_CSS_MANIFEST")
            .env_remove("TOPCOAT_CSS_STYLESHEET")
            .output()
            .expect("run rust-analyzer")
    };
    let output = diagnostics();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The fallback must preserve field checking, not merely silence macro errors.
    fs::write(&source, good.replace("style.card", "style.cadr")).unwrap();
    let output = diagnostics();
    let messages = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{messages}");
    assert!(
        messages.contains("no field") && messages.contains("cadr"),
        "{messages}"
    );
    assert!(!messages.contains("must appear directly"), "{messages}");
}
