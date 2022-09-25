use clap::{App, AppSettings, Arg};
use log::error;
use simplelog::{
    ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode,
};
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
        .setting(AppSettings::TrailingVarArg)
        .arg(
            Arg::with_name("clients")
                .short("c")
                .long("clients")
                .value_name("hostnames")
                .use_delimiter(true)
                .takes_value(true)
                .required(true)
                .help("Comma-separated list of encoding hosts"),
        )
        .arg(
            Arg::with_name("length")
                .short("l")
                .long("length")
                .value_name("seconds")
                .takes_value(true)
                .help("The length of video chunks in seconds"),
        )
        .arg(
            Arg::with_name("tmp")
                .short("t")
                .long("tmp")
                .value_name("path")
                .takes_value(true)
                .help("The path to the local temporary directory"),
        )
        .arg(
            Arg::with_name("keep")
                .short("k")
                .long("keep")
                .help("Don't clean up temporary files"),
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
        .arg(
            Arg::with_name("ffmpeg")
                .value_name("FFMPEG OPTIONS")
                .multiple(true)
                .help(
                    "Options/flags for ffmpeg encoding of chunks. The\n\
                     chunks are video only, so don't pass in anything\n\
                     concerning audio. Input/output file names are added\n\
                     by the application, so there is no need for that\n\
                     either. This is the last positional argument and\n\
                     needs to be preceeded by double hypens (--) as in:\n\
                     shepherd -c c1,c2 in.mp4 out.mp4 -- -c:v libx264\n\
                     -crf 26 -preset veryslow -profile:v high -level 4.2\n\
                     -pix_fmt yuv420p\n\
                     This is also the default that is used if no options\n\
                     are provided.",
                ),
        )
        .get_matches();
    // If we get here, unwrap is safe on mandatory arguments
    let input = matches.value_of("IN").unwrap();
    let output = matches.value_of("OUT").unwrap();
    let hosts = matches.values_of("clients").unwrap().collect();
    let seconds = matches.value_of("length");
    let tmp = matches.value_of("tmp");
    let keep = matches.is_present("keep");
    // Take the given arguments for ffmpeg or use the defaults
    let args: Vec<&str> = matches
        .values_of("ffmpeg")
        .map(|a| a.collect())
        .unwrap_or_else(|| {
            vec![
                "-c:v",
                "libx264",
                "-crf",
                "26",
                "-preset",
                "veryslow",
                "-profile:v",
                "high",
                "-level",
                "4.2",
                "-pix_fmt",
                "yuv420p",
            ]
        });

    TermLogger::init(
        LevelFilter::Info,
        ConfigBuilder::new()
            .set_time_offset_to_local()
            .expect("Unable to determine time offset")
            .build(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .expect("Failed initializing logger");

    if cfg!(debug_assertions) {
        shepherd::run(input, output, &args, hosts, seconds, tmp, keep)
            .unwrap();
    } else if let Err(e) =
        shepherd::run(input, output, &args, hosts, seconds, tmp, keep)
    {
        error!("{}", e);
        process::exit(1);
    }
}
