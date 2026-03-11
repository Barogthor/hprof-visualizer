//! Stack frame panel: frame list with inline local variable tree.
//!
//! [`StackState`] manages frame selection and expand/collapse of local vars.
//! [`StackView`] is a [`StatefulWidget`] rendering the current state.

mod format;
mod state;
mod types;
mod widget;

pub use state::StackState;
pub use types::{ChunkState, CollectionChunks, ExpansionPhase, StackCursor};
pub use widget::StackView;

pub(crate) use format::{
    compute_chunk_ranges, field_value_style, format_entry_value_text,
    format_field_value_display, format_frame_label, format_object_ref_collapsed,
};
pub(crate) use types::FAILED_LABEL_SEP;

#[cfg(test)]
mod tests;
