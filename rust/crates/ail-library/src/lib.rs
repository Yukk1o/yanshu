#![forbid(unsafe_code)]

mod contract;
mod registry;
mod text;
mod value;

use ail_diagnostic::AilResult;

pub use contract::{
    FuelModel, LibraryContract, LibraryType, OperationContract, TEXT_V1, trusted_contract,
};
pub use registry::{BackendDescriptor, LibraryInvocation, LibraryRegistry};
pub use text::RustTextBackend;
pub use value::{LibraryKey, LibraryValue};

pub trait LibraryBackend: Send {
    fn descriptor(&self) -> BackendDescriptor;
    fn invoke(&mut self, operation: &str, arguments: &[LibraryValue]) -> AilResult<LibraryValue>;
}

#[cfg(test)]
mod tests {
    use ail_diagnostic::{AilResult, Diagnostic};

    use crate::{
        BackendDescriptor, LibraryBackend, LibraryRegistry, LibraryValue, RustTextBackend,
    };

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
        ) -> AilResult<LibraryValue> {
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
        ) -> AilResult<LibraryValue> {
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
