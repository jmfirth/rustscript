//! External function signature information from rustdoc JSON.
//!
//! Used to bridge rustdoc data from rsc-driver into rsc-lower
//! for call-site analysis of external crate functions.

/// Information about an external function's signature.
#[derive(Debug, Clone)]
pub struct ExternalFnInfo {
    /// Function or method name.
    pub name: String,
    /// Crate this function belongs to.
    pub crate_name: String,
    /// Parameter information.
    pub params: Vec<ExternalParamInfo>,
    /// Return type classification.
    pub return_type: ExternalReturnType,
    /// Whether this is an async function.
    pub is_async: bool,
    /// Whether this is an instance method (has self param).
    pub is_method: bool,
    /// Parent type name if this is an impl method (e.g., "Router").
    pub parent_type: Option<String>,
}

/// Parameter type classification for external functions.
#[derive(Debug, Clone)]
pub struct ExternalParamInfo {
    /// Parameter name.
    pub name: String,
    /// Whether the parameter is a shared reference (&T).
    pub is_ref: bool,
    /// Whether the parameter is specifically &str.
    pub is_str_ref: bool,
    /// Whether the parameter is a mutable reference (&mut T).
    pub is_mut_ref: bool,
}

/// Return type classification for external functions.
#[derive(Debug, Clone)]
pub enum ExternalReturnType {
    /// Returns nothing (unit type).
    Unit,
    /// Returns a plain value.
    Value,
    /// Returns Result<T, E> (a "throws" function).
    Result,
    /// Returns Option<T>.
    Option,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_external_fn_info_construction() {
        let info = ExternalFnInfo {
            name: "route".to_owned(),
            crate_name: "axum".to_owned(),
            params: vec![
                ExternalParamInfo {
                    name: "path".to_owned(),
                    is_ref: true,
                    is_str_ref: true,
                    is_mut_ref: false,
                },
                ExternalParamInfo {
                    name: "handler".to_owned(),
                    is_ref: false,
                    is_str_ref: false,
                    is_mut_ref: false,
                },
            ],
            return_type: ExternalReturnType::Value,
            is_async: false,
            is_method: true,
            parent_type: Some("Router".to_owned()),
        };

        assert_eq!(info.name, "route");
        assert_eq!(info.crate_name, "axum");
        assert_eq!(info.params.len(), 2);
        assert!(info.params[0].is_str_ref);
        assert!(!info.params[1].is_ref);
        assert!(matches!(info.return_type, ExternalReturnType::Value));
        assert!(info.is_method);
        assert_eq!(info.parent_type.as_deref(), Some("Router"));
    }

    #[test]
    fn test_external_fn_info_async_function() {
        let info = ExternalFnInfo {
            name: "serve".to_owned(),
            crate_name: "axum".to_owned(),
            params: vec![],
            return_type: ExternalReturnType::Result,
            is_async: true,
            is_method: false,
            parent_type: None,
        };

        assert!(info.is_async);
        assert!(matches!(info.return_type, ExternalReturnType::Result));
        assert!(info.parent_type.is_none());
    }

    #[test]
    fn test_external_return_type_variants() {
        let unit = ExternalReturnType::Unit;
        let value = ExternalReturnType::Value;
        let result = ExternalReturnType::Result;
        let option = ExternalReturnType::Option;

        assert!(matches!(unit, ExternalReturnType::Unit));
        assert!(matches!(value, ExternalReturnType::Value));
        assert!(matches!(result, ExternalReturnType::Result));
        assert!(matches!(option, ExternalReturnType::Option));
    }
}
