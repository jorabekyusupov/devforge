use devforge_control::{ErrorCode, Id};
use devforge_manager::{
    external_control::ExternalReply,
    mcp_contract::{Action, LEGACY_VERSION, MODERN_VERSION, Protocol, ToolCall},
};
use serde_json::{Value, json};
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn modern(id: Value, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":MODERN_VERSION,"io.modelcontextprotocol/clientCapabilities":{}});
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn reply(action: Action) -> Value {
    match action {
        Action::Reply(value) => value,
        _ => panic!("expected protocol reply"),
    }
}
fn call(action: Action) -> ToolCall {
    match action {
        Action::Call(value) => value,
        _ => panic!("expected authorized dispatch"),
    }
}
fn fixture(call: &ToolCall, project: &str) -> ExternalReply {
    serde_json::from_value(json!({"api_version":1,"request_id":call.request_id,"ok":true,"data":{"tool":"projects.get","data":{"freshness":{"origin":"cached","observed_at":1,"revision":1},"items":[{"project_id":project,"name":"Fixture","php":"php-id","node":"node-id","composer":"composer-id","revision":1}],"next_cursor":null}},"warnings":[]})).unwrap()
}

#[test]
fn modern_metadata_is_per_request_with_discovery_cache_and_version_errors() {
    let mut p = Protocol::new(id("project-a"));
    let discover = reply(p.receive(modern(json!(1), "server/discover", json!({}))));
    assert_eq!(discover["result"]["resultType"], "complete");
    assert_eq!(
        discover["result"]["supportedVersions"],
        json!([MODERN_VERSION, LEGACY_VERSION])
    );
    assert_eq!(discover["result"]["ttlMs"], 0);
    assert_eq!(discover["result"]["cacheScope"], "public");
    let missing =
        reply(p.receive(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})));
    assert_eq!(missing["error"]["code"], -32602);
    let mut wrong = modern(json!(3), "tools/list", json!({}));
    wrong["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("1900-01-01");
    let error = reply(p.receive(wrong));
    assert_eq!(error["error"]["code"], -32022);
    assert_eq!(error["error"]["data"]["requested"], "1900-01-01");
    let mut missing = modern(json!(4), "tools/list", json!({}));
    missing["params"]["_meta"]
        .as_object_mut()
        .unwrap()
        .remove("io.modelcontextprotocol/clientCapabilities");
    assert_eq!(reply(p.receive(missing))["error"]["code"], -32602);
}
#[test]
fn legacy_handshake_does_not_supply_metadata_to_modern_requests() {
    let mut p = Protocol::new(id("project-a"));
    let initialized=reply(p.receive(json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":LEGACY_VERSION,"capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})));
    assert_eq!(initialized["result"]["protocolVersion"], LEGACY_VERSION);
    assert!(initialized["result"].get("resultType").is_none());
    assert_eq!(
        reply(p.receive(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})))["error"]["code"],
        -32602
    );
    assert!(matches!(
        p.receive(json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
        Action::Ignore
    ));
    assert_eq!(
        reply(p.receive(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"})))["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        14
    );
    let mut modern = modern(json!(4), "ping", json!({}));
    modern["params"]["_meta"]
        .as_object_mut()
        .unwrap()
        .remove("io.modelcontextprotocol/clientCapabilities");
    assert_eq!(reply(p.receive(modern))["error"]["code"], -32602);
}
#[test]
fn resources_require_authorized_project_projection_and_reject_foreign_uris_and_cursors() {
    let mut p = Protocol::new(id("project-a"));
    let denied = call(p.receive(modern(json!(1), "resources/list", json!({}))));
    let denied = Protocol::complete(denied, Err(ErrorCode::GrantRevoked));
    assert_eq!(denied["error"]["data"]["code"], "grant_revoked");
    assert!(!denied.to_string().contains("devforge://"));
    let list = call(p.receive(modern(json!(2), "resources/list", json!({}))));
    let data = fixture(&list, "project-a");
    let list = Protocol::complete(list, Ok(data));
    assert_eq!(
        list["result"]["resources"][0]["uri"],
        "devforge://project/project-a/summary"
    );
    assert_eq!(list["result"]["cacheScope"], "private");
    assert_eq!(list["result"]["ttlMs"], 0);
    let foreign = call(p.receive(modern(json!(3), "resources/list", json!({}))));
    let data = fixture(&foreign, "project-b");
    assert_eq!(
        Protocol::complete(foreign, Ok(data))["error"]["data"]["code"],
        "project_mismatch"
    );
    assert_eq!(
        reply(p.receive(modern(
            json!(4),
            "resources/read",
            json!({"uri":"devforge://project/project-b/summary"})
        )))["error"]["code"],
        -32602
    );
    assert_eq!(
        reply(p.receive(modern(
            json!(5),
            "resources/list",
            json!({"cursor":"foreign"})
        )))["error"]["code"],
        -32602
    );
    let read = call(p.receive(modern(
        json!(6),
        "resources/read",
        json!({"uri":"devforge://project/project-a/summary"}),
    )));
    let data = fixture(&read, "project-a");
    let read = Protocol::complete(read, Ok(data));
    assert!(
        read["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("composer-id")
    );
}
#[test]
fn only_fourteen_read_tools_are_dispatchable_and_notifications_never_mint_authority() {
    let mut p = Protocol::new(id("project-a"));
    let listed = reply(p.receive(modern(json!(1), "tools/list", json!({}))));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 14);
    assert!(
        tools
            .iter()
            .all(|tool| tool["annotations"]["readOnlyHint"] == true)
    );
    assert!(!listed.to_string().contains("operations.cancel"));
    for arguments in [
        json!({"name":"operations.cancel","arguments":{"operation_id":"op"}}),
        json!({"name":"projects.get","arguments":{"project_id":"foreign"}}),
        json!({"name":"projects.get","arguments":{},"role":"desktop"}),
    ] {
        assert_eq!(
            reply(p.receive(modern(json!(2), "tools/call", arguments)))["error"]["code"],
            -32602
        );
    }
    assert!(
        matches!(p.receive(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"read-1"}})),Action::Cancel(id) if id=="read-1")
    );
    assert_eq!(reply(p.receive(json!([])))["error"]["code"], -32600);
    assert_eq!(
        reply(p.receive(modern(Value::Null, "ping", json!({}))))["error"]["code"],
        -32600
    );
    assert_eq!(
        reply(p.receive(modern(json!(""), "ping", json!({}))))["result"]["resultType"],
        "complete"
    );
}

