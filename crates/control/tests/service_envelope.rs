use devforge_control::{Id, Response};
#[test]
fn successful_missing_lookup_receipt_keeps_present_null_data() {
    let response = Response::success(Id::new("lookup").unwrap(), None, None::<Id>);
    let value = serde_json::to_value(response).unwrap();
    assert!(value.get("data").is_some_and(serde_json::Value::is_null));
    let decoded: Response<Option<Id>> = serde_json::from_value(value.clone()).unwrap();
    assert!(decoded.is_ok());
    assert_eq!(decoded.data(), Some(&None));
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
}
