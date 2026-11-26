use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/brisa.ico");
    println!("cargo:rerun-if-changed=assets/brisa.rc");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let file = output.join(if msvc { "brisa.res" } else { "brisa-icon.o" });
    let tool = if msvc { "rc.exe" } else { "windres.exe" };
    let mut command = Command::new(tool);
    if msvc {
        command
            .arg("/nologo")
            .arg("/fo")
            .arg(&file)
            .arg("assets/brisa.rc");
    } else {
        command
            .args(["-i", "assets/brisa.rc", "-O", "coff", "-o"])
            .arg(&file);
    }
    let result = command
        .status()
        .expect("Não foi possível iniciar o compilador de recursos do Windows");
    assert!(result.success(), "Falha ao compilar o ícone do Brisa.");
    println!("cargo:rustc-link-arg-bins={}", file.display());
}
