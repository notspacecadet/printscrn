fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/stillshot.ico");
        res.set_manifest_file("packaging/windows/stillshot.exe.manifest");
        res.set("ProductName", "Stillshot");
        res.set("FileDescription", "Stillshot freeze-frame screenshots");
        res.set("LegalCopyright", "Stillshot");
        if let Err(err) = res.compile() {
            println!("cargo:warning=failed to embed Windows icon/manifest: {err}");
        }
    }
    println!("cargo:rerun-if-changed=assets/stillshot.ico");
    println!("cargo:rerun-if-changed=packaging/windows/stillshot.exe.manifest");
}
