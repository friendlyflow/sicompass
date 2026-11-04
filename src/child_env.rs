//! What the programs this app starts inherit from it.
//!
//! The dev shell and the Nix package's wrapper set `LD_LIBRARY_PATH` so this binary
//! can find Vulkan and friends. It outranks every other binary's own library paths, so
//! a program started from here (LibreOffice through Enter or Open with, a plugin, a
//! shell) would load this app's libraries instead of its own and die with
//! "version `GLIBC_2.43' not found".
//!
//! The variable has done its job by the time `main` runs: the dynamic loader reads it
//! once, at process start, and keeps using that for every later `dlopen`. So taking it
//! out of the environment costs this process nothing.

const LIBRARY_PATH: &str = "LD_LIBRARY_PATH";

/// Nothing this process starts from now on inherits the library path it was given.
///
/// # Safety
///
/// Changes the environment, so no other thread may exist yet.
pub unsafe fn keep_library_path_to_ourselves() {
    #[cfg(target_os = "linux")]
    // SAFETY: the caller promises this is the only thread.
    unsafe {
        std::env::remove_var(LIBRARY_PATH);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::Command;

    const AS_THE_CHILD: &str = "SICOMPASS_TEST_CHILD_ENV";

    /// Runs in a process of its own, because the environment is shared with every other
    /// test in this one.
    #[test]
    fn a_started_program_does_not_inherit_the_library_path() {
        if std::env::var_os(AS_THE_CHILD).is_some() {
            // SAFETY: this process runs this one test, and has no other test thread
            // touching the environment.
            unsafe { keep_library_path_to_ourselves() };
            let out = Command::new("/bin/sh")
                .args(["-c", "printf %s \"${LD_LIBRARY_PATH-unset}\""])
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), "unset");
            return;
        }
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["a_started_program_does_not_inherit_the_library_path"])
            .env(AS_THE_CHILD, "1")
            .env(LIBRARY_PATH, "/nonexistent")
            .status()
            .unwrap();
        assert!(status.success());
    }
}