#[test]
fn support_export_requires_opt_in_and_exact_review_shape() {
    use devforge_manager::mcp_contract::ExecutionInput;
    let params = json!({"name":"support.export","arguments":{"action":"plan","artifact_id":"owned","expected_revision":1,"idempotency_key":"key"}});
    let mut disabled = Protocol::new(id("project-a"));
    assert_eq!(
        reply(disabled.receive(modern(json!(1), "tools/call", params.clone())))["error"]["code"],
        -32602
    );
    let mut enabled = Protocol::new(id("project-a")).with_support_export_actions(true);
    assert!(
        matches!(enabled.receive(modern(json!(2), "tools/call", params)), Action::Execution(call) if matches!(call.input, ExecutionInput::SupportPlan(_)))
    );
    let apply = json!({"name":"support.export","arguments":{"action":"apply","artifact_id":"owned","expected_revision":1,"idempotency_key":"key","review":"a".repeat(64)}});
    assert!(
        matches!(enabled.receive(modern(json!(3), "tools/call", apply)), Action::Execution(call) if matches!(call.input, ExecutionInput::SupportApply { .. }))
    );
    for args in [
        json!({"name":"support.export","arguments":{"action":"apply","artifact_id":"owned","expected_revision":1,"idempotency_key":"key"}}),
        json!({"name":"support.export","arguments":{"action":"plan","artifact_id":"owned","expected_revision":1,"idempotency_key":"key","path":"/tmp/test"}}),
    ] {
        assert_eq!(
            reply(enabled.receive(modern(json!(4), "tools/call", args)))["error"]["code"],
            -32602
        );
    }
}

