use std::collections::BTreeMap;

use num_bigint::BigInt;

use crate::{LibraryKey, LibraryRegistry, LibraryValue};

fn int(value: i32) -> LibraryValue {
    LibraryValue::Int(BigInt::from(value))
}

fn mapping(entries: impl IntoIterator<Item = (LibraryKey, LibraryValue)>) -> LibraryValue {
    LibraryValue::Map(BTreeMap::from_iter(entries))
}

fn string_key(value: &str) -> LibraryKey {
    LibraryKey::String(value.to_owned())
}

fn guest_error_code(value: &LibraryValue) -> &str {
    let LibraryValue::Err(error) = value else {
        panic!("expected Err");
    };
    let LibraryValue::Map(fields) = error.as_ref() else {
        panic!("expected error map");
    };
    let Some(LibraryValue::String(code)) = fields.get(&string_key("code")) else {
        panic!("expected error code");
    };
    code
}

#[test]
fn views_use_stable_key_order_and_preserve_key_kinds() {
    let mut registry = LibraryRegistry::rust_standard();
    let source = mapping([
        (LibraryKey::Symbol("z".to_owned()), int(3)),
        (string_key("b"), int(2)),
        (string_key("a"), int(1)),
    ]);

    let size = registry
        .invoke("map", 1, "size", std::slice::from_ref(&source))
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(size.value, int(3));

    let keys = registry
        .invoke("map", 1, "keys", std::slice::from_ref(&source))
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(
        keys.value,
        LibraryValue::List(vec![
            LibraryValue::String("a".to_owned()),
            LibraryValue::String("b".to_owned()),
            LibraryValue::Symbol("z".to_owned()),
        ])
    );

    let values = registry
        .invoke("map", 1, "values", std::slice::from_ref(&source))
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(
        values.value,
        LibraryValue::List(vec![int(1), int(2), int(3)])
    );

    let entries = registry
        .invoke("map", 1, "entries", &[source])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let LibraryValue::List(entries) = entries.value else {
        panic!("expected entries");
    };
    assert_eq!(
        entries[0],
        LibraryValue::List(vec![LibraryValue::String("a".to_owned()), int(1)])
    );
}

#[test]
fn remove_is_idempotent_and_rejects_non_keys_as_guest_data() {
    let mut registry = LibraryRegistry::rust_standard();
    let source = mapping([(string_key("a"), int(1)), (string_key("b"), int(2))]);

    let removed = registry
        .invoke(
            "map",
            1,
            "remove",
            &[source.clone(), LibraryValue::String("a".to_owned())],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(
        removed.value,
        LibraryValue::Ok(Box::new(mapping([(string_key("b"), int(2))])))
    );

    let absent = registry
        .invoke(
            "map",
            1,
            "remove",
            &[source.clone(), LibraryValue::String("missing".to_owned())],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(absent.value, LibraryValue::Ok(Box::new(source)));

    let invalid = registry
        .invoke("map", 1, "remove", &[mapping([]), int(1)])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(guest_error_code(&invalid.value), "MAP_INVALID_KEY");
}

#[test]
fn merge_policy_is_explicit_and_conflicts_are_recoverable() {
    let mut registry = LibraryRegistry::rust_standard();
    let left = mapping([(string_key("a"), int(1)), (string_key("shared"), int(10))]);
    let right = mapping([(string_key("b"), int(2)), (string_key("shared"), int(20))]);

    let conflict = registry
        .invoke("map", 1, "merge-disjoint", &[left.clone(), right.clone()])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(guest_error_code(&conflict.value), "MAP_KEY_CONFLICT");

    for (operation, shared) in [("merge-left", 10), ("merge-right", 20)] {
        let merged = registry
            .invoke("map", 1, operation, &[left.clone(), right.clone()])
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let LibraryValue::Map(value) = &merged.value else {
            panic!("expected map");
        };
        assert_eq!(value.get(&string_key("shared")), Some(&int(shared)));
        assert_eq!(value.get(&string_key("a")), Some(&int(1)));
        assert_eq!(value.get(&string_key("b")), Some(&int(2)));
    }
}

#[test]
fn contains_value_uses_structural_equality_and_output_amplification_is_prechecked() {
    let mut registry = LibraryRegistry::rust_standard();
    let nested = mapping([(string_key("id"), int(1))]);
    let source = mapping([(string_key("item"), nested.clone())]);
    let contains = registry
        .invoke("map", 1, "contains-value?", &[source, nested])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(contains.value, LibraryValue::Bool(true));

    let source =
        mapping((0..3_334).map(|index| (string_key(&format!("k{index}")), LibraryValue::Nil)));
    let diagnostic = match registry.invoke("map", 1, "entries", &[source]) {
        Err(diagnostic) => diagnostic,
        Ok(_) => panic!("entry projection must exceed the portable node envelope"),
    };
    assert_eq!(diagnostic.code, "RUNTIME_LIBRARY_RESULT_LIMIT");
}
