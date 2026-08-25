#![forbid(unsafe_code)]

mod contract;
mod decimal;
mod digest;
mod json;
mod math;
mod registry;
mod text;
mod value;

use yanshu_diagnostic::YanshuResult;

pub use contract::{
    DECIMAL_V1, DIGEST_V1, FuelModel, JSON_V1, LibraryContract, LibraryType, MATH_V1,
    OperationContract, TEXT_V1, TEXT_V2, is_trusted_operation_name, trusted_contract,
};
pub use decimal::{
    MAXIMUM_DECIMAL_INPUT_BYTES, MAXIMUM_DECIMAL_INTEGER_BITS, MAXIMUM_DECIMAL_OUTPUT_BYTES,
    MAXIMUM_DECIMAL_SCALE, RustDecimalBackend,
};
pub use digest::RustDigestBackend;
pub use json::{
    MAXIMUM_JSON_DEPTH, MAXIMUM_JSON_INPUT_BYTES, MAXIMUM_JSON_INTEGER_BITS, MAXIMUM_JSON_NODES,
    MAXIMUM_JSON_OUTPUT_BYTES, MAXIMUM_JSON_STRING_BYTES, RustJsonBackend,
};
pub use math::{MAXIMUM_MATH_INTEGER_BITS, RustMathBackend};
pub use registry::{BackendDescriptor, LibraryInvocation, LibraryRegistry};
pub use text::{RustTextBackend, RustTextV2Backend};
pub use value::{LibraryKey, LibraryValue};

pub trait LibraryBackend: Send {
    fn descriptor(&self) -> BackendDescriptor;
    fn invoke(&mut self, operation: &str, arguments: &[LibraryValue])
    -> YanshuResult<LibraryValue>;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use num_bigint::BigInt;
    use yanshu_diagnostic::{Diagnostic, YanshuResult};

    use crate::{
        BackendDescriptor, LibraryBackend, LibraryKey, LibraryRegistry, LibraryValue,
        MAXIMUM_DECIMAL_INPUT_BYTES, MAXIMUM_JSON_INPUT_BYTES, RustTextBackend, trusted_contract,
    };

    fn require_error<T>(result: YanshuResult<T>) -> Diagnostic {
        match result {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected a diagnostic"),
        }
    }

    fn guest_error_code(value: &LibraryValue) -> &str {
        let LibraryValue::Err(error) = value else {
            panic!("expected a guest Err value");
        };
        let LibraryValue::Map(fields) = error.as_ref() else {
            panic!("expected a structured guest error");
        };
        let Some(LibraryValue::String(code)) = fields.get(&LibraryKey::String("code".to_owned()))
        else {
            panic!("expected a stable guest error code");
        };
        code
    }

    fn guest_ok(value: &LibraryValue) -> &LibraryValue {
        let LibraryValue::Ok(value) = value else {
            panic!("expected a guest Ok value");
        };
        value
    }

    struct InvalidBackend;

    impl LibraryBackend for InvalidBackend {
        fn descriptor(&self) -> BackendDescriptor {
            BackendDescriptor {
                provider: "invalid".to_owned(),
                library: "text".to_owned(),
                version: 1,
                operations: vec!["length".to_owned(), "extra".to_owned()],
            }
        }

        fn invoke(
            &mut self,
            _operation: &str,
            _arguments: &[LibraryValue],
        ) -> YanshuResult<LibraryValue> {
            Ok(LibraryValue::Nil)
        }
    }

    struct FailingBackend;

    impl LibraryBackend for FailingBackend {
        fn descriptor(&self) -> BackendDescriptor {
            RustTextBackend.descriptor()
        }

        fn invoke(
            &mut self,
            _operation: &str,
            _arguments: &[LibraryValue],
        ) -> YanshuResult<LibraryValue> {
            Err(Diagnostic::simple(
                "SECRET_BACKEND_ERROR",
                "credential=must-not-escape",
            ))
        }
    }

