//! JVM internal type name → human-readable Java name conversion.
//!
//! Used to convert class names from `LOAD_CLASS` records and type descriptors
//! from `STACK_FRAME` records into the format Java developers expect.
//!
//! ## Rules
//! - Descriptor `Lsome/pkg/Class;` → `Class` (strip `L`, `;`, take last `/` component)
//! - Binary name `some/pkg/Class` → `Class` (take last `/` component)
//! - Array `[descriptor` → `inner[]`, recursively (e.g. `[[I` → `int[][]`)
//! - Primitive descriptor single char → primitive keyword
//! - Already-simple names are returned as-is

/// Converts a JVM type descriptor or binary class name to a human-readable
/// Java simple name.
///
/// See module docs for conversion rules.
pub fn jvm_to_java(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    // Array: strip leading '[', recurse, append "[]"
    if let Some(inner) = name.strip_prefix('[') {
        return format!("{}[]", jvm_to_java(inner));
    }
    // Object descriptor: Lsome/pkg/Class;
    if let Some(stripped) = name.strip_prefix('L').and_then(|s| s.strip_suffix(';')) {
        return simple_name(stripped);
    }
    // Primitive descriptors
    if name.len() == 1 {
        return match name.chars().next().unwrap() {
            'I' => "int",
            'J' => "long",
            'Z' => "boolean",
            'B' => "byte",
            'C' => "char",
            'D' => "double",
            'F' => "float",
            'S' => "short",
            'V' => "void",
            _ => name,
        }
        .to_string();
    }
    // Binary name: some/pkg/Class (no L prefix, no ; suffix)
    simple_name(name)
}

fn simple_name(binary: &str) -> String {
    binary.rsplit('/').next().unwrap_or(binary).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_descriptor_with_package() {
        assert_eq!(jvm_to_java("Ljava/util/HashMap;"), "HashMap");
    }

    #[test]
    fn binary_name_with_package() {
        assert_eq!(jvm_to_java("java/util/HashMap"), "HashMap");
    }

    #[test]
    fn array_of_object() {
        assert_eq!(jvm_to_java("[Ljava/lang/String;"), "String[]");
    }

    #[test]
    fn multidimensional_primitive_array() {
        assert_eq!(jvm_to_java("[[I"), "int[][]");
    }

    #[test]
    fn primitives() {
        assert_eq!(jvm_to_java("I"), "int");
        assert_eq!(jvm_to_java("J"), "long");
        assert_eq!(jvm_to_java("Z"), "boolean");
        assert_eq!(jvm_to_java("B"), "byte");
        assert_eq!(jvm_to_java("C"), "char");
        assert_eq!(jvm_to_java("D"), "double");
        assert_eq!(jvm_to_java("F"), "float");
        assert_eq!(jvm_to_java("S"), "short");
        assert_eq!(jvm_to_java("V"), "void");
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(jvm_to_java(""), "");
    }

    #[test]
    fn already_simple_name_returned_as_is() {
        assert_eq!(jvm_to_java("HashMap"), "HashMap");
    }
}
