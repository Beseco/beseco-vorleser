//! Bettet Programmsymbol und Versionsinfo in die EXE ein (Explorer, Eigenschaften-Dialog).
//! Dazu wird `windres` aus MinGW-w64 aufgerufen; fehlt es, entsteht die EXE ohne beides.

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=res/vorleser.ico");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu") {
        println!("cargo:warning=Symbol und Versionsinfo werden nur mit der GNU-Toolchain eingebettet.");
        return;
    }

    let var = |name: &str| env::var(name).unwrap_or_default();
    let version = var("CARGO_PKG_VERSION");
    let numbers = format!(
        "{},{},{},0",
        var("CARGO_PKG_VERSION_MAJOR"),
        var("CARGO_PKG_VERSION_MINOR"),
        var("CARGO_PKG_VERSION_PATCH")
    );
    let icon = format!("{}/res/vorleser.ico", var("CARGO_MANIFEST_DIR")).replace('\\', "/");
    // Nur ASCII, damit die Zeichenkodierung des Ressourcen-Compilers keine Rolle spielt.
    let script = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040704B0"
    BEGIN
      VALUE "CompanyName", "beseco - Beubl IT Systems"
      VALUE "FileDescription", "Vorleser"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Vorleser"
      VALUE "ProductVersion", "{version}"
      VALUE "LegalCopyright", "Florian Beubl, beseco - Beubl IT Systems"
      VALUE "OriginalFilename", "vorleser.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0407, 0x04B0
  END
END
"#
    );

    let out = PathBuf::from(var("OUT_DIR"));
    let source = out.join("vorleser.rc");
    let object = out.join("vorleser_res.o");
    fs::write(&source, script).expect("Ressourcenskript schreiben");
    let compiled = Command::new("windres")
        .arg(&source)
        .args(["-O", "coff", "-o"])
        .arg(&object)
        .status();
    match compiled {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg-bins={}", object.display());
        }
        _ => println!("cargo:warning=windres fehlt oder ist gescheitert; EXE ohne Symbol und Versionsinfo."),
    }
}
