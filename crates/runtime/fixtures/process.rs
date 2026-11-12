//! Portable integration-test executable. Never installed in the desktop bundle.
use std::{
    io::{self, BufRead, Write},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
#[allow(
    clippy::zombie_processes,
    reason = "Fault-injection fixture deliberately leaves a descendant for the supervisor's crash cleanup test"
)]
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("ready");
    match mode {
        "stdin" => {
            use std::io::Read;
            let mut bytes = String::new();
            io::stdin().read_to_string(&mut bytes).unwrap();
            assert_eq!(bytes, "sensitive database row\n");
            print!("{bytes}");
        }
        "tcp" => {
            thread::sleep(Duration::from_millis(150));
            let listener =
                std::net::TcpListener::bind(("127.0.0.1", args[1].parse::<u16>().unwrap()))
                    .unwrap();
            wait();
            drop(listener);
        }
        "secret" => {
            let value = std::env::var("APP_TEST_SECRET").unwrap();
            let middle = value.len() / 2;
            io::stdout().write_all(&value.as_bytes()[..middle]).unwrap();
            io::stdout().flush().unwrap();
            thread::sleep(Duration::from_millis(30));
            println!("{}", &value[middle..]);
            eprintln!("{value}");
            std::process::exit(17);
        }
        "echo" => {
            println!(
                "{}",
                serde_json::json!({"args":&args[1..],"marker":std::env::var("DEVFORGE_FIXTURE_MARKER").ok(),"inherited":std::env::var("DEVFORGE_MUST_NOT_INHERIT").ok(),"cwd":std::env::current_dir().unwrap()})
            );
        }
        "crash" => {
            println!("crashing");
            std::process::exit(17);
        }
        "tree" | "tree-crash" => {
            let child = Command::new(std::env::current_exe().unwrap())
                .arg("child")
                .stdin(Stdio::null())
                .spawn()
                .unwrap();
            println!("CHILD:{}", child.id());
            println!("FIXTURE_READY");
            io::stdout().flush().unwrap();
            if mode == "tree-crash" {
                thread::sleep(Duration::from_millis(100));
                std::process::exit(19);
            }
            wait();
        }
        "flood" => {
            println!("FIXTURE_READY");
            for _ in 0..100 {
                println!("{}", "x".repeat(4096));
                eprintln!("{}", "y".repeat(4096));
            }
            wait();
        }
        "delayed" => {
            thread::sleep(Duration::from_millis(500));
            println!("FIXTURE_READY");
            wait();
        }
        "stubborn" => {
            #[cfg(unix)]
            unsafe {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
            }
            println!("FIXTURE_READY");
            io::stdout().flush().unwrap();
            loop {
                thread::sleep(Duration::from_secs(1));
            }
        }
        "quiet" | "child" => loop {
            thread::sleep(Duration::from_secs(1));
        },
        _ => {
            println!("FIXTURE_READY");
            io::stdout().flush().unwrap();
            wait();
        }
    }
}
fn wait() {
    for line in io::stdin().lock().lines() {
        if line.ok().as_deref() == Some("quit") {
            return;
        }
    }
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
