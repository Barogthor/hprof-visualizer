//! Enum types for hprof record tags and heap sub-tags.
//!
//! Replaces raw hex magic numbers with named variants for
//! compile-time exhaustive matching while remaining compatible
//! with tolerant parsing via `Unknown(u8)`.

use std::fmt;

/// Top-level hprof record tag, read from the 9-byte record header.
///
/// `Unknown(u8)` catches any unrecognised tag value, keeping
/// the enum infallible via `From<u8>`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RecordTag {
    StringInUtf8,
    LoadClass,
    StackFrame,
    StackTrace,
    StartThread,
    HeapDump,
    HeapDumpSegment,
    Unknown(u8),
}

impl From<u8> for RecordTag {
    fn from(v: u8) -> Self {
        match v {
            0x01 => Self::StringInUtf8,
            0x02 => Self::LoadClass,
            0x04 => Self::StackFrame,
            0x05 => Self::StackTrace,
            0x06 => Self::StartThread,
            0x0C => Self::HeapDump,
            0x1C => Self::HeapDumpSegment,
            other => Self::Unknown(other),
        }
    }
}

impl RecordTag {
    /// Returns the raw `u8` tag value.
    pub fn as_u8(&self) -> u8 {
        match self {
            Self::StringInUtf8 => 0x01,
            Self::LoadClass => 0x02,
            Self::StackFrame => 0x04,
            Self::StackTrace => 0x05,
            Self::StartThread => 0x06,
            Self::HeapDump => 0x0C,
            Self::HeapDumpSegment => 0x1C,
            Self::Unknown(v) => *v,
        }
    }
}

impl fmt::Display for RecordTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StringInUtf8 => {
                write!(f, "STRING_IN_UTF8(0x01)")
            }
            Self::LoadClass => write!(f, "LOAD_CLASS(0x02)"),
            Self::StackFrame => write!(f, "STACK_FRAME(0x04)"),
            Self::StackTrace => write!(f, "STACK_TRACE(0x05)"),
            Self::StartThread => {
                write!(f, "START_THREAD(0x06)")
            }
            Self::HeapDump => write!(f, "HEAP_DUMP(0x0C)"),
            Self::HeapDumpSegment => {
                write!(f, "HEAP_DUMP_SEGMENT(0x1C)")
            }
            Self::Unknown(v) => write!(f, "UNKNOWN(0x{v:02X})"),
        }
    }
}

/// Heap sub-tag identifying a sub-record inside a `HEAP_DUMP`
/// or `HEAP_DUMP_SEGMENT` payload.
///
/// Covers both GC root sub-tags (`0x01`–`0x09`) and heap object
/// sub-tags (`0x20`–`0x23`) because they share one `match` block
/// in production code.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HeapSubTag {
    GcRootUnknown,
    GcRootJniGlobal,
    GcRootJniLocal,
    GcRootJavaFrame,
    GcRootNativeStack,
    GcRootStickyClass,
    GcRootThreadBlock,
    GcRootMonitorUsed,
    GcRootThreadObj,
    GcRootInternedString,
    ClassDump,
    InstanceDump,
    ObjectArrayDump,
    PrimArrayDump,
    Unknown(u8),
}

impl From<u8> for HeapSubTag {
    fn from(v: u8) -> Self {
        match v {
            0x00 => Self::GcRootUnknown,
            0x01 => Self::GcRootJniGlobal,
            0x02 => Self::GcRootJniLocal,
            0x03 => Self::GcRootJavaFrame,
            0x04 => Self::GcRootNativeStack,
            0x05 => Self::GcRootStickyClass,
            0x06 => Self::GcRootThreadBlock,
            0x07 => Self::GcRootMonitorUsed,
            0x08 => Self::GcRootThreadObj,
            0x09 => Self::GcRootInternedString,
            0x20 => Self::ClassDump,
            0x21 => Self::InstanceDump,
            0x22 => Self::ObjectArrayDump,
            0x23 => Self::PrimArrayDump,
            other => Self::Unknown(other),
        }
    }
}

impl HeapSubTag {
    /// Returns the raw `u8` sub-tag value.
    pub fn as_u8(&self) -> u8 {
        match self {
            Self::GcRootUnknown => 0x00,
            Self::GcRootJniGlobal => 0x01,
            Self::GcRootJniLocal => 0x02,
            Self::GcRootJavaFrame => 0x03,
            Self::GcRootNativeStack => 0x04,
            Self::GcRootStickyClass => 0x05,
            Self::GcRootThreadBlock => 0x06,
            Self::GcRootMonitorUsed => 0x07,
            Self::GcRootThreadObj => 0x08,
            Self::GcRootInternedString => 0x09,
            Self::ClassDump => 0x20,
            Self::InstanceDump => 0x21,
            Self::ObjectArrayDump => 0x22,
            Self::PrimArrayDump => 0x23,
            Self::Unknown(v) => *v,
        }
    }
}

