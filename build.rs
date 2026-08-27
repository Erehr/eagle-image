fn main() {
    // napi_build emits link args for the Node addon. The wasm target has no
    // Node addon and those args break wasm-ld, so it is skipped there.
    #[cfg(not(target_arch = "wasm32"))]
    napi_build::setup();

    // A Windows VERSIONINFO resource: company, product, version, description.
    //
    // Generic AV heuristics weight "unsigned PE with no version metadata"
    // heavily, because that is what a dropper looks like and what almost no
    // legitimately-shipped DLL looks like. Rust cdylibs get none of this by
    // default. It is not a signature and guarantees nothing, but it is the
    // cheapest thing that changes how a binary reads to a scanner.
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
