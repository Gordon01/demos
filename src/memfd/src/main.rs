#[cfg(target_os = "linux")]
mod server;

fn main() -> std::process::ExitCode {
    #[cfg(target_os = "linux")]
    let result = server::run();
    #[cfg(not(target_os = "linux"))]
    let result: std::io::Result<()> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "this demo requires Linux",
    ));

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("memfd-server: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
