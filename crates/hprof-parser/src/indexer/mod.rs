//! Indexing subsystem: builds in-memory lookup structures from a sequential
//! hprof file pass.
//!
//! - [`precise`] — [`precise::PreciseIndex`]: five `HashMap` collections for
//!   O(1) lookup of structural records.
//! - [`first_pass`] — [`first_pass::run_first_pass`]: single sequential scan
//!   that populates a [`precise::PreciseIndex`].

pub(crate) mod first_pass;
pub(crate) mod precise;
