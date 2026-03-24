//! Registry of user-defined types.
//!
//! Stores type definitions that have been registered during a pre-pass
//! over the AST. The lowering pass queries this registry to resolve
//! user-defined type names and look up struct field information.

use std::collections::HashMap;

use crate::types::Type;

/// A user-defined type definition stored in the registry.
#[derive(Debug, Clone)]
pub struct RegisteredTypeDef {
    /// The type name.
    pub name: String,
    /// The fields: `(field_name, field_type)` pairs, in declaration order.
    pub fields: Vec<(String, Type)>,
}

/// Registry of user-defined types.
///
/// Populated during a pre-pass before lowering function bodies. The lowering
/// pass queries this to resolve user-defined type names and struct field types.
#[derive(Debug)]
pub struct TypeRegistry {
    types: HashMap<String, RegisteredTypeDef>,
}

impl TypeRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            types: HashMap::new(),
        }
    }

    /// Register a user-defined type.
    pub fn register(&mut self, name: String, fields: Vec<(String, Type)>) {
        self.types
            .insert(name.clone(), RegisteredTypeDef { name, fields });
    }

    /// Look up a registered type by name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&RegisteredTypeDef> {
        self.types.get(name)
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PrimitiveType;

    #[test]
    fn test_registry_register_and_lookup() {
        let mut reg = TypeRegistry::new();
        reg.register(
            "User".to_owned(),
            vec![
                ("name".to_owned(), Type::String),
                ("age".to_owned(), Type::Primitive(PrimitiveType::U32)),
            ],
        );

        let td = reg.lookup("User");
        assert!(td.is_some());
        let td = td.unwrap();
        assert_eq!(td.name, "User");
        assert_eq!(td.fields.len(), 2);
        assert_eq!(td.fields[0].0, "name");
        assert_eq!(td.fields[0].1, Type::String);
        assert_eq!(td.fields[1].0, "age");
        assert_eq!(td.fields[1].1, Type::Primitive(PrimitiveType::U32));
    }

    #[test]
    fn test_registry_lookup_missing_returns_none() {
        let reg = TypeRegistry::new();
        assert!(reg.lookup("Missing").is_none());
    }

    #[test]
    fn test_registry_default() {
        let reg = TypeRegistry::default();
        assert!(reg.lookup("anything").is_none());
    }
}
