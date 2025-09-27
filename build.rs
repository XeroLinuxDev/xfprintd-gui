fn main() {
    // Rebuild if the build script itself changes
    println!("cargo:rerun-if-changed=build.rs");

    // Rebuild if any resource file changes
    fn watch_dir(path: &std::path::Path) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                println!("cargo:rerun-if-changed={}", p.display());
                if p.is_dir() {
                    watch_dir(&p);
                }
            }
        }
    }
    let resources_dir = std::path::Path::new("resources");
    watch_dir(resources_dir);

    glib_build_tools::compile_resources(
        &["resources"],
        "resources/resources.gresource.xml",
        "xyz.xerolinux.fp_gui.gresource",
    );
}
