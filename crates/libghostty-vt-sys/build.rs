use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Pinned ghostty commit. Update this to pull a newer version.
const GHOSTTY_REPO: &str = "https://github.com/ghostty-org/ghostty.git";
const GHOSTTY_COMMIT: &str = "debcffbadb75221a030319c075fae12cfe114176";

fn main() {
    // docs.rs has no Zig toolchain. The checked-in bindings in src/bindings.rs
    // are enough for generating documentation, so skip the entire native
    // build when running under docs.rs.
    if env::var("DOCS_RS").is_ok() {
        return;
    }

    println!("cargo:rerun-if-env-changed=LIBGHOSTTY_VT_SYS_NO_VENDOR");
    println!("cargo:rerun-if-env-changed=GHOSTTY_SOURCE_DIR");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=HOST");
    println!("cargo:rerun-if-changed=crates/libghostty-vt-sys/build.rs");
    println!("cargo:rerun-if-changed=crates/libghostty-vt-sys/patches/search.zig");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let target = env::var("TARGET").expect("TARGET must be set");
    let host = env::var("HOST").expect("HOST must be set");

    // Locate ghostty source: env override > fetch into OUT_DIR.
    let ghostty_dir = match env::var("GHOSTTY_SOURCE_DIR") {
        Ok(dir) => {
            let p = PathBuf::from(dir);
            assert!(
                p.join("build.zig").exists(),
                "GHOSTTY_SOURCE_DIR does not contain build.zig: {}",
                p.display()
            );
            p
        }
        Err(_) => fetch_ghostty(&out_dir),
    };

    patch_ghostty_source(&ghostty_dir, &manifest_dir);

    // Build libghostty-vt via zig.
    let install_prefix = out_dir.join("ghostty-install");

    let mut build = Command::new("zig");
    build
        .arg("build")
        .arg("-Demit-lib-vt")
        .arg("--prefix")
        .arg(&install_prefix)
        .current_dir(&ghostty_dir);

    // Only pass -Dtarget when cross-compiling. For native builds, let zig
    // auto-detect the host (matches how ghostty's own CMakeLists.txt works).
    if target != host {
        let zig_target = zig_target(&target);
        build.arg(format!("-Dtarget={zig_target}"));
    }

    run(build, "zig build");

    let lib_dir = install_prefix.join("lib");
    let include_dir = install_prefix.join("include");

    let lib_name = if target.contains("darwin") {
        "libghostty-vt.0.1.0.dylib"
    } else {
        "libghostty-vt.so.0.1.0"
    };

    assert!(
        lib_dir.join(lib_name).exists(),
        "expected shared library at {}",
        lib_dir.join(lib_name).display()
    );
    assert!(
        include_dir.join("ghostty").join("vt.h").exists(),
        "expected header at {}",
        include_dir.join("ghostty").join("vt.h").display()
    );

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=ghostty-vt");
    println!("cargo:include={}", include_dir.display());
    println!("cargo:rustc-env=LIBGHOSTTY_VT_GHOSTTY_COMMIT={GHOSTTY_COMMIT}");
}

/// Clone ghostty at the pinned commit into OUT_DIR/ghostty-src.
/// Reuses an existing clone if the commit matches.
fn fetch_ghostty(out_dir: &Path) -> PathBuf {
    let src_dir = out_dir.join("ghostty-src");
    let stamp = src_dir.join(".ghostty-commit");

    // Skip fetch if we already have the right commit.
    if stamp.exists()
        && let Ok(existing) = std::fs::read_to_string(&stamp)
        && existing.trim() == GHOSTTY_COMMIT
    {
        return src_dir;
    }

    // Clean and clone fresh.
    if src_dir.exists() {
        std::fs::remove_dir_all(&src_dir)
            .unwrap_or_else(|e| panic!("failed to remove {}: {e}", src_dir.display()));
    }

    eprintln!("Fetching ghostty {GHOSTTY_COMMIT} ...");

    let mut clone = Command::new("git");
    clone
        .arg("clone")
        .arg("--filter=blob:none")
        .arg("--no-checkout")
        .arg(GHOSTTY_REPO)
        .arg(&src_dir);
    run(clone, "git clone ghostty");

    let mut checkout = Command::new("git");
    checkout
        .arg("checkout")
        .arg(GHOSTTY_COMMIT)
        .current_dir(&src_dir);
    run(checkout, "git checkout ghostty commit");

    std::fs::write(&stamp, GHOSTTY_COMMIT).unwrap_or_else(|e| panic!("failed to write stamp: {e}"));

    src_dir
}

fn run(mut command: Command, context: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to execute {context}: {error}"));
    assert!(status.success(), "{context} failed with status {status}");
}

fn zig_target(target: &str) -> String {
    let value = match target {
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        "x86_64-unknown-linux-musl" => "x86_64-linux-musl",
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "aarch64-unknown-linux-musl" => "aarch64-linux-musl",
        "aarch64-apple-darwin" => "aarch64-macos-none",
        "x86_64-apple-darwin" => "x86_64-macos-none",
        other => panic!("unsupported Rust target for vendored build: {other}"),
    };
    value.to_owned()
}

