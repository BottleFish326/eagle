use std::path::Path;
use std::process::ExitCode;

use format_worker::open_libheif_worker_bundle;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(directory) = arguments.next() else {
        eprintln!("usage: material-eagle-verify-worker-bundle <bundle-directory>");
        return ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("usage: material-eagle-verify-worker-bundle <bundle-directory>");
        return ExitCode::from(2);
    }
    match open_libheif_worker_bundle(Path::new(&directory)) {
        Ok(worker) => {
            println!("{} {}", worker.provider_id(), worker.provider_version());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("worker bundle rejected: {error}");
            ExitCode::FAILURE
        }
    }
}
