use clap::{Arg, Command};
use std::env;

use tidymedia::Config;

fn main() {
    let mut cmd = Command::new("tidymedia")
        .about("A command line tool to tidy media files")
        // .author(crate_authors!())
        // .version(version_info.as_ref())
        // .long_version(version_info.as_ref())
        .arg(
            Arg::new("config")
                .short('C')
                .long("config")
                .value_name("FILE")
                .help("Set the configuration file")
                .takes_value(true),
        )
        .arg(
            Arg::new("config-check")
                .required(false)
                .long("config-check")
                .takes_value(false)
                .help("Check config file validity and exit"),
        )
        .arg(
            Arg::new("log-level")
                .short('L')
                .long("log-level")
                .alias("log")
                .takes_value(true)
                .value_name("LEVEL")
                .possible_values(&[
                    "trace", "debug", "info", "warn", "warning", "error", "critical", "fatal",
                ])
                .help("Set the log level"),
        )
        .arg(
            Arg::new("log-file")
                .short('f')
                .long("log-file")
                .takes_value(true)
                .value_name("FILE")
                .help("Sets log file")
                .long_help("Set the log file path. If not set, logs will output to stderr"),
        )
        .arg(
            Arg::new("data-dir")
                .long("data-dir")
                .short('s')
                .alias("store")
                .takes_value(true)
                .value_name("PATH")
                .help("Set the directory used to store data"),
        );

    //     if cmd.is_set(String::from("help")) {
    _ = cmd.print_long_help();
    //     }
    // let matches =  cmd.get_matches();
    let config = Config::new(env::args()).expect("err");
    tidymedia::run(config);
    // let crc32_table = crc32::initialize();

    //
    // for (key, value) in env::vars() {
    //     println!("{}: {}", key, value);
    // }
}
