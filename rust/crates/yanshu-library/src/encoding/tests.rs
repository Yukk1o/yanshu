#![forbid(unsafe_code)]

use crate::{
    LibraryBackend, LibraryKey, LibraryRegistry, LibraryValue, MAXIMUM_ENCODING_INPUT_BYTES,
    RustEncodingBackend, trusted_contract,
};

fn invoke(operation: &str, input: String) -> LibraryValue {
    RustEncodingBackend
        .invoke(operation, &[LibraryValue::String(input)])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
}

fn ok_text(value: LibraryValue) -> String {
    let LibraryValue::Ok(value) = value else {
        panic!("expected Ok");
    };
    let LibraryValue::String(value) = *value else {
        panic!("expected String");
    };
    value
}

fn error(value: LibraryValue) -> (String, Option<usize>) {
    let LibraryValue::Err(value) = value else {
        panic!("expected Err");
    };
    let LibraryValue::Map(fields) = *value else {
        panic!("expected error Map");
    };
    let code = match fields.get(&LibraryKey::String("code".to_owned())) {
        Some(LibraryValue::String(value)) => value.clone(),
        _ => panic!("expected error code"),
    };
    let offset = match fields.get(&LibraryKey::String("offset".to_owned())) {
        Some(LibraryValue::Int(value)) => usize::try_from(value).ok(),
        None => None,
        _ => panic!("expected integer offset"),
    };
    (code, offset)
}

#[test]
fn base64_uses_rfc_4648_standard_alphabet_and_required_padding() {
    for (plain, encoded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
        ("衍术🦀", "6KGN5pyv8J+mgA=="),
    ] {
        assert_eq!(
            ok_text(invoke("base64-encode-text", plain.to_owned())),
            encoded
        );
        assert_eq!(
            ok_text(invoke("base64-decode-text", encoded.to_owned())),
            plain
        );
    }
}

#[test]
fn base64_rejects_noncanonical_and_non_utf8_inputs() {
    for (input, offset) in [
        ("Zg", 2),
        ("Zg=", 3),
        ("Zg===", 5),
        ("=m9v", 0),
        ("Zm=v", 2),
        ("Zm9v=AAA", 4),
        ("Zh==", 2),
        ("Zm9=", 3),
        ("Zm_8", 2),
    ] {
        assert_eq!(
            error(invoke("base64-decode-text", input.to_owned())),
            ("ENCODING_INVALID_BASE64".to_owned(), Some(offset))
        );
    }

    assert_eq!(
        error(invoke("base64-decode-text", "/w==".to_owned())),
        ("ENCODING_INVALID_UTF8".to_owned(), Some(0))
    );
}

#[test]
fn hex_is_lowercase_on_output_and_accepts_both_input_cases() {
    assert_eq!(
        ok_text(invoke("hex-encode-text", "衍术🦀".to_owned())),
        "e8a18de69caff09fa680"
    );
    assert_eq!(
        ok_text(invoke("hex-decode-text", "E8A18DE69CAFF09FA680".to_owned())),
        "衍术🦀"
    );
}

#[test]
fn hex_rejects_invalid_shape_digits_and_utf8() {
    for (input, offset) in [("0", 1), ("0g", 1), ("xx", 0)] {
        assert_eq!(
            error(invoke("hex-decode-text", input.to_owned())),
            ("ENCODING_INVALID_HEX".to_owned(), Some(offset))
        );
    }
    assert_eq!(
        error(invoke("hex-decode-text", "ff".to_owned())),
        ("ENCODING_INVALID_UTF8".to_owned(), Some(0))
    );
}

#[test]
fn encoding_checks_input_and_amplified_output_before_allocation() {
    assert_eq!(
        error(invoke(
            "base64-encode-text",
            "x".repeat(MAXIMUM_ENCODING_INPUT_BYTES)
        ))
        .0,
        "ENCODING_OUTPUT_LIMIT"
    );
    assert_eq!(
        error(invoke(
            "hex-encode-text",
            "x".repeat(MAXIMUM_ENCODING_INPUT_BYTES / 2 + 1)
        ))
        .0,
        "ENCODING_OUTPUT_LIMIT"
    );
    assert_eq!(
        error(invoke(
            "base64-decode-text",
            "A".repeat(MAXIMUM_ENCODING_INPUT_BYTES + 4)
        ))
        .0,
        "ENCODING_INPUT_LIMIT"
    );
}

#[test]
fn encoding_fuel_tracks_input_and_predicted_output_even_for_invalid_data() {
    let registry = LibraryRegistry::rust_standard();
    let small = registry
        .call_fuel(
            "encoding",
            1,
            "base64-encode-text",
            &[LibraryValue::String("x".to_owned())],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let large = registry
        .call_fuel(
            "encoding",
            1,
            "base64-encode-text",
            &[LibraryValue::String("x".repeat(1024))],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let invalid = registry
        .call_fuel(
            "encoding",
            1,
            "base64-decode-text",
            &[LibraryValue::String("!".repeat(1024))],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert!(large > small);
    assert!(invalid > small);
    assert!(trusted_contract("encoding", 1).is_some());
    assert!(trusted_contract("encoding", 2).is_none());
}
