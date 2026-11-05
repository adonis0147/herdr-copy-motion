#[test]
fn dimensions_reject_invalid_host_values() {
    for value in [
        serde_json::json!(0),
        serde_json::json!(100_000),
        serde_json::Value::Null,
    ] {
        assert!(crate::dimension(&value).is_err());
    }
}
