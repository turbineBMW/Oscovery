//! Compiles the Blueprint UI files, bundles them with the stylesheet, icons
//! and metainfo into a GResource, and compiles the GSettings schema -- all
//! into OUT_DIR, so `cargo run` works from a checkout with nothing installed.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_ID: &str = "io.github.turbinebmw.Oscovery";
const RESOURCE_PREFIX: &str = "/io/github/turbinebmw/Oscovery";

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo_data = manifest_dir.join("../../data");

    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=resources");
    for name in ["gschema.xml", "metainfo.xml.in"] {
        println!(
            "cargo:rerun-if-changed={}",
            repo_data.join(format!("{APP_ID}.{name}")).display()
        );
    }

    compile_blueprints(&manifest_dir.join("ui"), &out_dir.join("ui"));
    stage_resources(&manifest_dir.join("resources"), &repo_data, &out_dir);
    glib_build_tools::compile_resources(
        &[out_dir.to_str().unwrap()],
        out_dir.join("oscovery.gresource.xml").to_str().unwrap(),
        "oscovery.gresource",
    );
    compile_schemas(&repo_data, &out_dir.join("schemas"));
}

fn compile_blueprints(source_dir: &Path, target_dir: &Path) {
    fs::create_dir_all(target_dir).unwrap();
    let mut inputs: Vec<PathBuf> = fs::read_dir(source_dir)
        .expect("ui/ directory exists")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "blp"))
        .collect();
    inputs.sort();
    let status = Command::new("blueprint-compiler")
        .arg("batch-compile")
        .arg(target_dir)
        .arg(source_dir)
        .args(&inputs)
        .status()
        .expect("blueprint-compiler must be installed (package: blueprint-compiler)");
    assert!(status.success(), "blueprint-compiler failed");
}

/// Copy the static assets next to the compiled UI and write the manifest
/// listing everything, so the gresource compiler sees one flat tree.
fn stage_resources(resource_dir: &Path, repo_data: &Path, out_dir: &Path) {
    fs::copy(resource_dir.join("style.css"), out_dir.join("style.css")).unwrap();
    // GTK searches `<resource base>/icons` like an icon theme directory, so
    // the app's own symbolic icon and its action icons ship inside the binary.
    let mut icon_entries = Vec::new();
    for context in ["actions", "apps"] {
        let source = resource_dir.join(format!("icons/scalable/{context}"));
        let target = out_dir.join(format!("icons/scalable/{context}"));
        fs::create_dir_all(&target).unwrap();
        // An empty context directory is fine (git keeps none).
        let Ok(entries) = fs::read_dir(&source) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "svg") {
                continue;
            }
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            fs::copy(&path, target.join(&name)).unwrap();
            icon_entries.push(format!(
                "    <file preprocess=\"xml-stripblanks\">icons/scalable/{context}/{name}</file>"
            ));
        }
    }
    icon_entries.sort();
    // The metainfo has no placeholders; it ships as written.
    fs::copy(
        repo_data.join(format!("{APP_ID}.metainfo.xml.in")),
        out_dir.join("metainfo.xml"),
    )
    .unwrap();

    let mut ui_entries: Vec<String> = fs::read_dir(out_dir.join("ui"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_str().unwrap().to_string())
        .filter(|name| name.ends_with(".ui"))
        .map(|name| format!("    <file preprocess=\"xml-stripblanks\">ui/{name}</file>"))
        .collect();
    ui_entries.sort();

    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gresources>\n  <gresource prefix=\"{RESOURCE_PREFIX}\">\n{}\n    <file>style.css</file>\n    <file>metainfo.xml</file>\n{}\n  </gresource>\n</gresources>\n",
        ui_entries.join("\n"),
        icon_entries.join("\n")
    );
    fs::write(out_dir.join("oscovery.gresource.xml"), manifest).unwrap();
}

fn compile_schemas(repo_data: &Path, target_dir: &Path) {
    fs::create_dir_all(target_dir).unwrap();
    let status = Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg("--targetdir")
        .arg(target_dir)
        .arg(repo_data)
        .status()
        .expect("glib-compile-schemas must be installed (package: glib2)");
    assert!(status.success(), "glib-compile-schemas failed");
}
