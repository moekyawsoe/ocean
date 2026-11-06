fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Locate ocean binary (check sibling directory of this executable first)
    let current_exe = std::env::current_exe().ok();
    let ocean_exe = current_exe
        .as_ref()
        .and_then(|p| p.parent())
        .map(|dir| dir.join("ocean"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("ocean"));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new(&ocean_exe);
        cmd.arg("file").args(&args);
        let err = cmd.exec();
        eprintln!("Failed to exec ocean: {err}");
        std::process::exit(1);
    }

    #[cfg(not(unix))]
    {
        let mut cmd = std::process::Command::new(&ocean_exe);
        cmd.arg("file").args(&args);
        match cmd.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(0)),
            Err(err) => {
                eprintln!("Failed to run ocean: {err}");
                std::process::exit(1);
            }
        }
    }
}
