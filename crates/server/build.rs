//! `web/dist` (Vite のビルドの成果物) のファイルを、`(パス, include_bytes!(...))` の一覧としてサーバーに埋め込むコードを
//! `$OUT_DIR/web_assets.rs` に生成する。`~/.local/bin/tansu` 1 つで画面が出るように、成果物をバイナリに入れる。
//! `web/dist` が無い (Web 画面をビルドせずに Rust だけをビルドした) 時は空の一覧にし、サーバーは画面の代わりに案内を返す。

use std::path::{Path, PathBuf};

fn main() {
    let dist = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo が渡す"))
        .join("../../web/dist");
    // ディレクトリを指定すると、cargo はその下のファイルの変更を見る
    // (https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-changed)
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = Vec::new();
    if dist.is_dir() {
        collect_files(&dist, &mut files);
    }
    files.sort();
    let entries: String = files
        .iter()
        .map(|file| {
            format!(
                "    ({:?}, include_bytes!({:?})),\n",
                relative_path(&dist, file),
                file.display().to_string()
            )
        })
        .collect();
    std::fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo が渡す")).join("web_assets.rs"),
        format!("&[\n{entries}]\n"),
    )
    .expect("OUT_DIR に書ける");
}

/// `directory` の下のファイルを再帰的に `files` へ足す。
fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("web/dist を読める") {
        let path = entry.expect("web/dist を読める").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

/// `file` の `dist` からの相対パス (区切りは `/`)。URL のパスと比べるため。
fn relative_path(dist: &Path, file: &Path) -> String {
    file.strip_prefix(dist)
        .expect("dist の下のファイル")
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