#[test]
fn support_preview_and_read_are_scoped_and_reject_paths() {
    use devforge_manager::mcp_contract::ActivityInput;
    let mut p = Protocol::new(id("project-a"));
    let listing = reply(p.receive(modern(json!(1), "tools/list", json!({}))));
    let tools = listing["result"]["tools"].as_array().unwrap();
    for name in ["support.preview", "support.read"] {
        assert!(
            tools
                .iter()
                .any(|tool| tool["name"] == name && tool["annotations"]["readOnlyHint"] == true)
        );
    }
    let preview = p.receive(modern(
        json!(2),
        "tools/call",
        json!({"name":"support.preview","arguments":{}}),
    ));
    assert!(
        matches!(preview, Action::Activity(call) if matches!(call.input, ActivityInput::SupportPreview(_)))
    );
    let read = p.receive(modern(
        json!(3),
        "tools/call",
        json!({"name":"support.read","arguments":{"artifact_id":"owned"}}),
    ));
    assert!(
        matches!(read, Action::Activity(call) if matches!(call.input, ActivityInput::SupportRead(_)))
    );
    for args in [
        json!({"name":"support.preview","arguments":{"project_id":"other"}}),
        json!({"name":"support.read","arguments":{"artifact_id":"../private"}}),
        json!({"name":"support.read","arguments":{}}),
        json!({"name":"support.read","arguments":{"artifact_id":"owned","path":"/tmp/out"}}),
    ] {
        assert_eq!(
            reply(p.receive(modern(json!(4), "tools/call", args)))["error"]["code"],
            -32602
        );
    }
}

#[test]
fn activity_mark_wait_contract_is_bounded_and_rejects_ambiguous_input() {
    use devforge_manager::mcp_contract::ActivityInput;
    let mut p = Protocol::new(id("project-a"));
    let listed = reply(p.receive(modern(json!(1), "tools/list", json!({}))));
    let tools = listed["result"]["tools"].as_array().unwrap();
    for name in ["activity.mark", "activity.wait"] {
        assert!(
            tools
                .iter()
                .any(|tool| tool["name"] == name && tool["annotations"]["readOnlyHint"] == true)
        );
    }
    let call = p.receive(modern(
        json!(2),
        "tools/call",
        json!({"name":"activity.mark", "arguments":{"stream":"logs","detail":false}}),
    ));
    assert!(matches!(call, Action::Activity(call) if matches!(call.input, ActivityInput::Mark(_))));
    let call = p.receive(modern(json!(3), "tools/call", json!({"name":"activity.wait", "arguments":{"bookmark":"owned","timeout_ms":3000,"context_id":"c".repeat(32)}})));
    assert!(matches!(call, Action::Activity(call) if matches!(call.input, ActivityInput::Wait(_))));
    for args in [
        json!({"name":"activity.mark","arguments":{"stream":"all","detail":false}}),
        json!({"name":"activity.mark","arguments":{"stream":"inspector","detail":false,"context_id":null}}),
        json!({"name":"activity.mark","arguments":{"stream":"logs","detail":true,"project_id":"foreign"}}),
        json!({"name":"activity.wait","arguments":{"bookmark":"owned","timeout_ms":0}}),
        json!({"name":"activity.wait","arguments":{"bookmark":"owned","timeout_ms":3001}}),
        json!({"name":"activity.wait","arguments":{"bookmark":"owned","timeout_ms":100,"context_id":null}}),
        json!({"name":"activity.wait","arguments":{"bookmark":"owned","timeout_ms":100,"context_id":"not-32-hex"}}),
    ] {
        let response = reply(p.receive(modern(json!(4), "tools/call", args)));
        assert_eq!(response["error"]["code"], -32602);
    }
}

