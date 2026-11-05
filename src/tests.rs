use serde_json::json;
use std::cell::{Cell, RefCell};

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

#[test]
fn copy_tab_name_is_restored_after_success_or_viewer_failure() {
    for fail in [false, true] {
        let label = RefCell::new("work λ".to_owned());
        let renames = RefCell::new(Vec::new());
        let result = crate::with_copy_tab(
            "w1:t1",
            |method, params| {
                assert_eq!(params["tab_id"], "w1:t1");
                match method {
                    "tab.get" => Ok(json!({"tab": {"label": label.borrow().as_str()}})),
                    "tab.rename" => {
                        let name = params["label"].as_str().unwrap().to_owned();
                        renames.borrow_mut().push(name.clone());
                        *label.borrow_mut() = name;
                        Ok(json!({}))
                    }
                    _ => panic!("unexpected request: {method}"),
                }
            },
            || {
                assert_eq!(label.borrow().as_str(), "COPY");
                if fail {
                    Err("viewer failed".into())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result.is_err(), fail);
        assert_eq!(label.borrow().as_str(), "work λ");
        assert_eq!(*renames.borrow(), ["COPY", "work λ"]);
    }
}

#[test]
fn user_tab_rename_is_not_overwritten_when_copy_mode_closes() {
    let label = RefCell::new("work".to_owned());
    let renames = RefCell::new(Vec::new());
    crate::with_copy_tab(
        "w1:t1",
        |method, params| match method {
            "tab.get" => Ok(json!({"tab": {"label": label.borrow().as_str()}})),
            "tab.rename" => {
                let name = params["label"].as_str().unwrap().to_owned();
                renames.borrow_mut().push(name.clone());
                *label.borrow_mut() = name;
                Ok(json!({}))
            }
            _ => panic!("unexpected request: {method}"),
        },
        || {
            *label.borrow_mut() = "renamed by user".into();
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(label.borrow().as_str(), "renamed by user");
    assert_eq!(*renames.borrow(), ["COPY"]);
}

#[test]
fn an_existing_copy_tab_name_is_left_alone() {
    let viewed = Cell::new(false);
    crate::with_copy_tab(
        "w1:t1",
        |method, params| {
            assert_eq!(method, "tab.get");
            assert_eq!(params["tab_id"], "w1:t1");
            Ok(json!({"tab": {"label": "COPY"}}))
        },
        || {
            viewed.set(true);
            Ok(())
        },
    )
    .unwrap();
    assert!(viewed.get());
}

#[test]
fn viewer_and_tab_restoration_errors_are_both_reported() {
    let label = RefCell::new("work".to_owned());
    let result = crate::with_copy_tab(
        "w1:t1",
        |method, params| match method {
            "tab.get" => Ok(json!({"tab": {"label": label.borrow().as_str()}})),
            "tab.rename" if params["label"] == "COPY" => {
                *label.borrow_mut() = "COPY".into();
                Ok(json!({}))
            }
            "tab.rename" => Err("restore failed".into()),
            _ => panic!("unexpected request: {method}"),
        },
        || Err("viewer failed".into()),
    );
    let error = result.unwrap_err().to_string();
    assert!(error.contains("viewer failed"));
    assert!(error.contains("restore failed"));
}

#[test]
fn missing_tab_label_does_not_start_the_viewer() {
    let result = crate::with_copy_tab(
        "w1:t1",
        |method, _| {
            assert_eq!(method, "tab.get");
            Ok(json!({"tab": {}}))
        },
        || panic!("must not run without a restorable tab name"),
    );
    assert!(result.is_err());
}
