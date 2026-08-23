#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
};

use serde_json::json;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{
    LibraryBackend, LibraryContract, LibraryValue, RustTextBackend, RustTextV2Backend,
    trusted_contract,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDescriptor {
    pub provider: String,
    pub library: String,
    pub version: u16,
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryInvocation {
    pub value: LibraryValue,
    pub fuel: u64,
}

#[derive(Default)]
pub struct LibraryRegistry {
    backends: BTreeMap<(String, u16), RegisteredBackend>,
}

struct RegisteredBackend {
    provider: String,
    backend: Box<dyn LibraryBackend>,
}

impl LibraryRegistry {
    #[must_use]
    pub fn rust_standard() -> Self {
        let mut registry = Self::default();
        registry.backends.insert(
            ("text".to_owned(), 1),
            RegisteredBackend {
                provider: "rust-std".to_owned(),
                backend: Box::<RustTextBackend>::default(),
            },
        );
        registry.backends.insert(
            ("text".to_owned(), 2),
            RegisteredBackend {
                provider: "rust-std".to_owned(),
                backend: Box::<RustTextV2Backend>::default(),
            },
        );
        registry
    }

    pub fn register(&mut self, backend: Box<dyn LibraryBackend>) -> YanshuResult<()> {
        let descriptor = backend.descriptor();
        validate_provider(&descriptor.provider)?;
        if !valid_contract_identifier(&descriptor.library)
            || descriptor.operations.len() > 256
            || descriptor
                .operations
                .iter()
                .any(|operation| !valid_operation_name(operation))
        {
            return Err(Diagnostic::simple(
                "RUNTIME_INVALID_LIBRARY_BACKEND",
                "backend descriptor contains an invalid contract identifier",
            ));
        }
        let contract =
            trusted_contract(&descriptor.library, descriptor.version).ok_or_else(|| {
                Diagnostic::new(
                    "RUNTIME_LIBRARY_CONTRACT_MISSING",
                    "backend refers to an unknown trusted library contract",
                    json!({ "library": descriptor.library, "version": descriptor.version }),
                )
            })?;
        let expected = contract
            .operations
            .iter()
            .map(|operation| operation.name.to_owned())
            .collect::<BTreeSet<_>>();
        let actual = descriptor
            .operations
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if actual.len() != descriptor.operations.len() || actual != expected {
            return Err(Diagnostic::new(
                "RUNTIME_INVALID_LIBRARY_BACKEND",
                "backend operation set does not exactly match the trusted contract",
                json!({
                    "library": descriptor.library,
                    "version": descriptor.version,
                    "expected": expected,
                    "actual": actual,
                }),
            ));
        }
        let key = (descriptor.library.clone(), descriptor.version);
        if self.backends.contains_key(&key) {
            return Err(Diagnostic::new(
                "RUNTIME_DUPLICATE_LIBRARY_BACKEND",
                "host registered more than one backend for a library contract",
                json!({ "library": descriptor.library, "version": descriptor.version }),
            ));
        }
        self.backends.insert(
            key,
            RegisteredBackend {
                provider: descriptor.provider,
                backend,
            },
        );
        Ok(())
    }

    pub fn require(&self, library: &str, version: u16) -> YanshuResult<LibraryContract> {
        let contract = trusted_contract(library, version).ok_or_else(|| {
            Diagnostic::new(
                "RUNTIME_LIBRARY_CONTRACT_MISSING",
                "program refers to an unknown trusted library contract",
                json!({ "library": library, "version": version }),
            )
        })?;
        if self.backends.contains_key(&(library.to_owned(), version)) {
            Ok(contract)
        } else {
            Err(Diagnostic::new(
                "RUNTIME_LIBRARY_UNAVAILABLE",
                "host did not provide a declared library backend",
                json!({ "library": library, "version": version }),
            ))
        }
    }

    pub fn invoke(
        &mut self,
        library: &str,
        version: u16,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryInvocation> {
        let fuel = self.call_fuel(library, version, operation, arguments)?;
        let contract = self.require(library, version)?;
        let operation_contract = contract.operation(operation).ok_or_else(|| {
            Diagnostic::new(
                "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                "library operation is not part of its trusted contract",
                json!({ "library": library, "version": version, "operation": operation }),
            )
        })?;
        let registered = self
            .backends
            .get_mut(&(library.to_owned(), version))
            .ok_or_else(|| {
                Diagnostic::new(
                    "RUNTIME_LIBRARY_UNAVAILABLE",
                    "host did not provide a declared library backend",
                    json!({ "library": library, "version": version }),
                )
            })?;
        let provider = registered.provider.clone();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            registered.backend.invoke(operation, arguments)
        }));
        let value = match outcome {
            Ok(Ok(value)) => value,
            Ok(Err(_)) | Err(_) => {
                return Err(Diagnostic::new(
                    "RUNTIME_LIBRARY_FAILURE",
                    "library backend failed behind the host boundary",
                    json!({
                        "library": library,
                        "version": version,
                        "operation": operation,
                        "provider": provider,
                    }),
                ));
            }
        };
        operation_contract.validate_result(&value)?;
        Ok(LibraryInvocation { value, fuel })
    }

    pub fn call_fuel(
        &self,
        library: &str,
        version: u16,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<u64> {
        let contract = self.require(library, version)?;
        let operation_contract = contract.operation(operation).ok_or_else(|| {
            Diagnostic::new(
                "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                "library operation is not part of its trusted contract",
                json!({ "library": library, "version": version, "operation": operation }),
            )
        })?;
        operation_contract.validate_arguments_as(&format!("{library}/{operation}"), arguments)?;
        operation_contract.fuel.cost(arguments)
    }
}

fn validate_provider(provider: &str) -> YanshuResult<()> {
    if provider.is_empty()
        || provider.len() > 64
        || !provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        Err(Diagnostic::new(
            "RUNTIME_INVALID_LIBRARY_BACKEND",
            "backend provider label is invalid",
            json!({ "maximumLength": 64 }),
        ))
    } else {
        Ok(())
    }
}

fn valid_contract_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_operation_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'?' | b'!')
        })
}
