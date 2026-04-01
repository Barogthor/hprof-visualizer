//! JVM internal type name → human-readable Java name conversion,
//! plus named constants for hprof primitive type codes.
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

/// Hprof type code for an object reference field.
pub const PRIM_TYPE_OBJECT_REF: u8 = 2;
/// Hprof type code for `boolean`.
pub const PRIM_TYPE_BOOLEAN: u8 = 4;
/// Hprof type code for `char`.
pub const PRIM_TYPE_CHAR: u8 = 5;
/// Hprof type code for `float`.
pub const PRIM_TYPE_FLOAT: u8 = 6;
/// Hprof type code for `double`.
pub const PRIM_TYPE_DOUBLE: u8 = 7;
/// Hprof type code for `byte`.
pub const PRIM_TYPE_BYTE: u8 = 8;
/// Hprof type code for `short`.
pub const PRIM_TYPE_SHORT: u8 = 9;
/// Hprof type code for `int`.
pub const PRIM_TYPE_INT: u8 = 10;
/// Hprof type code for `long`.
pub const PRIM_TYPE_LONG: u8 = 11;

/// Returns the byte size of a value with the given
/// hprof type code.
pub(crate) fn value_byte_size(type_code: u8, id_size: crate::format::IdSize) -> usize {
    match type_code {
        PRIM_TYPE_OBJECT_REF => id_size.as_usize(),
        PRIM_TYPE_BOOLEAN | PRIM_TYPE_BYTE => 1,
        PRIM_TYPE_CHAR | PRIM_TYPE_SHORT => 2,
        PRIM_TYPE_FLOAT | PRIM_TYPE_INT => 4,
        PRIM_TYPE_DOUBLE | PRIM_TYPE_LONG => 8,
        _ => 0,
    }
}

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

    #[test]
    fn prim_type_constants_have_expected_values() {
        assert_eq!(PRIM_TYPE_OBJECT_REF, 2);
        assert_eq!(PRIM_TYPE_BOOLEAN, 4);
        assert_eq!(PRIM_TYPE_CHAR, 5);
        assert_eq!(PRIM_TYPE_FLOAT, 6);
        assert_eq!(PRIM_TYPE_DOUBLE, 7);
        assert_eq!(PRIM_TYPE_BYTE, 8);
        assert_eq!(PRIM_TYPE_SHORT, 9);
        assert_eq!(PRIM_TYPE_INT, 10);
        assert_eq!(PRIM_TYPE_LONG, 11);
    }
}