#[test]
fn mail_show_and_activity_filters_are_scoped_and_unambiguous() {
    use devforge_control::tools::{ActivityStream, ToolInput};
    use devforge_manager::mcp_contract::ActivityInput;
    let mut protocol = Protocol::new(id("project-a"));
    let show = protocol.receive(modern(
        json!(1),
        "tools/call",
        json!({"name":"mail.show","arguments":{"mail_id":"f".repeat(32)}}),
    ));
    assert!(
        matches!(show,Action::Diagnostics(call) if matches!(call.request.input,ToolInput::Mail(ref input) if input.detail==Some(true) && input.mail_id.is_some()))
    );
    let show_budget=protocol.receive(modern(json!(5),"tools/call",json!({"name":"mail.show","arguments":{"mail_id":"f".repeat(32),"page":{"limit":1,"max_bytes":131072,"timeout_ms":5000}}})));
    assert!(
        matches!(show_budget,Action::Diagnostics(call) if matches!(call.request.input,ToolInput::Mail(ref input) if input.detail==Some(true) && input.page.is_some()))
    );
    let list=protocol.receive(modern(json!(2),"tools/call",json!({"name":"mail.list","arguments":{"subject":"receipt","sender":"sender","recipient":"recipient","since":123}})));
    assert!(
        matches!(list,Action::Diagnostics(call) if matches!(call.request.input,ToolInput::Mail(ref input) if input.subject.is_some() && input.sender.is_some() && input.recipient.is_some() && input.since.is_some()))
    );
    let mark=protocol.receive(modern(json!(3),"tools/call",json!({"name":"activity.mark","arguments":{"stream":"mail","detail":true,"subject":"receipt","since":123}})));
    assert!(
        matches!(mark,Action::Activity(call) if matches!(call.input,ActivityInput::Mark(ref req) if req.input.stream==ActivityStream::Mail && req.input.subject.is_some()))
    );
    for args in [
        json!({"name":"mail.show","arguments":{}}),
        json!({"name":"mail.show","arguments":{"mail_id":"f".repeat(32),"detail":false}}),
        json!({"name":"mail.show","arguments":{"mail_id":"f".repeat(32),"page":null}}),
        json!({"name":"mail.list","arguments":{"sender":null}}),
        json!({"name":"activity.mark","arguments":{"stream":"mail","detail":false,"context_id":"c".repeat(32)}}),
        json!({"name":"activity.mark","arguments":{"stream":"logs","detail":false,"subject":"private"}}),
    ] {
        let value = reply(protocol.receive(modern(json!(4), "tools/call", args)));
        assert_eq!(value["error"]["code"], -32602, "{value}");
        assert!(!value.to_string().contains("private"));
    }
}

#[test]
fn site_probe_surface_is_opt_in_and_rejects_host_or_query_arguments() {
    use devforge_manager::mcp_contract::ActivityInput;
    let args = json!({"name":"site.probe","arguments":{"path":"/status"}});
    let mut disabled = Protocol::new(id("project-a"));
    assert_eq!(
        reply(disabled.receive(modern(json!(1), "tools/call", args.clone())))["error"]["code"],
        -32602
    );
    let mut enabled = Protocol::new(id("project-a")).with_site_probe_actions(true);
    let listed = reply(enabled.receive(modern(json!(2), "tools/list", json!({}))));
    let tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "site.probe")
        .unwrap();
    assert_eq!(tool["annotations"]["readOnlyHint"], false);
    assert!(
        matches!(enabled.receive(modern(json!(3), "tools/call", args)), Action::Activity(call) if matches!(call.input, ActivityInput::SiteProbe(_)))
    );
    for arguments in [
        json!({"path":"/", "host":"evil.test"}),
        json!({"path":"/", "url":"http://evil.test"}),
        json!({"path":"/", "context_id":"forged"}),
        json!({"path":"/", "query":"secret"}),
        json!({"path":"/?secret=value"}),
        json!({"path":"https://evil.test/"}),
        json!({"path":"//evil.test/"}),
        json!({"path":"/#fragment"}),
    ] {
        let response = reply(enabled.receive(modern(
            json!(4),
            "tools/call",
            json!({"name":"site.probe","arguments":arguments}),
        )));
        assert_eq!(response["error"]["code"], -32602);
    }
}

