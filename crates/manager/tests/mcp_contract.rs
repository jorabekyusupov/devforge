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
        5
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
fn only_five_read_tools_are_dispatchable_and_notifications_never_mint_authority() {
    let mut p = Protocol::new(id("project-a"));
    let listed = reply(p.receive(modern(json!(1), "tools/list", json!({}))));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
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
