//! Typed visitor pattern for heap sub-record traversal.
//!
//! [`HeapVisitor`] provides typed callbacks for each heap
//! sub-record kind, letting consumers iterate the heap
//! without knowing about sub-tag encoding or skip logic.
//!
//! Default implementations are no-ops returning
//! [`ControlFlow::Continue`], so consumers only override
//! the callbacks they need.

use std::ops::ControlFlow;

use crate::ClassDumpInfo;

/// Visitor trait for heap sub-record traversal.
///
/// Implement only the callbacks you care about.
/// Return [`ControlFlow::Break`] to stop the walk early.
///
/// # Example
///
/// ```ignore
/// struct InstanceCounter { count: usize }
///
/// impl HeapVisitor for InstanceCounter {
///     fn on_instance(
///         &mut self, _id: u64, _class_id: u64,
///         _data: &[u8],
///     ) -> ControlFlow<()> {
///         self.count += 1;
///         ControlFlow::Continue(())
///     }
/// }
/// ```
pub trait HeapVisitor {
    /// Called for each `INSTANCE_DUMP` sub-record.
    ///
    /// - `id`: object ID
    /// - `class_id`: class object ID
    /// - `data`: raw instance field bytes
    fn on_instance(&mut self, _id: u64, _class_id: u64, _data: &[u8]) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    /// Called for each `OBJECT_ARRAY_DUMP` sub-record.
    ///
    /// - `id`: array object ID
    /// - `class_id`: array element class ID
    /// - `num_elements`: number of elements
    /// - `elements_data`: raw element ID bytes
    ///   (each element is `id_size` bytes, big-endian)
    /// - `id_size`: bytes per object ID (4 or 8)
    fn on_object_array(
        &mut self,
        _id: u64,
        _class_id: u64,
        _num_elements: u32,
        _elements_data: &[u8],
        _id_size: u32,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    /// Called for each `PRIMITIVE_ARRAY_DUMP` sub-record.
    ///
    /// - `id`: array object ID
    /// - `element_type`: hprof primitive type code
    ///   (e.g. 5=char, 8=byte)
    /// - `num_elements`: number of elements
    /// - `data`: raw element bytes
    fn on_prim_array(
        &mut self,
        _id: u64,
        _element_type: u8,
        _num_elements: u32,
        _data: &[u8],
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    /// Called for each `CLASS_DUMP` sub-record.
    ///
    /// - `info`: fully parsed class dump metadata
    fn on_class_dump(&mut self, _info: &ClassDumpInfo) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
}
