//! Bridging between the typeck [`Type`] and the IR [`RustType`].
//!
//! Converts the canonical type representation to the Rust IR type used
//! by the lowering pass and emitter.

use rsc_syntax::rust_ir::RustType;

use crate::types::{PrimitiveType, Type};

/// Convert a typeck [`Type`] to a [`RustType`] for the IR.
///
/// Types that don't yet have IR representations produce `RustType::Unit` as
/// a placeholder.
#[must_use]
pub fn type_to_rust_type(ty: &Type) -> RustType {
    match ty {
        Type::Primitive(prim) => primitive_to_rust_type(*prim),
        Type::String => RustType::String,
        Type::Named(name) => RustType::Named(name.clone()),
        Type::TypeVar(name) => RustType::TypeParam(name.clone()),
        Type::Generic(name, args) => {
            let base = RustType::Named(name.clone());
            let rust_args: Vec<RustType> = args.iter().map(type_to_rust_type).collect();
            RustType::Generic(Box::new(base), rust_args)
        }
        Type::Option(inner) => RustType::Option(Box::new(type_to_rust_type(inner))),
        Type::Function(params, ret) => {
            let rust_params: Vec<RustType> = params.iter().map(type_to_rust_type).collect();
            let rust_ret = type_to_rust_type(ret);
            RustType::ImplFn(rust_params, Box::new(rust_ret))
        }
        Type::Result(ok, err) => RustType::Result(
            Box::new(type_to_rust_type(ok)),
            Box::new(type_to_rust_type(err)),
        ),
        Type::Unit | Type::Error => RustType::Unit,
    }
}

/// Convert a primitive type to the corresponding `RustType`.
fn primitive_to_rust_type(prim: PrimitiveType) -> RustType {
    match prim {
        PrimitiveType::I8 => RustType::I8,
        PrimitiveType::I16 => RustType::I16,
        PrimitiveType::I32 => RustType::I32,
        PrimitiveType::I64 => RustType::I64,
        PrimitiveType::U8 => RustType::U8,
        PrimitiveType::U16 => RustType::U16,
        PrimitiveType::U32 => RustType::U32,
        PrimitiveType::U64 => RustType::U64,
        PrimitiveType::F32 => RustType::F32,
        PrimitiveType::F64 => RustType::F64,
        PrimitiveType::Bool => RustType::Bool,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_all_primitives() {
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::I8)),
            RustType::I8
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::I16)),
            RustType::I16
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::I32)),
            RustType::I32
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::I64)),
            RustType::I64
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::U8)),
            RustType::U8
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::U16)),
            RustType::U16
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::U32)),
            RustType::U32
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::U64)),
            RustType::U64
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::F32)),
            RustType::F32
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::F64)),
            RustType::F64
        );
        assert_eq!(
            type_to_rust_type(&Type::Primitive(PrimitiveType::Bool)),
            RustType::Bool
        );
    }

    #[test]
    fn test_bridge_string() {
        assert_eq!(type_to_rust_type(&Type::String), RustType::String);
    }

    #[test]
    fn test_bridge_unit() {
        assert_eq!(type_to_rust_type(&Type::Unit), RustType::Unit);
    }

    #[test]
    fn test_bridge_named_type_produces_named() {
        assert_eq!(
            type_to_rust_type(&Type::Named("Foo".to_owned())),
            RustType::Named("Foo".to_owned())
        );
    }

    #[test]
    fn test_bridge_type_param_produces_type_param() {
        assert_eq!(
            type_to_rust_type(&Type::TypeVar("T".to_owned())),
            RustType::TypeParam("T".to_owned())
        );
    }

    #[test]
    fn test_bridge_generic_produces_generic() {
        assert_eq!(
            type_to_rust_type(&Type::Generic("Vec".to_owned(), vec![Type::String])),
            RustType::Generic(
                Box::new(RustType::Named("Vec".to_owned())),
                vec![RustType::String]
            )
        );
    }

    #[test]
    fn test_bridge_option_type_produces_option() {
        assert_eq!(
            type_to_rust_type(&Type::Option(Box::new(Type::String))),
            RustType::Option(Box::new(RustType::String))
        );
    }

    #[test]
    fn test_bridge_result_type_produces_result() {
        assert_eq!(
            type_to_rust_type(&Type::Result(Box::new(Type::String), Box::new(Type::Unit))),
            RustType::Result(Box::new(RustType::String), Box::new(RustType::Unit))
        );
    }

    #[test]
    fn test_bridge_error_type_produces_unit() {
        assert_eq!(type_to_rust_type(&Type::Error), RustType::Unit);
    }

    #[test]
    fn test_bridge_function_type_produces_impl_fn() {
        assert_eq!(
            type_to_rust_type(&Type::Function(
                vec![Type::Primitive(PrimitiveType::I32)],
                Box::new(Type::Primitive(PrimitiveType::I32))
            )),
            RustType::ImplFn(vec![RustType::I32], Box::new(RustType::I32))
        );
        assert_eq!(
            type_to_rust_type(&Type::Function(vec![], Box::new(Type::Unit))),
            RustType::ImplFn(vec![], Box::new(RustType::Unit))
        );
    }
}
