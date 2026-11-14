use devforge_control::{mutations::*, *};
use devforge_control_adapter::*;
use devforge_workspace::{
    Command,
    worktrees::{DatabaseMode, Input, Record},
};
use serde_json::{Value, json};
fn id(v: &str) -> Id {
    Id::new(v).unwrap()
}
fn hash(v: &[u8]) -> devforge_control::Result<Digest> {
    sha256(v)
}

fn digest() -> Digest {
    Digest::new("a".repeat(64)).unwrap()
}
#[test]
fn reviewed_database_actions_map_only_owned_ids_and_synthesize_consent() {
    let fixture = Fixture::new();
    let binding = fixture.binding("p1", 4);
    let backend = digest();
    let context = BackendContext::reviewed(&backend);
    let bind = fixture.prepare("database.bind", json!({"instance_id":"instance-a"}));
    let Dispatch::Command(command) = map_mutation(&bind, &binding, &context).unwrap() else {
        panic!("commandexpected")
    };
    assert!(
        matches!(*command,Command::SwitchDatabase{id,instance,consent:true}if id=="p1"&&instance=="instance-a")
    );
    let restore = fixture.prepare("database.restore", json!({"backup_id":"backup-a"}));
    let Dispatch::Command(command) = map_mutation(&restore, &binding, &context).unwrap() else {
        panic!("commandexpected")
    };
    assert!(
        matches!(*command,Command::RestoreBackup{id,backup,consent:true}if id=="p1"&&backup=="backup-a")
    );
    assert!(map_mutation(&restore, &binding, &BackendContext::default()).is_err());
}
struct Fixture {
    temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }
    fn binding(&self, project: &str, revision: u64) -> ProjectBinding {
        ProjectBinding::from_registered(id(project), self.temp.path(), revision).unwrap()
    }
    fn prepare(&self, tool: &str, args: Value) -> PreparedMutation {
        let binding = self.binding("p1", 4);
        let grant = Grant::issue(
            id("client"),
            id("p1"),
            vec![
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            1000,
        )
        .unwrap();
        let trust = ProjectTrust::record(&binding);
        let raw = json!({"api_version":1,"request_id":"request","project_id":"p1","expected_revision":4,"idempotency_key":"retry","input":{"tool":tool,"arguments":args}});
        let request: Request<MutationInput> = serde_json::from_value(raw.clone()).unwrap();
        let fingerprint = hash(&fingerprint_material(&request, &binding).unwrap()).unwrap();
        let backend = digest();
        let plan = ReviewedPlan::record(&binding, id(tool), backend.clone(), 1000).unwrap();
        prepare_mutation(
            &serde_json::to_vec(&raw).unwrap(),
            MutationContext {
                binding: &binding,
                grant: &grant,
                client: &id("client"),
                now: 100,
                trust: Some(&trust),
                review: Some(ReviewApproval {
                    plan: &plan,
                    current_backend_digest: &backend,
                    request_fingerprint: &fingerprint,
                }),
            },
            hash,
        )
        .unwrap()
    }
}
fn record(source: &str) -> Record {
    Record {
        isolated: false,
        schema: 1,
        id: "ws1".into(),
        input: Input {
            source_id: source.into(),
            name: "Child".into(),
            branch: "feature".into(),
            base: "main".into(),
            new_branch: true,
            database: DatabaseMode::Empty,
            dependencies: false,
            copy_environment: false,
        },
        source_path: Default::default(),
        common_dir: Default::default(),
        git_dir: None,
        commit: "a".repeat(40),
        domain: "child.devforge.test".into(),
        review_digest: digest().as_str().into(),
        project_id: Some("child".into()),
        stage: "removed".into(),
        last_error: None,
        configuration: devforge_projects::manifest::Manifest::parse(include_bytes!(
            "../../../docs/examples/devforge.yaml"
        ))
        .unwrap(),
    }
}
#[test]
fn project_lifecycle_maps_only_to_registered_identity_and_current_revision() {
    let f = Fixture::new();
    for (tool, action) in [
        ("services.start", "start"),
        ("services.stop", "stop"),
        ("services.restart", "restart"),
    ] {
        let prepared = f.prepare(tool, json!({}));
        let Dispatch::Command(command) =
            map_mutation(&prepared, &f.binding("p1", 4), &BackendContext::default()).unwrap()
        else {
            panic!("command")
        };
        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value, json!({"action":action,"id":"p1"}));
        assert!(matches!(
            map_mutation(&prepared, &f.binding("p2", 4), &BackendContext::default()),
            Err(ErrorCode::ProjectMismatch)
        ));
        assert!(matches!(
            map_mutation(&prepared, &f.binding("p1", 5), &BackendContext::default()),
            Err(ErrorCode::RevisionConflict)
        ));
        let other = Fixture::new();
        assert!(matches!(
            map_mutation(
                &prepared,
                &other.binding("p1", 4),
                &BackendContext::default()
            ),
            Err(ErrorCode::ProjectMismatch)
        ));
    }
}
#[test]
fn runtime_and_configuration_payloads_use_domain_shapes() {
    let f = Fixture::new();
    let binding = f.binding("p1", 4);
    let review = digest();
    let context = BackendContext::reviewed(&review);
    let install = f.prepare("runtimes.install", json!({"package_id":"php-package"}));
    let Dispatch::Command(c) = map_mutation(&install, &binding, &context).unwrap() else {
        panic!()
    };
    assert!(matches!(*c,Command::Install{package} if package=="php-package"));
    let remove = f.prepare("runtimes.remove", json!({"package_id":"php-package"}));
    assert!(matches!(
        map_mutation(&remove, &binding, &BackendContext::default()),
        Err(ErrorCode::Unauthorized)
    ));
    let Dispatch::Command(c) = map_mutation(&remove, &binding, &context).unwrap() else {
        panic!()
    };
    assert!(matches!(*c,Command::RemoveRuntime{package} if package=="php-package"));
    let select=f.prepare("runtimes.select",json!({"name":"Demo","php":"php","node":"node","domain":"demo.devforge.test","https":false,"vite":true,"queue":false}));
    let Dispatch::Command(c) = map_mutation(&select, &binding, &context).unwrap() else {
        panic!()
    };
    assert!(matches!(*c,Command::Configure{id,revision:4,vite:true,..} if id=="p1"));
}
#[test]
fn environment_review_is_server_loaded_and_planning_never_becomes_apply() {
    let f = Fixture::new();
    let binding = f.binding("p1", 4);
    let review = digest();
    let context = BackendContext::reviewed(&review);
    let plan = f.prepare("environment.plan", json!({"write_env":true}));
    assert!(
        matches!(map_mutation(&plan,&binding,&BackendContext::default()).unwrap(),Dispatch::ConfigurationPlan{project_id,write_env:true} if project_id=="p1")
    );
    let apply = f.prepare("environment.apply", json!({"write_env":false}));
    let Dispatch::Command(c) = map_mutation(&apply, &binding, &context).unwrap() else {
        panic!()
    };
    assert!(
        matches!(*c,Command::ConfigurationApply{id,review:ref r,trusted:true,write_env:false} if id=="p1" && r==review.as_str())
    );
    let wrong = Digest::new("b".repeat(64)).unwrap();
    assert!(matches!(
        map_mutation(&apply, &binding, &BackendContext::reviewed(&wrong)),
        Err(ErrorCode::ReviewMismatch)
    ));
    let save = f.prepare(
        "environment.yaml_save",
        json!({"expected":null,"text":"schema: 1\n"}),
    );
    let Dispatch::Command(c) = map_mutation(&save, &binding, &context).unwrap() else {
        panic!()
    };
    assert!(matches!(
        *c,
        Command::ConfigurationSave {
            expected: None,
            consent: true,
            ..
        }
    ));
}
#[test]
fn workspace_lifecycle_requires_registered_source_owner_and_backend_review() {
    let f = Fixture::new();
    let binding = f.binding("p1", 4);
    let review = digest();
    let own = record("p1");
    let other = record("p2");
    for (tool, operation) in [
        ("workspaces.archive", "archive"),
        ("workspaces.restore", "restore"),
        ("workspaces.purge", "purge"),
    ] {
        let prepared = f.prepare(tool, json!({"workspace_id":"ws1"}));
        assert!(matches!(
            map_mutation(&prepared, &binding, &BackendContext::reviewed(&review)),
            Err(ErrorCode::Unauthorized)
        ));
        assert!(matches!(
            map_mutation(
                &prepared,
                &binding,
                &BackendContext::reviewed(&review).with_workspace(&other)
            ),
            Err(ErrorCode::ProjectMismatch)
        ));
        let Dispatch::Command(c) = map_mutation(
            &prepared,
            &binding,
            &BackendContext::reviewed(&review).with_workspace(&own),
        )
        .unwrap() else {
            panic!()
        };
        match *c {
            Command::WorktreeCleanup {
                id, consent: true, ..
            } => {
                assert_eq!(operation, "archive");
                assert_eq!(id, "ws1");
            }
            Command::WorktreeArchive {
                id,
                operation: op,
                consent: true,
                ..
            } => {
                assert_eq!(operation, op);
                assert_eq!(id, "ws1");
            }
            _ => panic!("wrong dispatch"),
        }
    }
}
#[test]
fn domain_failures_never_return_foreign_ids_or_display_text() {
    let f = Fixture::new();
    let binding = f.binding("p1", 4);
    let mut error = devforge_workspace::Error {
        scope: "project",
        code: "stale_revision".into(),
        project_id: None,
    };
    let failure = map_failure(&error, &binding).unwrap();
    assert_eq!(failure.project_id, id("p1"));
    assert!(!failure.retryable);
    error.project_id = Some("p2".into());
    assert!(matches!(
        map_failure(&error, &binding),
        Err(ErrorCode::ProjectMismatch)
    ));
    error.project_id = None;
    error.code = "password=/private/path".into();
    assert!(matches!(
        map_failure(&error, &binding),
        Err(ErrorCode::StorageUnavailable)
    ));
    error.code = "busy".into();
    error.scope = "workspace";
    assert!(map_failure(&error, &binding).unwrap().retryable);
    error.scope = "debugger";
    error.code = "source_outside".into();
    let debugger = map_failure(&error, &binding).unwrap();
    assert_eq!(debugger.scope, DomainScope::Debugger);
    assert_eq!(debugger.code, "source_outside");
    error.scope = "unknown";
    assert!(matches!(
        map_failure(&error, &binding),
        Err(ErrorCode::StorageUnavailable)
    ));
}

#[test]
fn control_names_match_core_preflight_boundaries() {
    for value in [
        "Demo".into(),
        "   ".into(),
        "a".repeat(200),
        "a".repeat(201),
        "é".repeat(100),
        format!("{}a", "é".repeat(100)),
        "a\nb".into(),
    ] {
        assert_eq!(
            ProjectName::new(value.clone()).is_ok(),
            devforge_projects::project::validate_name(&value).is_ok(),
            "name boundary mismatch"
        );
    }
}

#[test]
fn production_fingerprints_use_sha256_known_vectors() {
    assert_eq!(
        sha256(b"").unwrap().as_str(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256(b"abc").unwrap().as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
