use std::process::{self, Command};

fn main() {
    println!("Starting dumb-init");

    // Spawn and wait for child
    let mut args = std::env::args().skip(1);
    process::exit(
        Command::new(args.next().unwrap())
            .args(args)
            .spawn()
            .unwrap()
            .wait()
            .unwrap()
            .code()
            .unwrap(),
    );
}
