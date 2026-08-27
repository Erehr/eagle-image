fn main() {
    // These env vars describe the TARGET. `cfg!(target_arch = ...)` here would
    // describe the HOST, because build.rs is compiled for and run on the machine
    // doing the building — so a cross-build to wasm would still take the native
    // branch. That is not a hypothetical: napi_build::setup() emits
    // package-scoped link args (--export=napi_register_wasm_v1 and friends)
    // which wasm-ld treats as a hard error, so getting this wrong fails the
    // wasm build outright rather than producing something subtly wrong.
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    if target_arch != "wasm32" {
        napi_build::setup();
    }

    // A Windows VERSIONINFO resource: company, product, version, description.
    //
    // Generic AV heuristics weight "unsigned PE with no version metadata"
    // heavily, because that is what a dropper looks like and what almost no
    // legitimately shipped DLL looks like. Rust cdylibs get none of this by
    // default. It is not a signature and guarantees nothing, but it is the
    // cheapest thing that changes how a binary reads to a scanner.
    //
    // Host cfg is correct here, unlike above: the Windows binary is only ever
    // built on a Windows runner, and this must match the gate on the `winres`
    // build-dependency or the crate would not compile elsewhere.
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set("ProductName", "Eagle CBZ Reader image addon");
        res.set("FileDescription", "Image decode, resize and encode for the Eagle CBZ Reader plugin");
        res.set("CompanyName", "grupa.graphnet");
        res.set("LegalCopyright", "MIT licensed");
        res.set("OriginalFilename", "eagle-image.win32-x64-msvc.node");
        res.set("InternalName", "eagle_image");
        if let Err(e) = res.compile() {
            println!("cargo:warning=version resource not embedded: {e}");
        }
    }
}
