use std::process::Command;

fn main() {
    let time = Command::new("date")
        .arg("+%Y-%m-%d %H:%M")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=TX81Z_BUILD_TIME={}", time);
    println!("cargo:rerun-if-changed=src/editor.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/midi_worker.rs");
    println!("cargo:rerun-if-changed=src/params.rs");
    println!("cargo:rerun-if-changed=src/sysex.rs");
    println!("cargo:rerun-if-changed=src/ui_widgets.rs");
}
