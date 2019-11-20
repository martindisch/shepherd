use clap::{App, Arg};
use std::process;

use shepherd;

fn main() {
    let matches = App::new(clap::crate_name!())
        .version(clap::crate_version!())
        .about(
            "A distributed video encoder that splits files into chunks \
             for multiple machines.",
        )
        .author(clap::crate_authors!())
        .arg(
            Arg::with_name("clients")
                .short("c")
                .long("clients")
                .value_name("HOSTNAMES")
                .takes_value(true)
                .required(true)
                .help("Comma-separated list of encoding hosts"),
        )
        .arg(
            Arg::with_name("IN")
                .help("The original video file")
                .required(true),
        )
        .arg(
            Arg::with_name("OUT")
                .help("The output video file")
                .required(true),
        )
        .get_matches();
    // If we get here, we know the arguments were supplied, so unwrap is safe
    let input = matches.value_of("IN").unwrap();
    let output = matches.value_of("OUT").unwrap();
    let hosts = matches.value_of("clients").unwrap().split(',');

    if cfg!(debug_assertions) {
        shepherd::run(input, output, hosts.collect()).unwrap();
    } else if let Err(e) = shepherd::run(input, output, hosts.collect()) {
        eprintln!("{}", e);
        process::exit(1);
    }
}
