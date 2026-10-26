fn main() {
    // Linux packages install the speech engine libraries in /usr/lib/readit; AppImages next to the binary.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN/../lib/readit:/usr/lib/readit:$ORIGIN");
    }
    tauri_build::build()
}
