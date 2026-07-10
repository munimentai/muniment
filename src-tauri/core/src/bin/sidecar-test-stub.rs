use std::fs;
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("echo") => echo(),
        Some("crash") => std::process::exit(17),
        Some("once") => {
            let marker = args.next().unwrap();
            if fs::create_dir(&marker).is_ok() {
                std::process::exit(18);
            }
            echo();
        }
        Some("probe-once") => {
            let marker = args.next().unwrap();
            let bad = fs::create_dir(&marker).is_ok();
            for line in io::stdin().lock().lines() {
                let line = line.unwrap();
                if line == "ping" && !bad {
                    println!("pong");
                } else if line != "ping" {
                    println!("{line}");
                }
                io::stdout().flush().unwrap();
            }
        }
        Some("pid") => {
            fs::write(args.next().unwrap(), std::process::id().to_string()).unwrap();
            echo();
        }
        Some("hang") => {
            fs::write(args.next().unwrap(), std::process::id().to_string()).unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        _ => std::process::exit(2),
    }
}

fn echo() {
    for line in io::stdin().lock().lines() {
        println!("{}", line.unwrap());
        io::stdout().flush().unwrap();
    }
}
