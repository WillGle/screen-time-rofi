use std::process::ExitCode;

fn main() -> ExitCode {
    match screen_time::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("screen-time: {err:#}");
            ExitCode::from(1)
        }
    }
}
