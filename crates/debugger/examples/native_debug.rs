use devforge_debugger::{
    Breakpoint,
    server::{Control, Server},
};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
fn wait(server: &Server, state: &str) {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        let s = server.snapshot();
        if s.state == state {
            return;
        }
        if let Some(e) = s.error {
            panic!("debug protocol error: {e}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("did not reach {state}: {:?}", server.snapshot());
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let php = PathBuf::from(&args[0]);
    let module = PathBuf::from(&args[1]);
    let dir = tempfile::tempdir()?;
    let root = fs::canonicalize(dir.path())?;
    fs::write(
        root.join("sample.php"),
        "<?php\n$api_token='SENTINEL_PRIVATE';\nfunction workload($value) {\n $result=$value*3;\n return $result;\n}\n$input=7;\n$output=workload($input);\necho $output;\n",
    )?;
    let key = "a".repeat(64);
    let server = Server::start(
        &root,
        key.clone(),
        vec![Breakpoint {
            file: "sample.php".into(),
            line: 8,
        }],
        vec!["SENTINEL_PRIVATE".into()],
    )?;
    let mut child = Command::new(php)
        .args(["-n", "-d"])
        .arg(format!("zend_extension={}", module.display()))
        .args([
            "-d",
            "xdebug.mode=debug",
            "-d",
            "xdebug.start_with_request=yes",
            "-d",
            "xdebug.client_host=127.0.0.1",
            "-d",
        ])
        .arg(format!("xdebug.client_port={}", server.snapshot().port))
        .arg("-d")
        .arg(format!("xdebug.idekey={key}"))
        .arg(root.join("sample.php"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    wait(&server, "paused");
    server.control(Control::Run)?;
    thread::sleep(Duration::from_millis(50));
    wait(&server, "paused");
    let at_break = server.snapshot();
    assert!(at_break.frames.iter().any(|f| f.line == 8));
    assert!(serde_json::to_string(&at_break)?.contains("REDACTED"));
    assert!(!serde_json::to_string(&at_break)?.contains("SENTINEL_PRIVATE"));
    server.control(Control::StepInto)?;
    thread::sleep(Duration::from_millis(50));
    wait(&server, "paused");
    assert!(
        server
            .snapshot()
            .frames
            .iter()
            .any(|f| f.function.contains("workload"))
    );
    server.control(Control::StepOver)?;
    thread::sleep(Duration::from_millis(50));
    wait(&server, "paused");
    server.control(Control::Run)?;
    let until = Instant::now() + Duration::from_secs(10);
    while child.try_wait()?.is_none() {
        assert!(Instant::now() < until, "PHP did not finish");
        thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout)?.trim(), "21");
    println!(
        "{{\"actual_xdebug_breakpoint_step_locals\":true,\"secret_redacted\":true,\"completed_php_result\":21}}"
    );
    Ok(())
}
