// Included by the tests that start the example programs.

/// The `target/<profile>` folder that holds `examples/<name>`.
///
/// It is normally the grandparent of the running test. With `build.build-dir` set in a
/// cargo config the test runs from there while cargo still puts finished programs in the
/// target folder, and the build folder only holds them under hashed names.
fn profile_dir() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().unwrap().parent().unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("target")
        });
    if exe.starts_with(&target) {
        profile.to_path_buf()
    } else {
        target.join(profile.file_name().unwrap())
    }
}