#[test]
fn execution_output_preserves_scoped_test_receipt_metadata_in_mcp() {
    use devforge_manager::mcp_contract::ExecutionInput;
    use devforge_workspace::execution_control::{ExecutionOutput, TestArtifactMetadata};
    let mut protocol = Protocol::new(id("project-a")).with_execution_actions(true);
    let call = match protocol.receive(modern(
        json!(99),
        "tools/call",
        json!({"name":"execution.output","arguments":{"operation_id":"owned-operation"}}),
    )) {
        Action::Execution(call) => call,
        _ => panic!("expected execution output call"),
    };
    assert!(matches!(call.input, ExecutionInput::Output { .. }));
    let output = ExecutionOutput {
        success: true,
        exit_code: Some(0),
        logs: vec![],
        dropped_bytes: 0,
        truncated: false,
        test_artifact: Some(TestArtifactMetadata {
            kind: "isolated_test_receipt".into(),
            state: "completed".into(),
            success: true,
            exit_code: Some(0),
        }),
    };
    let response = devforge_control::Response::success(call.request_id.clone(), None, output);
    let response = Protocol::complete_execution(call, Ok(serde_json::to_value(response).unwrap()));
    assert_eq!(response["result"]["isError"], false);
    let artifact = &response["result"]["structuredContent"]["data"]["test_artifact"];
    assert_eq!(
        artifact,
        &json!({"kind":"isolated_test_receipt","state":"completed","success":true,"exit_code":0})
    );
    assert!(!artifact.to_string().contains("database.sqlite"));
    assert!(!artifact.to_string().contains("test-job-"));
}

#[test]
fn debugger_surface_is_opt_in_and_only_accepts_finite_actions() {
    use devforge_manager::mcp_contract::{ActivityInput, ExecutionInput};
    let mut disabled = Protocol::new(id("project-a"));
    let inspect = json!({"name":"debugger.inspect","arguments":{"detail":false,"locals":false}});
    assert_eq!(
        reply(disabled.receive(modern(json!(1), "tools/call", inspect.clone())))["error"]["code"],
        -32602
    );
    let mut enabled = Protocol::new(id("project-a")).with_debugger_actions(true);
    assert!(
        matches!(enabled.receive(modern(json!(2),"tools/call",inspect)),Action::Activity(call) if matches!(call.input,ActivityInput::DebuggerInspect(_)))
    );
    let profile = json!({"name":"debugger.profile","arguments":{"profile_id":"a".repeat(32)}});
    assert!(
        matches!(enabled.receive(modern(json!(3),"tools/call",profile)),Action::Activity(call) if matches!(call.input,ActivityInput::DebuggerProfile(_)))
    );
    let plan = json!({"name":"debugger.control","arguments":{"action":"plan","expected_revision":1,"idempotency_key":"one","input":{"command":"step_over","session_id":"fixture-session","session_revision":4}}});
    assert!(
        matches!(enabled.receive(modern(json!(4),"tools/call",plan)),Action::Execution(call) if matches!(call.input,ExecutionInput::DebuggerPlan(_)))
    );
    let install = json!({"name":"debugger.install","arguments":{"action":"plan","expected_revision":1,"idempotency_key":"install","input":{}}});
    assert!(
        matches!(enabled.receive(modern(json!(4),"tools/call",install)),Action::Execution(call) if matches!(call.input,ExecutionInput::DebuggerPlan(_)))
    );
    let set = json!({"name":"debugger.primitive_assignment","arguments":{"action":"apply","expected_revision":1,"idempotency_key":"set","review":"a".repeat(64),"input":{"name":"$value","kind":"int","value":"7","session_id":"fixture-session","session_revision":4}}});
    assert!(
        matches!(enabled.receive(modern(json!(4),"tools/call",set)),Action::Execution(call) if matches!(call.input,ExecutionInput::DebuggerApply{..}))
    );
    for invalid in [
        json!({"name":"debugger.control","arguments":{"action":"plan","expected_revision":1,"idempotency_key":"one","input":{"command":"run","session_revision":4}}}),
        json!({"name":"debugger.primitive_assignment","arguments":{"action":"plan","expected_revision":1,"idempotency_key":"set","input":{"name":"$value","kind":"int","value":"7","session_revision":4}}}),
        json!({"name":"debugger.control","arguments":{"action":"plan","expected_revision":1,"idempotency_key":"one","input":{"command":"eval","session_id":"fixture-session","session_revision":4}}}),
        json!({"name":"debugger.control","arguments":{"action":"plan","expected_revision":1,"idempotency_key":"one","input":{"command":"set","session_id":"fixture-session","session_revision":4,"value":"malicious"}}}),
        json!({"name":"debugger.profile","arguments":{"profile_id":"../foreign"}}),
    ] {
        assert_eq!(
            reply(enabled.receive(modern(json!(5), "tools/call", invalid)))["error"]["code"],
            -32602
        );
    }
}

