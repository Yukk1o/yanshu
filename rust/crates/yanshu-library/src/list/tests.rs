use num_bigint::BigInt;

use crate::{LibraryKey, LibraryRegistry, LibraryValue};

fn int(value: i32) -> LibraryValue {
    LibraryValue::Int(BigInt::from(value))
}

fn guest_error_code(value: &LibraryValue) -> &str {
    let LibraryValue::Err(error) = value else {
        panic!("expected Err");
    };
    let LibraryValue::Map(fields) = error.as_ref() else {
        panic!("expected error map");
    };
    let Some(LibraryValue::String(code)) = fields.get(&LibraryKey::String("code".to_owned()))
    else {
        panic!("expected error code");
    };
    code
}

#[test]
fn structural_operations_preserve_order_and_values() {
    let mut registry = LibraryRegistry::rust_standard();
    let values = LibraryValue::List(vec![int(1), int(2), int(3)]);

    let reversed = registry
        .invoke("list", 1, "reverse", std::slice::from_ref(&values))
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(
        reversed.value,
        LibraryValue::List(vec![int(3), int(2), int(1)])
    );

    let appended = registry
        .invoke(
            "list",
            1,
            "append",
            &[values.clone(), LibraryValue::List(vec![int(4)])],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(
        appended.value,
        LibraryValue::List(vec![int(1), int(2), int(3), int(4)])
    );

    let contains = registry
        .invoke("list", 1, "contains?", &[values, int(2)])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(contains.value, LibraryValue::Bool(true));
}

#[test]
fn indexed_operations_return_recoverable_results() {
    let mut registry = LibraryRegistry::rust_standard();
    let values = LibraryValue::List(vec![int(1), int(2), int(3), int(4)]);

    let get = registry
        .invoke("list", 1, "get", &[values.clone(), int(1)])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(get.value, LibraryValue::Ok(Box::new(int(2))));

    let take = registry
        .invoke("list", 1, "take", &[values.clone(), int(2)])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(
        take.value,
        LibraryValue::Ok(Box::new(LibraryValue::List(vec![int(1), int(2)])))
    );

    let drop = registry
        .invoke("list", 1, "drop", &[values.clone(), int(2)])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(
        drop.value,
        LibraryValue::Ok(Box::new(LibraryValue::List(vec![int(3), int(4)])))
    );

    let slice = registry
        .invoke("list", 1, "slice", &[values.clone(), int(1), int(3)])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(
        slice.value,
        LibraryValue::Ok(Box::new(LibraryValue::List(vec![int(2), int(3)])))
    );

    for (operation, arguments, code) in [
        (
            "get",
            vec![values.clone(), int(-1)],
            "LIST_INDEX_OUT_OF_BOUNDS",
        ),
        (
            "take",
            vec![values.clone(), int(5)],
            "LIST_COUNT_OUT_OF_BOUNDS",
        ),
        (
            "drop",
            vec![values.clone(), int(-1)],
            "LIST_COUNT_OUT_OF_BOUNDS",
        ),
        (
            "slice",
            vec![values.clone(), int(3), int(2)],
            "LIST_RANGE_OUT_OF_BOUNDS",
        ),
    ] {
        let result = registry
            .invoke("list", 1, operation, &arguments)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(guest_error_code(&result.value), code);
    }
}

#[test]
fn fuel_tracks_structure_and_amplification_is_rejected_before_backend() {
    let registry = LibraryRegistry::rust_standard();
    let small = registry
        .call_fuel(
            "list",
            1,
            "reverse",
            &[LibraryValue::List(vec![LibraryValue::String(
                "x".to_owned(),
            )])],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let large = registry
        .call_fuel(
            "list",
            1,
            "reverse",
            &[LibraryValue::List(vec![LibraryValue::String(
                "x".repeat(4096),
            )])],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert!(large > small);

    let mut registry = LibraryRegistry::rust_standard();
    let left = LibraryValue::List(vec![LibraryValue::Nil; 5_000]);
    let right = LibraryValue::List(vec![LibraryValue::Nil; 5_000]);
    let diagnostic = match registry.invoke("list", 1, "append", &[left, right]) {
        Err(diagnostic) => diagnostic,
        Ok(_) => panic!("combined result must exceed the 10,000 node envelope"),
    };
    assert_eq!(diagnostic.code, "RUNTIME_LIBRARY_RESULT_LIMIT");
}
