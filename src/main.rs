use std::env;
use tidymedia::Config;

fn main() {
    let config = Config::new(env::args()).expect("err");
    tidymedia::run(config);
    // let crc32_table = crc32::initialize();

    //
    // for (key, value) in env::vars() {
    //     println!("{}: {}", key, value);
    // }
}
