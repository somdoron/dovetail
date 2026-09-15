//! Awaitable signatures used when lowering introduces intermediate computations.
//! Inference already validated the implementation; lowering substitutes its
//! declared Rebind/map result instead of assuming any concrete wrapper layout.
use std::cell::RefCell;

use crate::common::types::{Fqn, SymbolName, TypeParamName};
use crate::typechecker::infer::generics::apply_substitution;
use crate::typechecker::infer::type_param_substitution::TypeParamSubstitution;
use crate::typechecker::types::{ResolvedImplMethod, Type, TypedModule};

struct AwaitableSignature {
    for_type: Type,
    type_params: Vec<TypeParamName>,
    success: Type,
    rebound_param: TypeParamName,
    rebound_type: Type,
}

thread_local! {
    static SIGNATURES: RefCell<Vec<AwaitableSignature>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn snapshot(module: &TypedModule) {
    let awaitable = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
    let signatures = module
        .implement_blocks
        .iter()
        .filter_map(|block| {
            if block.trait_fqn != awaitable {
                return None;
            }
            let map = block.methods.iter().find(|method| method.name.0 == "map")?;
            let parameters: Vec<_> = block
                .type_params
                .iter()
                .chain(&map.method_type_params)
                .cloned()
                .collect();
            let variables: Vec<_> = parameters
                .iter()
                .map(|name| Type::TypeVariable(name.clone(), vec![]))
                .collect();
            let template = TypeParamSubstitution::from_pairs(&parameters, &variables);
            Some(AwaitableSignature {
                for_type: apply_substitution(&template, &block.for_type),
                type_params: block.type_params.clone(),
                success: apply_substitution(&template, block.trait_type_args.first()?),
                rebound_param: map.method_type_params.first()?.clone(),
                rebound_type: apply_substitution(&template, &map.return_type),
            })
        })
        .collect();
    SIGNATURES.with(|slot| *slot.borrow_mut() = signatures);
}

fn with_signature<T>(
    context: &Type,
    f: impl FnOnce(&AwaitableSignature, TypeParamSubstitution) -> T,
) -> Option<T> {
    SIGNATURES.with(|slot| {
        for signature in slot.borrow().iter() {
            if signature.for_type.try_to_fqn() != context.try_to_fqn() {
                continue;
            }
            let mut substitution = TypeParamSubstitution::new();
            if substitution.unify(&signature.for_type, context) {
                return Some(f(signature, substitution));
            }
        }
        None
    })
}

pub(super) fn success_type(context: &Type) -> Option<Type> {
    with_signature(context, |signature, substitution| {
        apply_substitution(&substitution, &signature.success)
    })
}

pub(super) fn rebind(context: &Type, success: Type) -> Type {
    with_signature(context, |signature, mut substitution| {
        substitution.insert(signature.rebound_param.clone(), success);
        apply_substitution(&substitution, &signature.rebound_type)
    })
    .unwrap_or_else(|| panic!("missing checked Awaitable implementation for {context}"))
}

pub(super) fn method(
    name: &str,
    context: Type,
    success: Type,
    method_params: Vec<Type>,
) -> ResolvedImplMethod {
    let block_params = with_signature(&context, |signature, substitution| {
        substitution
            .resolve_type_params(&signature.type_params)
            .expect("checked Awaitable type parameters")
    })
    .unwrap_or_else(|| panic!("missing checked Awaitable implementation for {context}"));
    ResolvedImplMethod {
        trait_fqn: Fqn::from_dotted("standard.prelude.Awaitable").unwrap(),
        trait_type_params: vec![success],
        for_type: context,
        method_name: SymbolName(name.into()),
        method_type_params: block_params.into_iter().chain(method_params).collect(),
    }
}
