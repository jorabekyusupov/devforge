//! Optional actual PHP/Laravel companion integration; synthetic and loopback only.
use super::probe;
use devforge_inspector::metadata::{ActivityKind, MetadataItem, read};
use std::{
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct PhpServer(Child);
impl Drop for PhpServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires DEVFORGE_A08_PHP and an offline Laravel DEVFORGE_A08_VENDOR"]
fn actual_probe_laravel_capture_and_activity_observation() {
    let php = std::env::var_os("DEVFORGE_A08_PHP").expect("explicit PHP executable");
    let vendor = std::env::var_os("DEVFORGE_A08_VENDOR").expect("explicit offline Laravel vendor");
    assert!(Path::new(&vendor).join("autoload.php").is_file());
    let fixture = tempfile::tempdir().unwrap();
    let root_owned = std::fs::canonicalize(fixture.path()).unwrap();
    let root = root_owned.as_path();
    let projects = root.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut collector =
        devforge_inspector::Collector::open(&root.join("inspector"), &projects).unwrap();
    let project = "1".repeat(32);
    collector.enable(&project).unwrap();
    let session = devforge_inspector::read_session(&projects, &project)
        .unwrap()
        .unwrap();
    let context = "a".repeat(32);
    let kind = ActivityKind::Inspector {
        context: Some(context.clone()),
    };
    let before = read(root, &project, &kind).unwrap();
    let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let companion = repo.join("packages/laravel-inspector/src");
    let router = repo.join("packages/laravel-inspector/tests/probe_http_router.php");
    let mut command = Command::new(php);
    command
        .env_clear()
        .arg("-S")
        .arg(format!("127.0.0.1:{port}"))
        .arg(router)
        .env("DF_VENDOR", vendor)
        .env("DF_COMPANION", companion)
        .env("DF_ENDPOINT", session.endpoint)
        .env("DF_TOKEN", session.token)
        .env("DF_PROJECT", &project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for key in ["SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut server = PhpServer(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok() {
            break;
        }
        assert!(
            server.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
            "synthetic PHP server did not start"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let result = probe(
        "probe.devforge.test",
        port,
        false,
        None,
        "/fixture",
        &context,
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(result.status, 200);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let after = read(root, &project, &kind).unwrap();
        if after.rows.iter().any(|row| row.sequence > before.high_water && matches!(&row.item, MetadataItem::Inspector {context_id, method, status, ..} if context_id == &context && method == "GET" && *status == Some(200))) { break; }
        assert!(
            Instant::now() < deadline,
            "matching request was not observed by A05 source"
        );
        thread::sleep(Duration::from_millis(25));
    }
}
