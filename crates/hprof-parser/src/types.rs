//! Parsing for structural hprof records: `LOAD_CLASS`, `START_THREAD`,
//! `STACK_FRAME`, and `STACK_TRACE`.
//!
//! Each parser function accepts a `cursor` positioned immediately after the
//! 9-byte record header, plus `id_size` to handle 4- or 8-byte object IDs.

use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

use crate::{HprofError, read_id};

/// A parsed `LOAD_CLASS` record (tag `0x02`).
///
/// ## Fields
/// - `class_serial`: `u32` — class serial number
/// - `class_object_id`: `u64` — object ID of the class
/// - `stack_trace_serial`: `u32` — serial of associated stack trace
/// - `class_name_string_id`: `u64` — string ID of the class name
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub class_serial: u32,
    pub class_object_id: u64,
    pub stack_trace_serial: u32,
    pub class_name_string_id: u64,
}

/// A parsed `START_THREAD` record (tag `0x06`).
///
/// ## Fields
/// - `thread_serial`: `u32`
/// - `object_id`: `u64` — object ID of the thread
/// - `stack_trace_serial`: `u32`
/// - `name_string_id`: `u64` — string ID of thread name
/// - `group_name_string_id`: `u64` — string ID of thread group name
/// - `group_parent_name_string_id`: `u64` — string ID of parent group name
#[derive(Debug, Clone)]
pub struct HprofThread {
    pub thread_serial: u32,
    pub object_id: u64,
    pub stack_trace_serial: u32,
    pub name_string_id: u64,
    pub group_name_string_id: u64,
    pub group_parent_name_string_id: u64,
}

/// A parsed `STACK_FRAME` record (tag `0x04`).
///
/// ## Fields
/// - `frame_id`: `u64`
/// - `method_name_string_id`: `u64`
/// - `method_sig_string_id`: `u64`
/// - `source_file_string_id`: `u64`
/// - `class_serial`: `u32`
/// - `line_number`: `i32` — negative = unknown, 0 = compiled, >0 = source line
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub frame_id: u64,
    pub method_name_string_id: u64,
    pub method_sig_string_id: u64,
    pub source_file_string_id: u64,
    pub class_serial: u32,
    pub line_number: i32,
}

/// A parsed `STACK_TRACE` record (tag `0x05`).
///
/// ## Fields
/// - `stack_trace_serial`: `u32`
/// - `thread_serial`: `u32`
/// - `frame_ids`: ordered list of `u64` frame IDs
#[derive(Debug, Clone)]
pub struct StackTrace {
    pub stack_trace_serial: u32,
    pub thread_serial: u32,
    pub frame_ids: Vec<u64>,
}

/// Parses a `LOAD_CLASS` record payload from `cursor`.
///
/// Cursor must be positioned immediately after the 9-byte record header.
///
/// ## Errors
/// - [`HprofError::TruncatedRecord`] if the payload is shorter than expected
pub fn parse_load_class(cursor: &mut Cursor<&[u8]>, id_size: u32) -> Result<ClassDef, HprofError> {
    let class_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let class_object_id = read_id(cursor, id_size)?;
    let stack_trace_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let class_name_string_id = read_id(cursor, id_size)?;
    Ok(ClassDef {
        class_serial,
        class_object_id,
        stack_trace_serial,
        class_name_string_id,
    })
}

/// Parses a `START_THREAD` record payload from `cursor`.
///
/// Cursor must be positioned immediately after the 9-byte record header.
///
/// ## Errors
/// - [`HprofError::TruncatedRecord`] if the payload is shorter than expected
pub fn parse_start_thread(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
) -> Result<HprofThread, HprofError> {
    let thread_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let object_id = read_id(cursor, id_size)?;
    let stack_trace_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let name_string_id = read_id(cursor, id_size)?;
    let group_name_string_id = read_id(cursor, id_size)?;
    let group_parent_name_string_id = read_id(cursor, id_size)?;
    Ok(HprofThread {
        thread_serial,
        object_id,
        stack_trace_serial,
        name_string_id,
        group_name_string_id,
        group_parent_name_string_id,
    })
}

/// Parses a `STACK_FRAME` record payload from `cursor`.
///
/// Cursor must be positioned immediately after the 9-byte record header.
///
/// ## Errors
/// - [`HprofError::TruncatedRecord`] if the payload is shorter than expected
pub fn parse_stack_frame(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
) -> Result<StackFrame, HprofError> {
    let frame_id = read_id(cursor, id_size)?;
    let method_name_string_id = read_id(cursor, id_size)?;
    let method_sig_string_id = read_id(cursor, id_size)?;
    let source_file_string_id = read_id(cursor, id_size)?;
    let class_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let line_number = cursor
        .read_i32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    Ok(StackFrame {
        frame_id,
        method_name_string_id,
        method_sig_string_id,
        source_file_string_id,
        class_serial,
        line_number,
    })
}