#[test]
fn workspace_surface_is_opt_in_scoped_and_requires_review_shape() {
    use devforge_manager::mcp_contract::ExecutionInput;
    let mut disabled = Protocol::new(id("project-a"));
    let list = json!({"name":"workspaces.list","arguments":{}});
    assert_eq!(
        reply(disabled.receive(modern(json!(1), "tools/call", list.clone())))["error"]["code"],
        -32602
    );
    let mut enabled = Protocol::new(id("project-a")).with_workspace_actions(true);
    let definitions = reply(enabled.receive(modern(json!(10), "tools/list", json!({}))));
    let tools = definitions["result"]["tools"].as_array().unwrap();
    for name in ["workspaces.restore", "workspaces.purge"] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert!(
            tool["description"]
                .as_str()
                .unwrap()
                .contains("unavailable")
        );
    }
    assert!(
        matches!(enabled.receive(modern(json!(2),"tools/call",list)),Action::Execution(call) if matches!(call.input,ExecutionInput::WorkspaceList(_)))
    );
    let prepare = json!({"name":"workspaces.prepare","arguments":{"action":"create","name":"demo","branch":"feature/demo","base":"main","new_branch":true,"database":"empty","dependencies":false,"copy_environment":false}});
    assert!(
        matches!(enabled.receive(modern(json!(3),"tools/call",prepare)),Action::Execution(call) if matches!(call.input,ExecutionInput::WorkspacePrepare(_)))
    );
    let plan = json!({"name":"workspaces.create","arguments":{"action":"plan","expected_revision":1,"idempotency_key":"fixture","input":{"plan":"a".repeat(64)}}});
    assert!(
        matches!(enabled.receive(modern(json!(4),"tools/call",plan)),Action::Execution(call) if matches!(call.input,ExecutionInput::WorkspacePlan(_)))
    );
    for bad in [
        json!({"name":"workspaces.prepare","arguments":{"action":"create","path":"/tmp/foreign"}}),
        json!({"name":"workspaces.list","arguments":{"project_id":"other"}}),
        json!({"name":"workspaces.purge","arguments":{"action":"apply","expected_revision":1,"idempotency_key":"k","input":{"workspace_id":"own","plan":"a".repeat(64)}}}),
    ] {
        assert_eq!(
            reply(enabled.receive(modern(json!(5), "tools/call", bad)))["error"]["code"],
            -32602
        );
    }
}