    #[test]
    fn rust_text_backend_uses_unicode_scalars_and_contract_fuel() {
        let mut registry = LibraryRegistry::rust_standard();
        let result = registry
            .invoke(
                "text",
                1,
                "length",
                &[LibraryValue::String("A语🙂".to_owned())],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(result.value, LibraryValue::Int(3.into()));
        assert_eq!(result.fuel, 2);
    }

    #[test]
    fn text_replace_rejects_output_amplification_before_allocation() {
        let mut registry = LibraryRegistry::rust_standard();
        let replacement = "x".repeat(1024 * 1024);
        let diagnostic = match registry.invoke(
            "text",
            1,
            "replace",
            &[
                LibraryValue::String("a".to_owned()),
                LibraryValue::String(String::new()),
                LibraryValue::String(replacement),
            ],
        ) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("replacement expansion must be rejected"),
        };
        assert_eq!(diagnostic.code, "RUNTIME_LIBRARY_RESULT_LIMIT");
    }

    #[test]
    fn text_v2_is_a_unicode_aware_compatible_superset() {
        let mut registry = LibraryRegistry::rust_standard();
        let trim = registry
            .invoke(
                "text",
                2,
                "trim",
                &[LibraryValue::String("  AI语言\n".to_owned())],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(trim.value, LibraryValue::String("AI语言".to_owned()));

        let uppercase = registry
            .invoke(
                "text",
                2,
                "uppercase",
                &[LibraryValue::String("straße".to_owned())],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(uppercase.value, LibraryValue::String("STRASSE".to_owned()));

        let split = registry
            .invoke(
                "text",
                2,
                "split",
                &[
                    LibraryValue::String("a,,b,".to_owned()),
                    LibraryValue::String(",".to_owned()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            split.value,
            LibraryValue::List(
                ["a", "", "b", ""]
                    .into_iter()
                    .map(|value| LibraryValue::String(value.to_owned()))
                    .collect()
            )
        );

        let joined = registry
            .invoke(
                "text",
                2,
                "join",
                &[
                    LibraryValue::List(vec![
                        LibraryValue::String("AI".to_owned()),
                        LibraryValue::String("语言".to_owned()),
                    ]),
                    LibraryValue::String("·".to_owned()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(joined.value, LibraryValue::String("AI·语言".to_owned()));

        let substring = registry
            .invoke(
                "text",
                2,
                "substring",
                &[
                    LibraryValue::String("A语言🦀Z".to_owned()),
                    LibraryValue::Int(1.into()),
                    LibraryValue::Int(4.into()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(substring.value, LibraryValue::String("语言🦀".to_owned()));

        assert!(
            trusted_contract("text", 1)
                .and_then(|contract| contract.operation("trim"))
                .is_none()
        );
    }

    #[test]
    fn text_v2_rejects_invalid_shapes_and_amplification_before_allocation() {
        let mut registry = LibraryRegistry::rust_standard();
        let empty_separator = require_error(registry.invoke(
            "text",
            2,
            "split",
            &[
                LibraryValue::String("value".to_owned()),
                LibraryValue::String(String::new()),
            ],
        ));
        assert_eq!(empty_separator.code, "RUNTIME_LIBRARY_ARGUMENT");

        let invalid_range = require_error(registry.invoke(
            "text",
            2,
            "substring",
            &[
                LibraryValue::String("语言".to_owned()),
                LibraryValue::Int(0.into()),
                LibraryValue::Int(3.into()),
            ],
        ));
        assert_eq!(invalid_range.code, "RUNTIME_LIBRARY_ARGUMENT");

        let invalid_list = require_error(registry.invoke(
            "text",
            2,
            "join",
            &[
                LibraryValue::List(vec![LibraryValue::Int(1.into())]),
                LibraryValue::String(",".to_owned()),
            ],
        ));
        assert_eq!(invalid_list.code, "RUNTIME_TYPE");

        let too_many_segments = require_error(registry.invoke(
            "text",
            2,
            "split",
            &[
                LibraryValue::String("a,".repeat(10_000)),
                LibraryValue::String(",".to_owned()),
            ],
        ));
        assert_eq!(too_many_segments.code, "RUNTIME_LIBRARY_RESULT_LIMIT");

        let oversized_join = require_error(registry.invoke(
            "text",
            2,
            "join",
            &[
                LibraryValue::List(vec![
                    LibraryValue::String(String::new()),
                    LibraryValue::String(String::new()),
                ]),
                LibraryValue::String("x".repeat(1024 * 1024 + 1)),
            ],
        ));
        assert_eq!(oversized_join.code, "RUNTIME_LIBRARY_RESULT_LIMIT");
    }

    #[test]
    fn math_v1_has_stable_integer_semantics_and_magnitude_fuel() {
        let mut registry = LibraryRegistry::rust_standard();
        let abs = registry
            .invoke("math", 1, "abs", &[LibraryValue::Int(BigInt::from(-42_i8))])
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(abs.value, LibraryValue::Int(BigInt::from(42_u8)));

        let sign = registry
            .invoke("math", 1, "sign", &[LibraryValue::Int(BigInt::ZERO)])
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(sign.value, LibraryValue::Int(BigInt::ZERO));

        let clamp = registry
            .invoke(
                "math",
                1,
                "clamp",
                &[
                    LibraryValue::Int(BigInt::from(-42_i8)),
                    LibraryValue::Int(BigInt::from(-10_i8)),
                    LibraryValue::Int(BigInt::from(10_u8)),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(clamp.value, LibraryValue::Int(BigInt::from(-10_i8)));

        let gcd = registry
            .invoke(
                "math",
                1,
                "gcd",
                &[
                    LibraryValue::Int(BigInt::from(-42_i8)),
                    LibraryValue::Int(BigInt::from(30_u8)),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(gcd.value, LibraryValue::Int(BigInt::from(6_u8)));

        let zero_gcd = registry
            .invoke(
                "math",
                1,
                "gcd",
                &[
                    LibraryValue::Int(BigInt::ZERO),
                    LibraryValue::Int(BigInt::ZERO),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(zero_gcd.value, LibraryValue::Int(BigInt::ZERO));

        let small_fuel = registry
            .call_fuel(
                "math",
                1,
                "gcd",
                &[
                    LibraryValue::Int(BigInt::from(12_u8)),
                    LibraryValue::Int(BigInt::from(18_u8)),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let large = BigInt::from(1_u8) << 4095_usize;
        let large_fuel = registry
            .call_fuel(
                "math",
                1,
                "gcd",
                &[LibraryValue::Int(large.clone()), LibraryValue::Int(large)],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert!(large_fuel > small_fuel);
    }

    #[test]
    fn math_v1_rejects_invalid_bounds_and_oversized_inputs_before_backend_work() {
        let mut registry = LibraryRegistry::rust_standard();
        let invalid_bounds = require_error(registry.invoke(
            "math",
            1,
            "clamp",
            &[
                LibraryValue::Int(BigInt::ZERO),
                LibraryValue::Int(BigInt::from(2_u8)),
                LibraryValue::Int(BigInt::from(1_u8)),
            ],
        ));
        assert_eq!(invalid_bounds.code, "RUNTIME_LIBRARY_ARGUMENT");

        let oversized = BigInt::from(1_u8) << 65_536_usize;
        let oversized_input =
            require_error(registry.invoke("math", 1, "abs", &[LibraryValue::Int(oversized)]));
        assert_eq!(oversized_input.code, "RUNTIME_LIBRARY_ARGUMENT");

        assert!(
            trusted_contract("math", 1)
                .and_then(|contract| contract.operation("gcd"))
                .is_some()
        );
        assert!(trusted_contract("math", 2).is_none());
    }

    #[test]
    fn digest_v1_has_standard_utf8_vectors_and_byte_fuel() {
        let mut registry = LibraryRegistry::rust_standard();
        let sha256 = registry
            .invoke(
                "digest",
                1,
                "sha256-text",
                &[LibraryValue::String("abc".to_owned())],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            sha256.value,
            LibraryValue::String(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()
            )
        );

        let sha512 = registry
            .invoke(
                "digest",
                1,
                "sha512-text",
                &[LibraryValue::String("abc".to_owned())],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            sha512.value,
            LibraryValue::String(
                concat!(
                    "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a",
                    "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
                )
                .to_owned()
            )
        );

        let ascii_fuel = registry
            .call_fuel(
                "digest",
                1,
                "sha256-text",
                &[LibraryValue::String("x".repeat(64))],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let unicode_fuel = registry
            .call_fuel(
                "digest",
                1,
                "sha256-text",
                &[LibraryValue::String("语".repeat(22))],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(ascii_fuel, 2);
        assert_eq!(unicode_fuel, 3);
        assert!(trusted_contract("digest", 1).is_some());
        assert!(trusted_contract("digest", 2).is_none());
    }

    #[test]
    fn json_v1_parses_integer_only_data_and_writes_canonical_text() {
        let mut registry = LibraryRegistry::rust_standard();
        let input = r#"{"z":2,"a":[true,null,"\u0041\uD83E\uDD80"],"negative":-42}"#;
        let parsed = registry
            .invoke(
                "json",
                1,
                "parse",
                &[LibraryValue::String(input.to_owned())],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let LibraryValue::Ok(value) = parsed.value else {
            panic!("valid JSON must parse");
        };
        let encoded = registry
            .invoke("json", 1, "stringify-canonical", &[(*value).clone()])
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            encoded.value,
            LibraryValue::Ok(Box::new(LibraryValue::String(
                r#"{"a":[true,null,"A🦀"],"negative":-42,"z":2}"#.to_owned()
            )))
        );
        assert_eq!(parsed.fuel, 2);
        assert!(encoded.fuel > 1);
        assert!(trusted_contract("json", 1).is_some());
        assert!(trusted_contract("json", 2).is_none());
    }

    #[test]
    fn json_v1_returns_bounded_machine_readable_guest_errors() {
        let mut registry = LibraryRegistry::rust_standard();
        for (input, expected) in [
            (r#"{"a":1,"a":2}"#.to_owned(), "JSON_DUPLICATE_KEY"),
            (r#"{"a":1,"\u0061":2}"#.to_owned(), "JSON_DUPLICATE_KEY"),
            ("1.5".to_owned(), "JSON_NON_INTEGER_NUMBER"),
            ("1e3".to_owned(), "JSON_NON_INTEGER_NUMBER"),
            ("01".to_owned(), "JSON_SYNTAX"),
            (r#""\uD800""#.to_owned(), "JSON_SYNTAX"),
            ("[1,]".to_owned(), "JSON_SYNTAX"),
            (
                format!("{}null{}", "[".repeat(64), "]".repeat(64)),
                "JSON_DEPTH_LIMIT",
            ),
            (
                format!("[{}]", vec!["null"; 10_000].join(",")),
                "JSON_NODE_LIMIT",
            ),
            (" ".repeat(MAXIMUM_JSON_INPUT_BYTES + 1), "JSON_INPUT_LIMIT"),
        ] {
            let parsed = registry
                .invoke("json", 1, "parse", &[LibraryValue::String(input)])
                .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
            assert_eq!(guest_error_code(&parsed.value), expected);
        }

        let symbol = registry
            .invoke(
                "json",
                1,
                "stringify-canonical",
                &[LibraryValue::Symbol("not-json".to_owned())],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(guest_error_code(&symbol.value), "JSON_UNSUPPORTED_VALUE");

        let amplified = registry
            .invoke(
                "json",
                1,
                "stringify-canonical",
                &[LibraryValue::String("\u{0001}".repeat(200_000))],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(guest_error_code(&amplified.value), "JSON_OUTPUT_LIMIT");

        let escaped = registry
            .invoke(
                "json",
                1,
                "stringify-canonical",
                &[LibraryValue::Map(BTreeMap::from([(
                    LibraryKey::String("control".to_owned()),
                    LibraryValue::String("\u{0001}\n\"\\".to_owned()),
                )]))],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            escaped.value,
            LibraryValue::Ok(Box::new(LibraryValue::String(
                r#"{"control":"\u0001\n\"\\"}"#.to_owned()
            )))
        );
    }

    #[test]
    fn decimal_v1_round_trips_exact_scaled_integers() {
        let mut registry = LibraryRegistry::rust_standard();
        for (input, scale, expected) in [
            ("12.34", 2_u16, 1_234_i32),
            ("-0.5", 2, -50),
            ("1.2300", 2, 123),
            ("0", 0, 0),
        ] {
            let parsed = registry
                .invoke(
                    "decimal",
                    1,
                    "parse-scaled",
                    &[
                        LibraryValue::String(input.to_owned()),
                        LibraryValue::Int(BigInt::from(scale)),
                    ],
                )
                .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
            assert_eq!(
                guest_ok(&parsed.value),
                &LibraryValue::Int(BigInt::from(expected))
            );
        }

        for (value, scale, expected) in [
            (1_234_i32, 2_u16, "12.34"),
            (-5, 2, "-0.05"),
            (12, 0, "12"),
            (0, 3, "0.000"),
        ] {
            let formatted = registry
                .invoke(
                    "decimal",
                    1,
                    "format-scaled",
                    &[
                        LibraryValue::Int(BigInt::from(value)),
                        LibraryValue::Int(BigInt::from(scale)),
                    ],
                )
                .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
            assert_eq!(
                guest_ok(&formatted.value),
                &LibraryValue::String(expected.to_owned())
            );
        }
        assert!(trusted_contract("decimal", 1).is_some());
        assert!(trusted_contract("decimal", 2).is_none());
    }

    #[test]
    fn decimal_v1_requires_explicit_deterministic_rounding() {
        let mut registry = LibraryRegistry::rust_standard();
        for (value, mode, expected) in [
            (125_i32, "toward-zero", 12_i32),
            (-125, "toward-zero", -12),
            (-125, "floor", -13),
            (-125, "ceiling", -12),
            (125, "half-up", 13),
            (-125, "half-up", -13),
            (125, "half-even", 12),
            (135, "half-even", 14),
        ] {
            let rounded = registry
                .invoke(
                    "decimal",
                    1,
                    "rescale",
                    &[
                        LibraryValue::Int(BigInt::from(value)),
                        LibraryValue::Int(BigInt::from(2_u8)),
                        LibraryValue::Int(BigInt::from(1_u8)),
                        LibraryValue::String(mode.to_owned()),
                    ],
                )
                .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
            assert_eq!(
                guest_ok(&rounded.value),
                &LibraryValue::Int(BigInt::from(expected))
            );
        }

        let exact = registry
            .invoke(
                "decimal",
                1,
                "rescale",
                &[
                    LibraryValue::Int(BigInt::from(125_u16)),
                    LibraryValue::Int(BigInt::from(2_u8)),
                    LibraryValue::Int(BigInt::from(1_u8)),
                    LibraryValue::String("exact".to_owned()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(guest_error_code(&exact.value), "DECIMAL_ROUNDING_REQUIRED");

        let expanded = registry
            .invoke(
                "decimal",
                1,
                "rescale",
                &[
                    LibraryValue::Int(BigInt::from(12_u8)),
                    LibraryValue::Int(BigInt::from(1_u8)),
                    LibraryValue::Int(BigInt::from(3_u8)),
                    LibraryValue::String("exact".to_owned()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            guest_ok(&expanded.value),
            &LibraryValue::Int(BigInt::from(1_200_u16))
        );
    }

    #[test]
    fn decimal_v1_returns_bounded_errors_and_scale_sensitive_fuel() {
        let mut registry = LibraryRegistry::rust_standard();
        for (input, scale, expected) in [
            ("01.00".to_owned(), 2_u16, "DECIMAL_SYNTAX"),
            ("1.234".to_owned(), 2, "DECIMAL_PRECISION_LOSS"),
            (
                "1".repeat(MAXIMUM_DECIMAL_INPUT_BYTES + 1),
                2,
                "DECIMAL_INPUT_LIMIT",
            ),
            ("1".to_owned(), 1_025, "DECIMAL_SCALE_LIMIT"),
        ] {
            let parsed = registry
                .invoke(
                    "decimal",
                    1,
                    "parse-scaled",
                    &[
                        LibraryValue::String(input),
                        LibraryValue::Int(BigInt::from(scale)),
                    ],
                )
                .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
            assert_eq!(guest_error_code(&parsed.value), expected);
        }

        let invalid_mode = registry
            .invoke(
                "decimal",
                1,
                "rescale",
                &[
                    LibraryValue::Int(BigInt::from(1_u8)),
                    LibraryValue::Int(BigInt::from(1_u8)),
                    LibraryValue::Int(BigInt::ZERO),
                    LibraryValue::String("host-default".to_owned()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            guest_error_code(&invalid_mode.value),
            "DECIMAL_INVALID_ROUNDING_MODE"
        );

        let maximum_magnitude = BigInt::from(1_u8) << 65_535_usize;
        let amplified = registry
            .invoke(
                "decimal",
                1,
                "rescale",
                &[
                    LibraryValue::Int(maximum_magnitude),
                    LibraryValue::Int(BigInt::ZERO),
                    LibraryValue::Int(BigInt::from(1_u8)),
                    LibraryValue::String("exact".to_owned()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(guest_error_code(&amplified.value), "DECIMAL_INTEGER_LIMIT");

        let small_fuel = registry
            .call_fuel(
                "decimal",
                1,
                "rescale",
                &[
                    LibraryValue::Int(BigInt::from(1_u8)),
                    LibraryValue::Int(BigInt::ZERO),
                    LibraryValue::Int(BigInt::from(2_u8)),
                    LibraryValue::String("half-even".to_owned()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let large_fuel = registry
            .call_fuel(
                "decimal",
                1,
                "rescale",
                &[
                    LibraryValue::Int(BigInt::from(1_u8)),
                    LibraryValue::Int(BigInt::ZERO),
                    LibraryValue::Int(BigInt::from(1_024_u16)),
                    LibraryValue::String("half-even".to_owned()),
                ],
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert!(large_fuel > small_fuel);
    }

    #[test]
    fn registry_rejects_shape_changes_and_redacts_backend_failures() {
        let mut invalid = LibraryRegistry::default();
        let diagnostic = match invalid.register(Box::new(InvalidBackend)) {
            Err(diagnostic) => diagnostic,
            Ok(()) => panic!("expected invalid backend diagnostic"),
        };
        assert_eq!(diagnostic.code, "RUNTIME_INVALID_LIBRARY_BACKEND");

        let mut failing = LibraryRegistry::default();
        failing
            .register(Box::new(FailingBackend))
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let diagnostic = match failing.invoke(
            "text",
            1,
            "length",
            &[LibraryValue::String("secret".to_owned())],
        ) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected backend failure"),
        };
        assert_eq!(diagnostic.code, "RUNTIME_LIBRARY_FAILURE");
        assert!(!diagnostic.to_string().contains("credential"));
        assert!(!diagnostic.to_string().contains("SECRET_BACKEND_ERROR"));
    }
}
