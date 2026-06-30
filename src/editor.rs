use std::path::Path;
use std::process::{Child, Command};

pub fn open_external_editor<P: AsRef<Path>>(path: P) -> std::io::Result<Child> {
    let p = path.as_ref();

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(p).spawn()
    }

    #[cfg(target_os = "freebsd")]
    {
        Command::new("xdg-open")
            .arg(p)
            .spawn()
            .or_else(|_| Command::new("gio").arg("open").arg(p).spawn())
    }

    #[cfg(target_os = "openbsd")]
    {
        Command::new("xdg-open")
            .arg(p)
            .spawn()
            .or_else(|_| Command::new("open").arg(p).spawn())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(p).spawn()
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(&["/C", "start", "", &p.to_string_lossy()])
            .spawn()
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "macos",
        target_os = "windows"
    )))]
    {
        Command::new("xdg-open")
            .arg(p)
            .spawn()
            .or_else(|_| Command::new("open").arg(p).spawn())
    }
}