/// Parses a `STACK_TRACE` record payload from `cursor`.
///
/// Cursor must be positioned immediately after the 9-byte record header.
///
/// ## Errors
/// - [`HprofError::TruncatedRecord`] if the payload is shorter than expected
pub fn parse_stack_trace(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
) -> Result<StackTrace, HprofError> {
    let stack_trace_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let thread_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let num_frames = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;

    let remaining = cursor
        .get_ref()
        .len()
        .saturating_sub(cursor.position() as usize);
    let required = (num_frames as usize)
        .checked_mul(id_size as usize)
        .ok_or_else(|| {
            HprofError::CorruptedData(format!(
                "stack trace frame list size overflow: num_frames={num_frames}, id_size={id_size}"
            ))
        })?;
    if required > remaining {
        return Err(HprofError::TruncatedRecord);
    }

    let mut frame_ids = Vec::new();
    for _ in 0..num_frames {
        frame_ids.push(read_id(cursor, id_size)?);
    }
    Ok(StackTrace {
        stack_trace_serial,
        thread_serial,
        frame_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // --- ClassDef / LOAD_CLASS ---

    #[test]
    fn parse_load_class_id_size_8() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // class_serial
        data.extend_from_slice(&100u64.to_be_bytes()); // class_object_id
        data.extend_from_slice(&0u32.to_be_bytes()); // stack_trace_serial
        data.extend_from_slice(&200u64.to_be_bytes()); // class_name_string_id
        let mut cursor = Cursor::new(data.as_slice());
        let def = parse_load_class(&mut cursor, 8).unwrap();
        assert_eq!(def.class_serial, 1);
        assert_eq!(def.class_object_id, 100);
        assert_eq!(def.stack_trace_serial, 0);
        assert_eq!(def.class_name_string_id, 200);
    }

    #[test]
    fn parse_load_class_id_size_4() {
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_be_bytes()); // class_serial
        data.extend_from_slice(&50u32.to_be_bytes()); // class_object_id (4 bytes)
        data.extend_from_slice(&3u32.to_be_bytes()); // stack_trace_serial
        data.extend_from_slice(&99u32.to_be_bytes()); // class_name_string_id (4 bytes)
        let mut cursor = Cursor::new(data.as_slice());
        let def = parse_load_class(&mut cursor, 4).unwrap();
        assert_eq!(def.class_serial, 2);
        assert_eq!(def.class_object_id, 50);
        assert_eq!(def.stack_trace_serial, 3);
        assert_eq!(def.class_name_string_id, 99);
    }

    #[test]
    fn parse_load_class_truncated() {
        // Only 4 bytes — not enough for full LOAD_CLASS record
        let data = 1u32.to_be_bytes().to_vec();
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_load_class(&mut cursor, 8).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    // --- HprofThread / START_THREAD ---

    #[test]
    fn parse_start_thread_id_size_8() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // thread_serial
        data.extend_from_slice(&100u64.to_be_bytes()); // object_id
        data.extend_from_slice(&0u32.to_be_bytes()); // stack_trace_serial
        data.extend_from_slice(&10u64.to_be_bytes()); // name_string_id
        data.extend_from_slice(&20u64.to_be_bytes()); // group_name_string_id
        data.extend_from_slice(&30u64.to_be_bytes()); // group_parent_name_string_id
        let mut cursor = Cursor::new(data.as_slice());
        let t = parse_start_thread(&mut cursor, 8).unwrap();
        assert_eq!(t.thread_serial, 1);
        assert_eq!(t.object_id, 100);
        assert_eq!(t.stack_trace_serial, 0);
        assert_eq!(t.name_string_id, 10);
        assert_eq!(t.group_name_string_id, 20);
        assert_eq!(t.group_parent_name_string_id, 30);
    }

    #[test]
    fn parse_start_thread_id_size_4() {
        let mut data = Vec::new();
        data.extend_from_slice(&5u32.to_be_bytes()); // thread_serial
        data.extend_from_slice(&200u32.to_be_bytes()); // object_id (4 bytes)
        data.extend_from_slice(&1u32.to_be_bytes()); // stack_trace_serial
        data.extend_from_slice(&11u32.to_be_bytes()); // name_string_id
        data.extend_from_slice(&22u32.to_be_bytes()); // group_name_string_id
        data.extend_from_slice(&33u32.to_be_bytes()); // group_parent_name_string_id
        let mut cursor = Cursor::new(data.as_slice());
        let t = parse_start_thread(&mut cursor, 4).unwrap();
        assert_eq!(t.thread_serial, 5);
        assert_eq!(t.object_id, 200);
        assert_eq!(t.stack_trace_serial, 1);
        assert_eq!(t.name_string_id, 11);
        assert_eq!(t.group_name_string_id, 22);
        assert_eq!(t.group_parent_name_string_id, 33);
    }

    #[test]
    fn parse_start_thread_truncated() {
        let data = vec![0u8; 4]; // only thread_serial bytes
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_start_thread(&mut cursor, 8).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    // --- StackFrame / STACK_FRAME ---

    #[test]
    fn parse_stack_frame_id_size_8() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes()); // frame_id
        data.extend_from_slice(&2u64.to_be_bytes()); // method_name_string_id
        data.extend_from_slice(&3u64.to_be_bytes()); // method_sig_string_id
        data.extend_from_slice(&4u64.to_be_bytes()); // source_file_string_id
        data.extend_from_slice(&5u32.to_be_bytes()); // class_serial
        data.extend_from_slice(&42i32.to_be_bytes()); // line_number (positive)
        let mut cursor = Cursor::new(data.as_slice());
        let f = parse_stack_frame(&mut cursor, 8).unwrap();
        assert_eq!(f.frame_id, 1);
        assert_eq!(f.method_name_string_id, 2);
        assert_eq!(f.method_sig_string_id, 3);
        assert_eq!(f.source_file_string_id, 4);
        assert_eq!(f.class_serial, 5);
        assert_eq!(f.line_number, 42);
    }

    #[test]
    fn parse_stack_frame_line_number_zero() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&2u64.to_be_bytes());
        data.extend_from_slice(&3u64.to_be_bytes());
        data.extend_from_slice(&4u64.to_be_bytes());
        data.extend_from_slice(&5u32.to_be_bytes());
        data.extend_from_slice(&0i32.to_be_bytes());
        let mut cursor = Cursor::new(data.as_slice());
        let f = parse_stack_frame(&mut cursor, 8).unwrap();
        assert_eq!(f.line_number, 0);
    }

    #[test]
    fn parse_stack_frame_line_number_negative() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&2u64.to_be_bytes());
        data.extend_from_slice(&3u64.to_be_bytes());
        data.extend_from_slice(&4u64.to_be_bytes());
        data.extend_from_slice(&5u32.to_be_bytes());
        data.extend_from_slice(&(-1i32).to_be_bytes());
        let mut cursor = Cursor::new(data.as_slice());
        let f = parse_stack_frame(&mut cursor, 8).unwrap();
        assert_eq!(f.line_number, -1);
    }

    #[test]
    fn parse_stack_frame_id_size_4() {
        let mut data = Vec::new();
        data.extend_from_slice(&10u32.to_be_bytes()); // frame_id
        data.extend_from_slice(&20u32.to_be_bytes()); // method_name_string_id
        data.extend_from_slice(&30u32.to_be_bytes()); // method_sig_string_id
        data.extend_from_slice(&40u32.to_be_bytes()); // source_file_string_id
        data.extend_from_slice(&7u32.to_be_bytes()); // class_serial
        data.extend_from_slice(&100i32.to_be_bytes()); // line_number
        let mut cursor = Cursor::new(data.as_slice());
        let f = parse_stack_frame(&mut cursor, 4).unwrap();
        assert_eq!(f.frame_id, 10);
        assert_eq!(f.method_name_string_id, 20);
        assert_eq!(f.method_sig_string_id, 30);
        assert_eq!(f.source_file_string_id, 40);
        assert_eq!(f.class_serial, 7);
        assert_eq!(f.line_number, 100);
    }

    #[test]
    fn parse_stack_frame_truncated() {
        let data = vec![0u8; 8]; // only one id worth of bytes
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_stack_frame(&mut cursor, 8).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    // --- StackTrace / STACK_TRACE ---

    #[test]
    fn parse_stack_trace_id_size_8_three_frames() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // stack_trace_serial
        data.extend_from_slice(&2u32.to_be_bytes()); // thread_serial
        data.extend_from_slice(&3u32.to_be_bytes()); // num_frames
        data.extend_from_slice(&10u64.to_be_bytes()); // frame_id[0]
        data.extend_from_slice(&20u64.to_be_bytes()); // frame_id[1]
        data.extend_from_slice(&30u64.to_be_bytes()); // frame_id[2]
        let mut cursor = Cursor::new(data.as_slice());
        let st = parse_stack_trace(&mut cursor, 8).unwrap();
        assert_eq!(st.stack_trace_serial, 1);
        assert_eq!(st.thread_serial, 2);
        assert_eq!(st.frame_ids, vec![10, 20, 30]);
    }

    #[test]
    fn parse_stack_trace_id_size_4() {
        let mut data = Vec::new();
        data.extend_from_slice(&5u32.to_be_bytes()); // stack_trace_serial
        data.extend_from_slice(&6u32.to_be_bytes()); // thread_serial
        data.extend_from_slice(&2u32.to_be_bytes()); // num_frames
        data.extend_from_slice(&100u32.to_be_bytes()); // frame_id[0]
        data.extend_from_slice(&200u32.to_be_bytes()); // frame_id[1]
        let mut cursor = Cursor::new(data.as_slice());
        let st = parse_stack_trace(&mut cursor, 4).unwrap();
        assert_eq!(st.stack_trace_serial, 5);
        assert_eq!(st.thread_serial, 6);
        assert_eq!(st.frame_ids, vec![100, 200]);
    }

    #[test]
    fn parse_stack_trace_zero_frames() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // stack_trace_serial
        data.extend_from_slice(&1u32.to_be_bytes()); // thread_serial
        data.extend_from_slice(&0u32.to_be_bytes()); // num_frames = 0
        let mut cursor = Cursor::new(data.as_slice());
        let st = parse_stack_trace(&mut cursor, 8).unwrap();
        assert!(st.frame_ids.is_empty());
    }

    #[test]
    fn parse_stack_trace_truncated_frames() {
        // claims 5 frames but only provides 2
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&5u32.to_be_bytes()); // num_frames = 5
        data.extend_from_slice(&10u64.to_be_bytes()); // frame_id[0]
        data.extend_from_slice(&20u64.to_be_bytes()); // frame_id[1]
        // only 2 frames present, 3 missing
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_stack_trace(&mut cursor, 8).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::record::parse_record_header;
    use crate::test_utils::{HprofTestBuilder, advance_past_header};
    use std::io::Cursor;

    #[test]
    fn round_trip_load_class() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class(1, 100, 0, 200)
            .build();
        let hdr_end = advance_past_header(&bytes);
        let mut cursor = Cursor::new(&bytes[hdr_end..]);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x02);
        let def = parse_load_class(&mut cursor, 8).unwrap();
        assert_eq!(def.class_serial, 1);
        assert_eq!(def.class_object_id, 100);
        assert_eq!(def.stack_trace_serial, 0);
        assert_eq!(def.class_name_string_id, 200);
    }

    #[test]
    fn round_trip_start_thread() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_thread(1, 100, 0, 10, 20, 30)
            .build();
        let hdr_end = advance_past_header(&bytes);
        let mut cursor = Cursor::new(&bytes[hdr_end..]);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x06);
        let t = parse_start_thread(&mut cursor, 8).unwrap();
        assert_eq!(t.thread_serial, 1);
        assert_eq!(t.object_id, 100);
        assert_eq!(t.stack_trace_serial, 0);
        assert_eq!(t.name_string_id, 10);
        assert_eq!(t.group_name_string_id, 20);
        assert_eq!(t.group_parent_name_string_id, 30);
    }

    #[test]
    fn round_trip_stack_frame() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(1, 2, 3, 4, 5, 42)
            .build();
        let hdr_end = advance_past_header(&bytes);
        let mut cursor = Cursor::new(&bytes[hdr_end..]);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x04);
        let f = parse_stack_frame(&mut cursor, 8).unwrap();
        assert_eq!(f.frame_id, 1);
        assert_eq!(f.method_name_string_id, 2);
        assert_eq!(f.method_sig_string_id, 3);
        assert_eq!(f.source_file_string_id, 4);
        assert_eq!(f.class_serial, 5);
        assert_eq!(f.line_number, 42);
    }

    #[test]
    fn round_trip_stack_trace() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_trace(10, 2, &[100, 200, 300])
            .build();
        let hdr_end = advance_past_header(&bytes);
        let mut cursor = Cursor::new(&bytes[hdr_end..]);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x05);
        let st = parse_stack_trace(&mut cursor, 8).unwrap();
        assert_eq!(st.stack_trace_serial, 10);
        assert_eq!(st.thread_serial, 2);
        assert_eq!(st.frame_ids, vec![100, 200, 300]);
    }
}
