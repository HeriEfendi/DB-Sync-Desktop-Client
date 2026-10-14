/// Helper utilities to construct child process commands with `CREATE_NO_WINDOW` on Windows.
/// On Windows, GUI applications spawning console processes (such as mysql, mariadb, docker, taskkill)
/// without CREATE_NO_WINDOW cause a new cmd.exe/conhost console window to open and flash.
/// This module ensures all child processes spawned by DB-Sync are executed silently in the background.

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

#[inline]
pub fn new_std_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[inline]
pub fn new_tokio_command<S: AsRef<std::ffi::OsStr>>(program: S) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
