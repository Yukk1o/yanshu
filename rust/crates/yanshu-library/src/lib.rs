#![forbid(unsafe_code)]

mod contract;
mod math;
mod registry;
mod text;
mod value;

use yanshu_diagnostic::YanshuResult;

pub use contract::{
    FuelModel, LibraryContract, LibraryType, MATH_V1, OperationContract, TEXT_V1, TEXT_V2,
    is_trusted_operation_name, trusted_contract,
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
    use num_bigint::BigInt;
    use yanshu_diagnostic::{Diagnostic, YanshuResult};

    use crate::{
        BackendDescriptor, LibraryBackend, LibraryRegistry, LibraryValue, RustTextBackend,
        trusted_contract,
    };

    fn require_error<T>(result: YanshuResult<T>) -> Diagnostic {
        match result {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected a diagnostic"),
        }
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
