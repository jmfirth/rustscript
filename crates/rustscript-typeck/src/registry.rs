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
    /// The kind of this type definition.
    pub kind: TypeDefKind,
}

/// The kind of a registered type definition.
#[derive(Debug, Clone)]
pub enum TypeDefKind {
    /// A struct type with named fields.
    Struct(Vec<(String, Type)>),
    /// A simple enum with variant names (no data).
    SimpleEnum(Vec<String>),
    /// A data enum (discriminated union) with variant names and their fields.
    DataEnum(Vec<(String, Vec<(String, Type)>)>),
    /// An interface (trait) with method signatures.
    /// Each method has a name and a list of parameter types plus a return type.
    Interface(Vec<InterfaceMethodSig>),
    /// A class type with fields, getters, setters, and static methods.
    Class {
        /// The struct fields of the class.
        fields: Vec<(String, Type)>,
        /// The base class this class extends (single inheritance).
        base_class: Option<String>,
        /// Instance method signatures, populated when this class is used as a
        /// base class for inheritance. Enables derived classes to generate trait impls.
        methods: Vec<InterfaceMethodSig>,
        /// Names of getter accessors (property-style read access).
        getters: Vec<String>,
        /// Names of setter accessors (property-style write access).
        setters: Vec<String>,
        /// Names of static methods (called via `ClassName.method()`).
        static_methods: Vec<String>,
    },
}

/// A method signature in a registered interface.
#[derive(Debug, Clone)]
pub struct InterfaceMethodSig {
    /// The method name.
    pub name: String,
    /// The parameter types (excluding `self`).
    pub param_types: Vec<(String, Type)>,
    /// The return type (`None` means `void`/unit).
    pub return_type: Option<Type>,
}

impl RegisteredTypeDef {
    /// Get the struct fields if this is a struct or class type.
    ///
    /// Returns `None` for enum types.
    #[must_use]
    pub fn struct_fields(&self) -> Option<&[(String, Type)]> {
        match &self.kind {
            TypeDefKind::Struct(fields) | TypeDefKind::Class { fields, .. } => Some(fields),
            _ => None,
        }
    }
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

    /// Register a struct type.
    pub fn register(&mut self, name: String, fields: Vec<(String, Type)>) {
        self.types.insert(
            name.clone(),
            RegisteredTypeDef {
                name,
                kind: TypeDefKind::Struct(fields),
            },
        );
    }

    /// Register a class type with getters, setters, and static methods.
    pub fn register_class(
        &mut self,
        name: String,
        fields: Vec<(String, Type)>,
        base_class: Option<String>,
        getters: Vec<String>,
        setters: Vec<String>,
        static_methods: Vec<String>,
    ) {
        self.types.insert(
            name.clone(),
            RegisteredTypeDef {
                name,
                kind: TypeDefKind::Class {
                    fields,
                    base_class,
                    methods: Vec::new(),
                    getters,
                    setters,
                    static_methods,
                },
            },
        );
    }

    /// Set method signatures on an already-registered class.
    ///
    /// Called during the pre-pass when a concrete class is identified as a base
    /// class (another class `extends` it). The methods enable derived classes to
    /// generate `impl {Name}Trait for DerivedClass` during lowering.
    pub fn set_class_methods(&mut self, name: &str, methods: Vec<InterfaceMethodSig>) {
        if let Some(td) = self.types.get_mut(name)
            && let TypeDefKind::Class {
                methods: ref mut existing_methods,
                ..
            } = td.kind
        {
            *existing_methods = methods;
        }
    }

    /// Register a simple enum type.
    pub fn register_simple_enum(&mut self, name: String, variants: Vec<String>) {
        self.types.insert(
            name.clone(),
            RegisteredTypeDef {
                name,
                kind: TypeDefKind::SimpleEnum(variants),
            },
        );
    }

    /// Register a data enum (discriminated union) type.
    pub fn register_data_enum(
        &mut self,
        name: String,
        variants: Vec<(String, Vec<(String, Type)>)>,
    ) {
        self.types.insert(
            name.clone(),
            RegisteredTypeDef {
                name,
                kind: TypeDefKind::DataEnum(variants),
            },
        );
    }

    /// Register an interface (trait) type.
    pub fn register_interface(&mut self, name: String, methods: Vec<InterfaceMethodSig>) {
        self.types.insert(
            name.clone(),
            RegisteredTypeDef {
                name,
                kind: TypeDefKind::Interface(methods),
            },
        );
    }