fn patch_ghostty_source(checkout_dir: &Path, sys_crate_dir: &Path) {
    let patch_source = sys_crate_dir.join("patches").join("search.zig");
    let patch_destination = checkout_dir
        .join("src")
        .join("terminal")
        .join("c")
        .join("search.zig");
    let patch_contents = std::fs::read_to_string(&patch_source)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", patch_source.display()));
    let existing_patch = std::fs::read_to_string(&patch_destination).unwrap_or_default();
    if existing_patch != patch_contents {
        std::fs::write(&patch_destination, patch_contents).unwrap_or_else(|error| {
            panic!(
                "failed to write patched Ghostty search API file {}: {error}",
                patch_destination.display()
            )
        });
    }

    let main_zig_path = checkout_dir
        .join("src")
        .join("terminal")
        .join("c")
        .join("main.zig");
    let lib_vt_path = checkout_dir.join("src").join("lib_vt.zig");
    let mut main_zig = std::fs::read_to_string(&main_zig_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", main_zig_path.display()));
    let mut lib_vt = std::fs::read_to_string(&lib_vt_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", lib_vt_path.display()));

    if has_upstream_search_exports(&main_zig, &lib_vt) {
        return;
    }

    for injected in [
        "pub const search = @import(\"search.zig\");\n",
        "pub const terminal_search_matches = search.terminal_search_matches;\n",
        "pub const terminal_selection_string = search.terminal_selection_string;\n",
        "pub const terminal_hyperlink_uri_at = search.terminal_hyperlink_uri_at;\n",
        "    _ = search;\n",
    ] {
        while main_zig.contains(injected) {
            main_zig = main_zig.replacen(injected, "", 1);
        }
    }

    insert_after_first_match(
        &mut main_zig,
        &["pub const terminal = @import(\"terminal.zig\");\n"],
        "pub const search = @import(\"search.zig\");\n",
        "terminal/c/main.zig search import",
    );
    insert_after_first_match(
        &mut main_zig,
        &[
            "pub const terminal_grid_ref = terminal.grid_ref;\n",
            "pub const terminal_get = terminal.get;\n",
        ],
        concat_lines(&[
            "pub const terminal_search_matches = search.terminal_search_matches;",
            "pub const terminal_selection_string = search.terminal_selection_string;",
            "pub const terminal_hyperlink_uri_at = search.terminal_hyperlink_uri_at;",
        ]),
        "terminal/c/main.zig search export",
    );
    insert_after_first_match(
        &mut main_zig,
        &["    _ = terminal;\n"],
        "    _ = search;\n",
        "terminal/c/main.zig test import",
    );
    std::fs::write(&main_zig_path, main_zig)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", main_zig_path.display()));

    for injected in [
        "        @export(&c.terminal_search_matches, .{ .name = \"ghostty_terminal_search_matches\" });\n",
        "        @export(&c.terminal_selection_string, .{ .name = \"ghostty_terminal_selection_string\" });\n",
        "        @export(&c.terminal_hyperlink_uri_at, .{ .name = \"ghostty_terminal_hyperlink_uri_at\" });\n",
    ] {
        while lib_vt.contains(injected) {
            lib_vt = lib_vt.replacen(injected, "", 1);
        }
    }

    insert_after_first_match(
        &mut lib_vt,
        &["        @export(&c.terminal_grid_ref, .{ .name = \"ghostty_terminal_grid_ref\" });\n"],
        concat_lines(&[
            "        @export(&c.terminal_search_matches, .{ .name = \"ghostty_terminal_search_matches\" });",
            "        @export(&c.terminal_selection_string, .{ .name = \"ghostty_terminal_selection_string\" });",
            "        @export(&c.terminal_hyperlink_uri_at, .{ .name = \"ghostty_terminal_hyperlink_uri_at\" });",
        ]),
        "lib_vt.zig terminal search export",
    );
    std::fs::write(&lib_vt_path, lib_vt)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", lib_vt_path.display()));
}

fn insert_after_first_match(
    contents: &mut String,
    anchors: &[&str],
    insertion: impl AsRef<str>,
    context: &str,
) {
    let insertion = insertion.as_ref();
    if contents.contains(insertion) {
        return;
    }

    for anchor in anchors {
        if let Some(index) = contents.find(anchor) {
            contents.insert_str(index + anchor.len(), insertion);
            return;
        }
    }

    panic!("failed to patch Ghostty source ({context}); anchor not found");
}

fn concat_lines(lines: &[&str]) -> String {
    let mut contents = String::new();
    for line in lines {
        contents.push_str(line);
        contents.push('\n');
    }
    contents
}

fn has_upstream_search_exports(main_zig: &str, lib_vt: &str) -> bool {
    let has_main_exports = main_zig.contains("pub const terminal_search_matches")
        && main_zig.contains("pub const terminal_selection_string")
        && main_zig.contains("pub const terminal_hyperlink_uri_at");
    let has_lib_exports = lib_vt.contains("ghostty_terminal_search_matches")
        && lib_vt.contains("ghostty_terminal_selection_string")
        && lib_vt.contains("ghostty_terminal_hyperlink_uri_at");
    has_main_exports && has_lib_exports
}
