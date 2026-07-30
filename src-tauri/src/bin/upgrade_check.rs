use std::path::Path;

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        eprintln!("usage: droidsmith-upgrade-check <fixture-dir> <empty-scratch-dir>");
        std::process::exit(2);
    }
    match droidsmith_lib::upgrade::verify_upgrade_fixture(Path::new(&args[0]), Path::new(&args[1]))
    {
        Ok(report) => match serde_json::to_string(&report) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("could not encode upgrade report: {error}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