impl fmt::Display for HeapSubTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GcRootUnknown => {
                write!(f, "GC_ROOT_UNKNOWN(0x00)")
            }
            Self::GcRootJniGlobal => {
                write!(f, "GC_ROOT_JNI_GLOBAL(0x01)")
            }
            Self::GcRootJniLocal => {
                write!(f, "GC_ROOT_JNI_LOCAL(0x02)")
            }
            Self::GcRootJavaFrame => {
                write!(f, "GC_ROOT_JAVA_FRAME(0x03)")
            }
            Self::GcRootNativeStack => {
                write!(f, "GC_ROOT_NATIVE_STACK(0x04)")
            }
            Self::GcRootStickyClass => {
                write!(f, "GC_ROOT_STICKY_CLASS(0x05)")
            }
            Self::GcRootThreadBlock => {
                write!(f, "GC_ROOT_THREAD_BLOCK(0x06)")
            }
            Self::GcRootMonitorUsed => {
                write!(f, "GC_ROOT_MONITOR_USED(0x07)")
            }
            Self::GcRootThreadObj => {
                write!(f, "GC_ROOT_THREAD_OBJ(0x08)")
            }
            Self::GcRootInternedString => {
                write!(f, "GC_ROOT_INTERNED_STRING(0x09)")
            }
            Self::ClassDump => write!(f, "CLASS_DUMP(0x20)"),
            Self::InstanceDump => {
                write!(f, "INSTANCE_DUMP(0x21)")
            }
            Self::ObjectArrayDump => {
                write!(f, "OBJECT_ARRAY_DUMP(0x22)")
            }
            Self::PrimArrayDump => {
                write!(f, "PRIM_ARRAY_DUMP(0x23)")
            }
            Self::Unknown(v) => {
                write!(f, "UNKNOWN(0x{v:02X})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- RecordTag ---

    #[test]
    fn record_tag_round_trip_known_variants() {
        let cases: &[(u8, RecordTag)] = &[
            (0x01, RecordTag::StringInUtf8),
            (0x02, RecordTag::LoadClass),
            (0x04, RecordTag::StackFrame),
            (0x05, RecordTag::StackTrace),
            (0x06, RecordTag::StartThread),
            (0x0C, RecordTag::HeapDump),
            (0x1C, RecordTag::HeapDumpSegment),
        ];
        for &(byte, expected) in cases {
            let tag = RecordTag::from(byte);
            assert_eq!(tag, expected);
            assert_eq!(tag.as_u8(), byte);
        }
    }

    #[test]
    fn record_tag_unknown_round_trip() {
        let tag = RecordTag::from(0xFF);
        assert_eq!(tag, RecordTag::Unknown(0xFF));
        assert_eq!(tag.as_u8(), 0xFF);
    }

    #[test]
    fn record_tag_heap_dump_end_maps_to_unknown() {
        let tag = RecordTag::from(0x2C);
        assert_eq!(tag, RecordTag::Unknown(0x2C));
    }

    #[test]
    fn record_tag_display_known() {
        assert_eq!(RecordTag::StringInUtf8.to_string(), "STRING_IN_UTF8(0x01)");
        assert_eq!(RecordTag::LoadClass.to_string(), "LOAD_CLASS(0x02)");
        assert_eq!(RecordTag::StackFrame.to_string(), "STACK_FRAME(0x04)");
        assert_eq!(RecordTag::StackTrace.to_string(), "STACK_TRACE(0x05)");
        assert_eq!(RecordTag::StartThread.to_string(), "START_THREAD(0x06)");
        assert_eq!(RecordTag::HeapDump.to_string(), "HEAP_DUMP(0x0C)");
        assert_eq!(
            RecordTag::HeapDumpSegment.to_string(),
            "HEAP_DUMP_SEGMENT(0x1C)"
        );
    }

    #[test]
    fn record_tag_display_unknown() {
        assert_eq!(RecordTag::Unknown(0x2C).to_string(), "UNKNOWN(0x2C)");
        assert_eq!(RecordTag::Unknown(0xFF).to_string(), "UNKNOWN(0xFF)");
    }

    // --- HeapSubTag ---

    #[test]
    fn heap_sub_tag_round_trip_known_variants() {
        let cases: &[(u8, HeapSubTag)] = &[
            (0x00, HeapSubTag::GcRootUnknown),
            (0x01, HeapSubTag::GcRootJniGlobal),
            (0x02, HeapSubTag::GcRootJniLocal),
            (0x03, HeapSubTag::GcRootJavaFrame),
            (0x04, HeapSubTag::GcRootNativeStack),
            (0x05, HeapSubTag::GcRootStickyClass),
            (0x06, HeapSubTag::GcRootThreadBlock),
            (0x07, HeapSubTag::GcRootMonitorUsed),
            (0x08, HeapSubTag::GcRootThreadObj),
            (0x09, HeapSubTag::GcRootInternedString),
            (0x20, HeapSubTag::ClassDump),
            (0x21, HeapSubTag::InstanceDump),
            (0x22, HeapSubTag::ObjectArrayDump),
            (0x23, HeapSubTag::PrimArrayDump),
        ];
        for &(byte, expected) in cases {
            let tag = HeapSubTag::from(byte);
            assert_eq!(tag, expected);
            assert_eq!(tag.as_u8(), byte);
        }
    }

    #[test]
    fn heap_sub_tag_unknown_round_trip() {
        let tag = HeapSubTag::from(0xFF);
        assert_eq!(tag, HeapSubTag::Unknown(0xFF));
        assert_eq!(tag.as_u8(), 0xFF);
    }

    #[test]
    fn heap_sub_tag_display_instance_dump() {
        assert_eq!(HeapSubTag::InstanceDump.to_string(), "INSTANCE_DUMP(0x21)");
    }

    #[test]
    fn heap_sub_tag_display_unknown() {
        assert_eq!(HeapSubTag::Unknown(0xFF).to_string(), "UNKNOWN(0xFF)");
    }
}
