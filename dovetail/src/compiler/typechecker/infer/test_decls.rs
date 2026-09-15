use crate::common::types::MangledName;
use crate::parser::ast::{TestAttribute, TestDecl};

use crate::typechecker::types::{Type, TypedTest};

use super::Inference;

impl Inference<'_> {
    /// Infer types for a test declaration body.
    /// Tests are like no-param functions returning Unit.
    pub(super) fn infer_test(&mut self, test: &TestDecl) {
        // Validate attributes
        let mut skip_reason: Option<Option<String>> = None;
        let mut expected_panic: Option<Option<String>> = None;
        let mut timeout_ms: Option<u64> = None;

        for attr in &test.attributes {
            match attr {
                TestAttribute::Skip { reason, span } => {
                    if skip_reason.is_some() {
                        self.diagnostics.error(span.clone(), "duplicate @skip attribute".to_string());
                    } else {
                        skip_reason = Some(reason.as_ref().map(|r| r.value.clone()));
                    }
                }
                TestAttribute::Panics { message, span } => {
                    if expected_panic.is_some() {
                        self.diagnostics.error(span.clone(), "duplicate @panics attribute".to_string());
                    } else {
                        expected_panic = Some(message.as_ref().map(|m| m.value.clone()));
                    }
                }
                TestAttribute::Timeout { millis, span } => {
                    if timeout_ms.is_some() {
                        self.diagnostics.error(span.clone(), "duplicate @timeout attribute".to_string());
                    } else {
                        match millis.value.parse::<u64>() {
                            Ok(ms) if ms > 0 => {
                                timeout_ms = Some(ms);
                            }
                            Ok(_) => {
                                self.diagnostics.error(span.clone(), "@timeout value must be greater than 0".to_string());
                            }
                            Err(_) => {
                                self.diagnostics.error(span.clone(), "@timeout value must be a positive integer".to_string());
                            }
                        }
                    }
                }
            }
        }

        self.push_scope();

        let prev_expected = self.expected_type.take();
        let prev_fn_return = self.function_return_type.take();
        self.expected_type = Some(Type::Unit);
        self.function_return_type = Some(Type::Unit);

        let body = self.infer_expr(&test.body);

        self.expected_type = prev_expected;
        self.function_return_type = prev_fn_return;
        self.pop_scope();

        self.check_assignable(body.span.clone(), &Type::Unit, &body.ty);

        let mangled_name = MangledName::for_test(&self.package_path, &test.name.value);
        let fqtn = format!("{} {}", self.package_path, test.name.value);

        self.typed_tests.push(TypedTest {
            name: test.name.value.clone(),
            fqtn,
            package_path: self.package_path.clone(),
            return_type: Type::Unit,
            body,
            mangled_name,
            span: test.span.clone(),
            skip_reason,
            expected_panic,
            timeout_ms,
        });
    }
}