    /// Look up a registered type by name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&RegisteredTypeDef> {
        self.types.get(name)
    }

    /// Check whether a registered type is a simple enum (no data variants).
    ///
    /// Simple enums are `Copy` because they have no associated data.
    /// Returns `false` if the name is not registered or is not a simple enum.
    #[must_use]
    pub fn is_simple_enum(&self, name: &str) -> bool {
        self.types
            .get(name)
            .is_some_and(|td| matches!(td.kind, TypeDefKind::SimpleEnum(_)))
    }

    /// Look up the method signatures for a registered interface or class.
    ///
    /// Returns methods from `Interface` types (abstract classes, interfaces)
    /// and from `Class` types that have methods set (concrete base classes).
    /// Returns `None` if the name is not registered or has no methods.
    #[must_use]
    pub fn get_interface_methods(&self, name: &str) -> Option<&[InterfaceMethodSig]> {
        self.types.get(name).and_then(|td| match &td.kind {
            TypeDefKind::Interface(methods) => Some(methods.as_slice()),
            TypeDefKind::Class { methods, .. } if !methods.is_empty() => Some(methods.as_slice()),
            _ => None,
        })
    }

    /// Check whether a class has a getter for a given field name.
    #[must_use]
    pub fn has_getter(&self, class_name: &str, field_name: &str) -> bool {
        self.types.get(class_name).is_some_and(|td| match &td.kind {
            TypeDefKind::Class { getters, .. } => getters.iter().any(|g| g == field_name),
            _ => false,
        })
    }

    /// Check whether a class has a setter for a given field name.
    #[must_use]
    pub fn has_setter(&self, class_name: &str, field_name: &str) -> bool {
        self.types.get(class_name).is_some_and(|td| match &td.kind {
            TypeDefKind::Class { setters, .. } => setters.iter().any(|s| s == field_name),
            _ => false,
        })
    }

    /// Check whether a type with the given name is registered.
    #[must_use]
    pub fn has_type(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    /// Get the base class name for a class, if it extends another class.
    #[must_use]
    pub fn get_base_class(&self, name: &str) -> Option<&str> {
        self.types.get(name).and_then(|td| match &td.kind {
            TypeDefKind::Class { base_class, .. } => base_class.as_deref(),
            _ => None,
        })
    }

    /// Get the fields for a class type.
    #[must_use]
    pub fn get_class_fields(&self, name: &str) -> Option<&[(String, Type)]> {
        self.types.get(name).and_then(|td| match &td.kind {
            TypeDefKind::Class { fields, .. } => Some(fields.as_slice()),
            _ => None,
        })
    }

    /// Check whether a registered type has a static method with the given name.
    #[must_use]
    pub fn has_static_method(&self, class_name: &str, method_name: &str) -> bool {
        self.types.get(class_name).is_some_and(|td| match &td.kind {
            TypeDefKind::Class { static_methods, .. } => {
                static_methods.iter().any(|m| m == method_name)
            }
            _ => false,
        })
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
        match &td.kind {
            TypeDefKind::Struct(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "name");
                assert_eq!(fields[0].1, Type::String);
                assert_eq!(fields[1].0, "age");
                assert_eq!(fields[1].1, Type::Primitive(PrimitiveType::U32));
            }
            _ => panic!("expected Struct"),
        }
    }

    #[test]
    fn test_registry_register_simple_enum() {
        let mut reg = TypeRegistry::new();
        reg.register_simple_enum(
            "Direction".to_owned(),
            vec![
                "North".to_owned(),
                "South".to_owned(),
                "East".to_owned(),
                "West".to_owned(),
            ],
        );

        let td = reg.lookup("Direction");
        assert!(td.is_some());
        let td = td.unwrap();
        match &td.kind {
            TypeDefKind::SimpleEnum(variants) => {
                assert_eq!(variants.len(), 4);
                assert_eq!(variants[0], "North");
            }
            _ => panic!("expected SimpleEnum"),
        }
    }

    #[test]
    fn test_registry_register_data_enum() {
        let mut reg = TypeRegistry::new();
        reg.register_data_enum(
            "Shape".to_owned(),
            vec![
                (
                    "Circle".to_owned(),
                    vec![("radius".to_owned(), Type::Primitive(PrimitiveType::F64))],
                ),
                (
                    "Rect".to_owned(),
                    vec![
                        ("width".to_owned(), Type::Primitive(PrimitiveType::F64)),
                        ("height".to_owned(), Type::Primitive(PrimitiveType::F64)),
                    ],
                ),
            ],
        );

        let td = reg.lookup("Shape");
        assert!(td.is_some());
        let td = td.unwrap();
        match &td.kind {
            TypeDefKind::DataEnum(variants) => {
                assert_eq!(variants.len(), 2);
                assert_eq!(variants[0].0, "Circle");
                assert_eq!(variants[0].1.len(), 1);
                assert_eq!(variants[1].0, "Rect");
                assert_eq!(variants[1].1.len(), 2);
            }
            _ => panic!("expected DataEnum"),
        }
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

    // ---- Task 022: Interface registration test ----

    #[test]
    fn test_registry_register_interface_stores_and_retrieves() {
        let mut reg = TypeRegistry::new();
        reg.register_interface(
            "Serializable".to_owned(),
            vec![InterfaceMethodSig {
                name: "serialize".to_owned(),
                param_types: vec![],
                return_type: Some(Type::String),
            }],
        );

        let td = reg.lookup("Serializable");
        assert!(td.is_some());
        let td = td.unwrap();
        assert_eq!(td.name, "Serializable");
        match &td.kind {
            TypeDefKind::Interface(methods) => {
                assert_eq!(methods.len(), 1);
                assert_eq!(methods[0].name, "serialize");
                assert_eq!(methods[0].return_type, Some(Type::String));
            }
            _ => panic!("expected Interface"),
        }
    }

    // ---- Task 057: Class registration tests ----

    #[test]
    fn test_registry_register_class_with_getters_setters_static_methods() {
        let mut reg = TypeRegistry::new();
        reg.register_class(
            "User".to_owned(),
            vec![
                ("name".to_owned(), Type::String),
                ("age".to_owned(), Type::Primitive(PrimitiveType::U32)),
            ],
            None,
            vec!["full_name".to_owned()],
            vec!["full_name".to_owned()],
            vec!["create".to_owned()],
        );

        let td = reg.lookup("User");
        assert!(td.is_some());

        // Check struct_fields works for Class kind
        let fields = td.unwrap().struct_fields();
        assert!(fields.is_some());
        assert_eq!(fields.unwrap().len(), 2);
    }

    #[test]
    fn test_registry_has_getter_for_class() {
        let mut reg = TypeRegistry::new();
        reg.register_class(
            "User".to_owned(),
            vec![],
            None,
            vec!["name".to_owned()],
            vec![],
            vec![],
        );

        assert!(reg.has_getter("User", "name"));
        assert!(!reg.has_getter("User", "age"));
        assert!(!reg.has_getter("Missing", "name"));
    }

    #[test]
    fn test_registry_has_setter_for_class() {
        let mut reg = TypeRegistry::new();
        reg.register_class(
            "User".to_owned(),
            vec![],
            None,
            vec![],
            vec!["name".to_owned()],
            vec![],
        );

        assert!(reg.has_setter("User", "name"));
        assert!(!reg.has_setter("User", "age"));
    }

    #[test]
    fn test_registry_has_static_method_for_class() {
        let mut reg = TypeRegistry::new();
        reg.register_class(
            "Factory".to_owned(),
            vec![],
            None,
            vec![],
            vec![],
            vec!["create".to_owned(), "default".to_owned()],
        );

        assert!(reg.has_static_method("Factory", "create"));
        assert!(reg.has_static_method("Factory", "default"));
        assert!(!reg.has_static_method("Factory", "instance_method"));
        assert!(!reg.has_static_method("Missing", "create"));
    }

    #[test]
    fn test_registry_has_type_returns_true_for_registered() {
        let mut reg = TypeRegistry::new();
        reg.register("User".to_owned(), vec![]);
        assert!(reg.has_type("User"));
    }

    #[test]
    fn test_registry_has_type_returns_false_for_missing() {
        let reg = TypeRegistry::new();
        assert!(!reg.has_type("Missing"));
    }

    // ---- Inheritance support tests ----

    #[test]
    fn test_registry_class_with_base_class() {
        let mut reg = TypeRegistry::new();
        reg.register_class(
            "Dog".to_owned(),
            vec![("name".to_owned(), Type::String)],
            Some("Animal".to_owned()),
            vec![],
            vec![],
            vec![],
        );

        assert_eq!(reg.get_base_class("Dog"), Some("Animal"));
        assert_eq!(reg.get_base_class("Missing"), None);
    }

    #[test]
    fn test_registry_class_without_base_class() {
        let mut reg = TypeRegistry::new();
        reg.register_class(
            "Animal".to_owned(),
            vec![("name".to_owned(), Type::String)],
            None,
            vec![],
            vec![],
            vec![],
        );

        assert_eq!(reg.get_base_class("Animal"), None);
    }

    #[test]
    fn test_registry_get_class_fields() {
        let mut reg = TypeRegistry::new();
        reg.register_class(
            "Animal".to_owned(),
            vec![
                ("name".to_owned(), Type::String),
                ("age".to_owned(), Type::Primitive(PrimitiveType::U32)),
            ],
            None,
            vec![],
            vec![],
            vec![],
        );

        let fields = reg.get_class_fields("Animal");
        assert!(fields.is_some());
        let fields = fields.unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, "name");
        assert_eq!(fields[1].0, "age");
    }

    #[test]
    fn test_registry_set_class_methods_and_get_interface_methods() {
        let mut reg = TypeRegistry::new();
        reg.register_class(
            "Animal".to_owned(),
            vec![("name".to_owned(), Type::String)],
            None,
            vec![],
            vec![],
            vec![],
        );

        // Initially no methods
        assert!(reg.get_interface_methods("Animal").is_none());

        // Set methods
        reg.set_class_methods(
            "Animal",
            vec![InterfaceMethodSig {
                name: "speak".to_owned(),
                param_types: vec![],
                return_type: Some(Type::String),
            }],
        );

        // Now methods should be available
        let methods = reg.get_interface_methods("Animal");
        assert!(methods.is_some());
        let methods = methods.unwrap();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name, "speak");

        // Fields should still be available
        assert!(reg.get_class_fields("Animal").is_some());
    }

    #[test]
    fn test_registry_get_class_fields_returns_none_for_non_class() {
        let mut reg = TypeRegistry::new();
        reg.register("Point".to_owned(), vec![]);
        assert!(reg.get_class_fields("Point").is_none());
    }
}
