# Story 3.3: Stack Frame & Local Variable Display

Status: done

## Story

As a user,
I want to view the stack frames of a selected thread and the local variables of a selected frame,
So that I can drill down to the exact execution context I need to inspect.

## Acceptance Criteria

1. **Given** a thread is selected and the user presses Enter
   **When** the stack frames view loads
   **Then** all stack frames are displayed with method name, class name, source file, and line
   number — all resolved from structural strings (FR11)

2. **Given** method names and class names in JVM internal format
   **When** displayed
   **Then** they are shown in human-readable Java format
   (e.g., `HashMap` not `Ljava/util/HashMap;`) (FR17)

3. **Given** a stack frame is selected
   **When** I press Enter
   **Then** the local variables (GC roots for that frame) are shown inline below the frame
   as an indented tree section — not in a separate panel (FR12)

4. **Given** local variables are displayed
   **When** a variable's object ID is 0
   **Then** it shows as `null` inline (FR16)

5. **Given** local variables are displayed
   **When** a variable holds a complex object (non-null object ID)
   **Then** it shows as `Object [expand →]` — type resolution deferred to Story 3.4 (FR13)

6. **Given** a frame with no GC roots in the heap dump
   **When** Enter is pressed on it
   **Then** the vars section shows `(no locals)` inline below the frame

7. **Given** the stack frames panel is focused
   **When** I press Escape
   **Then** focus returns to the thread list panel

## Tasks / Subtasks

### Task 1: Add `java_types.rs` to `hprof-parser` (AC: #2)

File: `crates/hprof-parser/src/java_types.rs`

- [x] **Red**: Write test — `jvm_to_java("Ljava/util/HashMap;")` returns `"HashMap"`
- [x] **Red**: Write test — `jvm_to_java("java/util/HashMap")` returns `"HashMap"`
- [x] **Red**: Write test — `jvm_to_java("[Ljava/lang/String;")` returns `"String[]"`
- [x] **Red**: Write test — `jvm_to_java("[[I")` returns `"int[][]"`
- [x] **Red**: Write test — primitives: `"I"` → `"int"`, `"J"` → `"long"`, `"Z"` → `"boolean"`,
  `"B"` → `"byte"`, `"C"` → `"char"`, `"D"` → `"double"`, `"F"` → `"float"`,
  `"S"` → `"short"`, `"V"` → `"void"`
- [x] **Red**: Write test — `jvm_to_java("")` returns `""`
- [x] **Red**: Write test — unknown/already-readable `"HashMap"` returns `"HashMap"`
- [x] **Green**: Create `crates/hprof-parser/src/java_types.rs`:

  ```rust
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
      binary
          .rsplit('/')
          .next()
          .unwrap_or(binary)
          .to_string()
  }
  ```

- [x] Add `pub(crate) mod java_types;` and `pub use java_types::jvm_to_java;` to
  `crates/hprof-parser/src/lib.rs`

### Task 2: Index `GC_ROOT_JAVA_FRAME` records (AC: #3, #4, #5, #6)

Files: `crates/hprof-parser/src/indexer/first_pass.rs`,
       `crates/hprof-parser/src/indexer/precise.rs`

Background: The `GC_ROOT_JAVA_FRAME` sub-record (heap dump sub-tag `0x03`) maps an object_id
to a thread_serial + frame_number (0-based index into the thread's stack trace frame list).
Currently, line ~488 in `first_pass.rs` skips it: `0x03 => skip_n(&mut cursor, id_size as
usize + 8)`. We must parse and correlate these roots to build a
`frame_id → Vec<u64>` (object_id) map.

Format of `GC_ROOT_JAVA_FRAME` payload:
- `object_id`: `id_size` bytes
- `thread_serial`: `u32` (4 bytes)
- `frame_number`: `i32` (4 bytes) — 0-based index, -1 = no frame info

- [x] **Red**: Write test — builder with `add_java_frame_root` produces indexable roots;
  after calling `run_first_pass`, the root at frame_number=0 of the thread's trace appears
  in `java_frame_roots` under the correct frame_id
- [x] **Red**: Write test — root with frame_number=-1 is NOT stored in `java_frame_roots`
- [x] **Red**: Write test — root with frame_number out of range for the trace is NOT stored
- [x] **Green**: Add `add_java_frame_root` to `HprofTestBuilder` in `test_utils.rs`:

  ```rust
  /// Appends a `HEAP_DUMP_SEGMENT` (tag `0x1C`) containing one
  /// `GC_ROOT_JAVA_FRAME` (sub-tag `0x03`) sub-record.
  ///
  /// Payload: `object_id(id_size)` + `thread_serial(u32)` + `frame_number(i32)`
  pub fn add_java_frame_root(
      mut self,
      object_id: u64,
      thread_serial: u32,
      frame_number: i32,
  ) -> Self {
      let mut sub = vec![0x03u8]; // GC_ROOT_JAVA_FRAME sub-tag
      sub.extend_from_slice(&self.encode_id(object_id));
      sub.extend_from_slice(&thread_serial.to_be_bytes());
      sub.extend_from_slice(&frame_number.to_be_bytes());
      self.records.push(Self::make_record(0x1C, &sub));
      self
  }
  ```

- [x] **Green**: Add to `PreciseIndex`:

  ```rust
  /// GC root object IDs keyed by frame ID. Populated during first pass by
  /// correlating `GC_ROOT_JAVA_FRAME` sub-records with `STACK_TRACE` records.
  ///
  /// Key: `frame_id` (u64) — Value: Vec of object IDs rooted at that frame.
  pub java_frame_roots: HashMap<u64, Vec<u64>>,
  ```

  Initialize to `HashMap::new()` in `PreciseIndex::new()`.

- [x] **Green**: Update `first_pass.rs` — parse `GC_ROOT_JAVA_FRAME` in the heap dump
  sub-record loop (currently skipped at `0x03`):

  ```rust
  // Intermediate collection for GC roots; correlated AFTER main loop.
  let mut raw_frame_roots: Vec<(u64, u32, i32)> = Vec::new();
  // ... in heap dump sub-record match:
  0x03 => {
      let object_id = read_id(&mut cursor, id_size)?; // or handle error gracefully
      let thread_serial = cursor.read_u32::<BigEndian>()?;
      let frame_number = cursor.read_i32::<BigEndian>()?;
      raw_frame_roots.push((object_id, thread_serial, frame_number));
  }
  ```

  After the main loop, correlate with stack traces:

  ```rust
  for (object_id, thread_serial, frame_number) in raw_frame_roots {
      if frame_number < 0 {
          continue; // no frame info
      }
      // find stack trace for this thread
      let Some(trace) = index.stack_traces
          .values()
          .find(|st| st.thread_serial == thread_serial)
      else { continue };
      let idx = frame_number as usize;
      let Some(&frame_id) = trace.frame_ids.get(idx) else { continue };
      index.java_frame_roots
          .entry(frame_id)
          .or_default()
          .push(object_id);
  }
  ```

  Note: `raw_frame_roots` must be declared before the main parse loop and populated inside the
  heap dump sub-record match. Errors reading the 9 bytes of `GC_ROOT_JAVA_FRAME` should be
  treated as non-fatal (add a warning and skip the segment remainder, consistent with the
  existing error strategy).

### Task 3: Populate `FrameInfo` and `VariableInfo` in engine trait (AC: #1, #2, #4, #5)

File: `crates/hprof-engine/src/engine.rs`

- [x] **Red**: Write compile test — `FrameInfo` has fields `frame_id`, `method_name`,
  `class_name`, `source_file`, `line`
- [x] **Red**: Write compile test — `VariableInfo` has fields `index`, `value`
- [x] **Red**: Write compile test — `VariableValue` has variants `Null` and `ObjectRef(u64)`
- [x] **Red**: Write compile test — `LineNumber` has variants `Line(u32)`, `NoInfo`,
  `Unknown`, `Compiled`, `Native`
- [x] **Green**: Replace stubs in `engine.rs`:

  ```rust
  /// Line number information for a stack frame.
  ///
  /// Encodes the `i32` line_number field from `STACK_FRAME` records:
  /// - `> 0` → `Line(n)` (actual source line)
  /// - `0` → `NoInfo` (no line information available)
  /// - `-1` → `Unknown`
  /// - `-2` → `Compiled` (optimised method)
  /// - `_ < -2` → `Native`
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum LineNumber {
      Line(u32),
      NoInfo,
      Unknown,
      Compiled,
      Native,
  }

  impl LineNumber {
      pub fn from_raw(n: i32) -> Self {
          match n {
              n if n > 0 => LineNumber::Line(n as u32),
              0 => LineNumber::NoInfo,
              -1 => LineNumber::Unknown,
              -2 => LineNumber::Compiled,
              _ => LineNumber::Native,
          }
      }
  }

  /// Display information for one stack frame.
  #[derive(Debug, Clone)]
  pub struct FrameInfo {
      /// Unique frame identifier from the `STACK_FRAME` record.
      pub frame_id: u64,
      /// Human-readable method name (resolved from structural strings).
      pub method_name: String,
      /// Human-readable class name (JVM binary name → Java simple name).
      pub class_name: String,
      /// Source file name, or empty string if the string ID resolved to nothing.
      pub source_file: String,
      /// Source line number.
      pub line: LineNumber,
  }

  /// The value of a local variable (GC root) for a stack frame.
  ///
  /// Type resolution (showing the actual class name) is deferred to Story 3.4.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum VariableValue {
      /// Null reference (object_id == 0).
      Null,
      /// Non-null object reference. Type resolved in Story 3.4.
      ObjectRef(u64),
  }

  /// One local variable entry for a stack frame.
  ///
  /// hprof `GC_ROOT_JAVA_FRAME` records carry object IDs but no variable names.
  /// Variables are numbered by their 0-based position in the root list.
  #[derive(Debug, Clone)]
  pub struct VariableInfo {
      /// 0-based index in the frame's root list (used as display label).
      pub index: usize,
      /// Resolved variable value.
      pub value: VariableValue,
  }
  ```

  Update `DummyEngine` in the test to construct `FrameInfo` and `VariableInfo` with the new
  fields (any valid values), so the compile test continues to pass.

### Task 4: Implement `get_stack_frames()` and `get_local_variables()` (AC: #1, #2, #3)

File: `crates/hprof-engine/src/engine_impl.rs`

- [x] **Red**: Write test — `get_stack_frames(thread_serial)` on a file with a thread,
  stack trace (serial=1), and one frame returns a `Vec<FrameInfo>` of length 1
- [x] **Red**: Write test — `FrameInfo.method_name` resolves from `method_name_string_id`
- [x] **Red**: Write test — `FrameInfo.class_name` is human-readable (e.g., `"HashMap"` not
  `"java/util/HashMap"`) — uses `jvm_to_java`
- [x] **Red**: Write test — `FrameInfo.line` encodes `line_number=42` as `LineNumber::Line(42)`
- [x] **Red**: Write test — `FrameInfo.line` encodes `line_number=0` as `LineNumber::NoInfo`
- [x] **Red**: Write test — `FrameInfo.line` encodes `line_number=-1` as `LineNumber::Unknown`
- [x] **Red**: Write test — `get_stack_frames` on unknown `thread_serial` returns empty vec
- [x] **Red**: Write test — `get_local_variables(frame_id)` with one non-null root returns
  `[VariableInfo { index: 0, value: VariableValue::ObjectRef(object_id) }]`
- [x] **Red**: Write test — `get_local_variables(frame_id)` with object_id=0 returns
  `[VariableInfo { index: 0, value: VariableValue::Null }]`
- [x] **Red**: Write test — `get_local_variables` on frame with no roots returns empty vec
- [x] **Green**: Implement `get_stack_frames`:

  ```rust
  fn get_stack_frames(&self, thread_serial: u32) -> Vec<FrameInfo> {
      // 1. thread → stack_trace_serial
      let Some(thread) = self.hfile.index.threads.get(&thread_serial) else {
          return vec![];
      };
      // 2. stack_trace_serial → frame_ids
      let Some(trace) = self.hfile.index.stack_traces.get(&thread.stack_trace_serial) else {
          return vec![];
      };
      // 3. Resolve each frame_id → FrameInfo
      trace
          .frame_ids
          .iter()
          .filter_map(|&fid| self.hfile.index.stack_frames.get(&fid))
          .map(|sf| {
              let method_name = self.resolve_name(sf.method_name_string_id);
              let class_name = self.hfile.index.classes
                  .get(&sf.class_serial)
                  .map(|c| {
                      let raw = self.resolve_name(c.class_name_string_id);
                      hprof_parser::jvm_to_java(&raw)
                  })
                  .unwrap_or_else(|| format!("<class:{}>", sf.class_serial));
              let source_file = self.resolve_name(sf.source_file_string_id);
              let source_file = if source_file.starts_with("<unknown:") {
                  String::new()
              } else {
                  source_file
              };
              FrameInfo {
                  frame_id: sf.frame_id,
                  method_name,
                  class_name,
                  source_file,
                  line: LineNumber::from_raw(sf.line_number),
              }
          })
          .collect()
  }
  ```

- [x] **Green**: Implement `get_local_variables`:

  ```rust
  fn get_local_variables(&self, frame_id: u64) -> Vec<VariableInfo> {
      let roots = self.hfile.index.java_frame_roots.get(&frame_id);
      match roots {
          None => vec![],
          Some(ids) => ids
              .iter()
              .enumerate()
              .map(|(idx, &object_id)| VariableInfo {
                  index: idx,
                  value: if object_id == 0 {
                      VariableValue::Null
                  } else {
                      VariableValue::ObjectRef(object_id)
                  },
              })
              .collect(),
      }
  }
  ```

- [x] Add `use hprof_engine::VariableValue;` where needed
- [x] Add imports in `engine_impl.rs`: `use crate::engine::{FrameInfo, LineNumber,
  VariableInfo, VariableValue};` and `use hprof_parser::jvm_to_java;`

### Task 5: Re-export new types from `hprof-engine` (AC: #1, #2)

File: `crates/hprof-engine/src/lib.rs`

- [x] **Green**: Add to re-exports:
  ```rust
  pub use engine::{FrameInfo, LineNumber, VariableInfo, VariableValue};
  ```

### Task 6: Create `StackState` — frame list + inline var tree state (AC: #3, #4, #5, #6)

File: `crates/hprof-tui/src/views/stack_view.rs` (full rewrite)

- [x] **Red**: Write test — `StackState::new(frames)` with 3 frames selects frame 0
- [x] **Red**: Write test — `move_down()` on 3-frame list (no expanded) moves to frame 1
- [x] **Red**: Write test — `move_up()` at frame 0 does nothing
- [x] **Red**: Write test — `toggle_expand(frame_id, vars)` when frame has vars shows them;
  `move_down()` from frame 0 (expanded, 2 vars) moves to var 0
- [x] **Red**: Write test — `move_down()` past last var of expanded frame moves to frame 1
- [x] **Red**: Write test — `toggle_expand` on already-expanded frame collapses it
- [x] **Red**: Write test — `selected_frame_id()` returns correct frame_id
- [x] **Red**: Write test — `StackState::new(vec![])` → `selected_frame_id()` returns `None`
- [x] **Green**: Replace `stack_view.rs` with:

  ```rust
  //! Stack frame panel: frame list with inline local variable tree.
  //!
  //! [`StackState`] manages frame selection and expand/collapse of local vars.
  //! [`StackView`] is a [`StatefulWidget`] rendering the current state.

  use std::collections::HashSet;

  use hprof_engine::{FrameInfo, LineNumber, VariableInfo, VariableValue};
  use ratatui::{
      buffer::Buffer,
      layout::Rect,
      style::Modifier,
      text::{Line, Span},
      widgets::{Block, BorderType, Borders, List, ListItem, ListState, StatefulWidget},
  };

  use crate::theme;

  /// Cursor position within the frame+var tree.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum StackCursor {
      NoFrames,
      OnFrame(usize),
      OnVar { frame_idx: usize, var_idx: usize },
  }

  /// State for the stack frame panel.
  pub struct StackState {
      frames: Vec<FrameInfo>,
      /// Vars per frame_id — populated on demand by `App` calling the engine.
      vars: std::collections::HashMap<u64, Vec<VariableInfo>>,
      expanded: HashSet<u64>,
      cursor: StackCursor,
      list_state: ListState,
  }

  impl StackState {
      /// Creates a new state for the given frames. Selects first frame.
      pub fn new(frames: Vec<FrameInfo>) -> Self {
          let cursor = if frames.is_empty() {
              StackCursor::NoFrames
          } else {
              StackCursor::OnFrame(0)
          };
          let mut list_state = ListState::default();
          if !frames.is_empty() {
              list_state.select(Some(0));
          }
          Self {
              frames,
              vars: std::collections::HashMap::new(),
              expanded: HashSet::new(),
              cursor,
              list_state,
          }
      }

      /// Returns the frame_id currently selected, if any.
      pub fn selected_frame_id(&self) -> Option<u64> {
          match self.cursor {
              StackCursor::NoFrames => None,
              StackCursor::OnFrame(fi) => self.frames.get(fi).map(|f| f.frame_id),
              StackCursor::OnVar { frame_idx, .. } => {
                  self.frames.get(frame_idx).map(|f| f.frame_id)
              }
          }
      }

      /// Loads vars for `frame_id` into internal cache and toggles expand/collapse.
      pub fn toggle_expand(&mut self, frame_id: u64, vars: Vec<VariableInfo>) {
          if self.expanded.contains(&frame_id) {
              self.expanded.remove(&frame_id);
          } else {
              self.vars.insert(frame_id, vars);
              self.expanded.insert(frame_id);
          }
          self.sync_list_state();
      }

      /// Returns whether `frame_id` is currently expanded.
      pub fn is_expanded(&self, frame_id: u64) -> bool {
          self.expanded.contains(&frame_id)
      }

      pub fn move_down(&mut self) {
          let flat = self.flat_items();
          if flat.is_empty() { return; }
          let current = self.flat_index();
          if let Some(next) = current.and_then(|i| if i + 1 < flat.len() { Some(i + 1) } else { None }) {
              self.cursor = flat[next].clone();
              self.list_state.select(Some(next));
          }
      }

      pub fn move_up(&mut self) {
          let flat = self.flat_items();
          if flat.is_empty() { return; }
          let current = self.flat_index();
          if let Some(prev) = current.and_then(|i| i.checked_sub(1)) {
              self.cursor = flat[prev].clone();
              self.list_state.select(Some(prev));
          }
      }

      /// Returns the flattened cursor index (position in the rendered list).
      fn flat_index(&self) -> Option<usize> {
          let flat = self.flat_items();
          flat.iter().position(|c| c == &self.cursor)
      }

      /// Flattened ordered list of cursors matching the rendered list items.
      fn flat_items(&self) -> Vec<StackCursor> {
          let mut out = Vec::new();
          for (fi, frame) in self.frames.iter().enumerate() {
              out.push(StackCursor::OnFrame(fi));
              if self.expanded.contains(&frame.frame_id) {
                  let empty = vec![];
                  let vars = self.vars.get(&frame.frame_id).unwrap_or(&empty);
                  for vi in 0..vars.len().max(1) {
                      // Include at least one slot for "(no locals)"
                      out.push(StackCursor::OnVar { frame_idx: fi, var_idx: vi });
                      if vars.is_empty() { break; }
                  }
              }
          }
          out
      }

      fn sync_list_state(&mut self) {
          let idx = self.flat_index();
          self.list_state.select(idx);
      }

      /// Builds the list items for rendering.
      pub fn build_items(&self) -> Vec<ListItem<'static>> {
          let mut items = Vec::new();
          for (fi, frame) in self.frames.iter().enumerate() {
              let line_label = match &frame.line {
                  LineNumber::Line(n) => format!(":{}", n),
                  LineNumber::NoInfo => String::new(),
                  LineNumber::Unknown => " (?)".to_string(),
                  LineNumber::Compiled => " (compiled)".to_string(),
                  LineNumber::Native => " (native)".to_string(),
              };
              let src = if frame.source_file.is_empty() {
                  String::new()
              } else {
                  format!(" [{}{}]", frame.source_file, line_label)
              };
              let text = format!(
                  "{}.{}(){}",
                  frame.class_name, frame.method_name, src
              );
              let is_selected = matches!(&self.cursor, StackCursor::OnFrame(i) if *i == fi)
                  || matches!(&self.cursor, StackCursor::OnVar { frame_idx, .. } if *frame_idx == fi);
              let style = if is_selected {
                  theme::SELECTED
              } else {
                  ratatui::style::Style::default()
              };
              items.push(ListItem::new(Line::from(Span::styled(text, style))));

              if self.expanded.contains(&frame.frame_id) {
                  let empty = vec![];
                  let vars = self.vars.get(&frame.frame_id).unwrap_or(&empty);
                  if vars.is_empty() {
                      let var_style = if matches!(&self.cursor,
                          StackCursor::OnVar { frame_idx, .. } if *frame_idx == fi) {
                          theme::SELECTED
                      } else {
                          theme::SEARCH_HINT
                      };
                      items.push(ListItem::new(
                          Line::from(Span::styled("  (no locals)", var_style))
                      ));
                  } else {
                      for (vi, var) in vars.iter().enumerate() {
                          let val_str = match &var.value {
                              VariableValue::Null => "null".to_string(),
                              VariableValue::ObjectRef(id) => {
                                  format!("Object [expand →] (0x{:x})", id)
                              }
                          };
                          let var_text = format!("  [{}] {}", var.index, val_str);
                          let var_selected = matches!(&self.cursor,
                              StackCursor::OnVar { frame_idx: ffi, var_idx: vvi }
                              if *ffi == fi && *vvi == vi);
                          let var_style = if var_selected {
                              theme::SELECTED
                          } else {
                              ratatui::style::Style::default()
                          };
                          items.push(ListItem::new(
                              Line::from(Span::styled(var_text, var_style))
                          ));
                      }
                  }
              }
          }
          if items.is_empty() {
              items.push(ListItem::new(
                  Line::from(Span::styled("(no frames)", theme::SEARCH_HINT))
              ));
          }
          items
      }
  }

  /// Stateful widget for the stack frame panel.
  pub struct StackView {
      pub focused: bool,
  }

  impl StatefulWidget for StackView {
      type State = StackState;

      fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
          let border_style = if self.focused {
              theme::BORDER_FOCUSED
          } else {
              theme::BORDER_UNFOCUSED
          };
          let block = Block::default()
              .borders(Borders::ALL)
              .border_type(BorderType::Plain)
              .border_style(border_style)
              .title("Stack Frames  [Enter] expand  [Esc] back");
          let inner = block.inner(area);
          block.render(area, buf);

          let items = state.build_items();
          let list = List::new(items).highlight_style(
              ratatui::style::Style::default().add_modifier(Modifier::BOLD)
          );
          StatefulWidget::render(list, inner, buf, &mut state.list_state);
      }
  }
  ```

### Task 7: Update `App` — load frames on Enter, handle stack panel navigation (AC: #1, #3, #7)

File: `crates/hprof-tui/src/app.rs`

- [x] **Red**: Write test — `handle_input(Enter)` in `Focus::ThreadList` loads frames and
  transitions to `Focus::StackFrames`, `app.stack_state` is `Some(_)` with correct frame count
- [x] **Red**: Write test — `handle_input(Up)` / `Down` in `Focus::StackFrames` moves cursor
  in `stack_state`
- [x] **Red**: Write test — `handle_input(Enter)` in `Focus::StackFrames` on an expanded frame
  collapses it; on a collapsed frame loads vars and expands it
- [x] **Red**: Write test — `handle_input(Escape)` in `Focus::StackFrames` clears
  `stack_state` and returns to `Focus::ThreadList`
- [x] **Green**: Add `stack_state: Option<StackState>` field to `App`
- [x] **Green**: Update `handle_thread_list_input` — on `InputEvent::Enter`:
  ```rust
  InputEvent::Enter => {
      if let Some(serial) = self.thread_list.selected_serial() {
          let frames = self.engine.get_stack_frames(serial);
          self.stack_state = Some(StackState::new(frames));
          self.focus = Focus::StackFrames;
      }
  }
  ```
- [x] **Green**: Implement `handle_stack_frames_input`:
  ```rust
  fn handle_stack_frames_input(&mut self, event: InputEvent) -> AppAction {
      match event {
          InputEvent::Escape => {
              self.stack_state = None;
              self.focus = Focus::ThreadList;
          }
          InputEvent::Up => {
              if let Some(s) = &mut self.stack_state { s.move_up(); }
          }
          InputEvent::Down => {
              if let Some(s) = &mut self.stack_state { s.move_down(); }
          }
          InputEvent::Enter => {
              if let Some(s) = &mut self.stack_state {
                  if let Some(frame_id) = s.selected_frame_id() {
                      if s.is_expanded(frame_id) {
                          s.toggle_expand(frame_id, vec![]);
                      } else {
                          let vars = self.engine.get_local_variables(frame_id);
                          s.toggle_expand(frame_id, vars);
                      }
                  }
              }
          }
          InputEvent::Quit => return AppAction::Quit,
          _ => {}
      }
      AppAction::Continue
  }
  ```

  Note: `toggle_expand` with `vec![]` when collapsing is acceptable — the method clears
  the expanded state. The cached vars remain in the HashMap for future re-expansion without
  re-querying the engine (optimization: do not reload if already cached).

  Revised approach — only load if not already cached:
  ```rust
  InputEvent::Enter => {
      if let Some(s) = &mut self.stack_state {
          if let Some(frame_id) = s.selected_frame_id() {
              if s.is_expanded(frame_id) {
                  s.toggle_expand(frame_id, vec![]); // collapse — vec ignored by toggle
              } else {
                  let vars = self.engine.get_local_variables(frame_id);
                  s.toggle_expand(frame_id, vars);
              }
          }
      }
  }
  ```

- [x] **Green**: Update `App::render` — replace old `StackView { selected_serial, focused }` with:
  ```rust
  // Stack view — use StackState if available, else create empty state
  let stack_focused = self.focus == Focus::StackFrames;
  if let Some(ref mut ss) = self.stack_state {
      frame.render_stateful_widget(StackView { focused: stack_focused }, stack_area, ss);
  } else {
      // No thread selected or not yet entered: show empty panel
      let mut empty_state = StackState::new(vec![]);
      frame.render_stateful_widget(
          StackView { focused: stack_focused }, stack_area, &mut empty_state
      );
  }
  ```
- [x] **Green**: Update `views/mod.rs` — add `stack_view::StackView` and `stack_view::StackState`
  to imports in `app.rs`. Update the `pub mod stack_view;` re-export.
- [x] **Green**: Update `hprof-tui/src/lib.rs` imports if needed (no public re-export of
  `StackState` needed — it's internal to app).

### Task 8: Update `input.rs` — add Enter/Up/Down handling in stack frames (AC: #3, #7)

No changes needed to `input.rs` — `InputEvent::Enter`, `Up`, `Down`, `Escape` already exist.
Update key hint in the status bar to reflect stack panel context if desired (optional, low priority).

### Task 9: Verify all checks pass

- [x] `cargo test -p hprof-parser` — all parser tests pass (java_types, GC root indexing,
  builder round-trips)
- [x] `cargo test -p hprof-engine` — all engine tests pass (get_stack_frames, get_local_variables)
- [x] `cargo test -p hprof-tui` — all TUI tests pass (StackState, App with StackFrames focus)
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace -- -D warnings`
- [x] `cargo fmt -- --check`
- [ ] Manual smoke test: `cargo run -- <some.hprof>` — thread list → Enter → frame list shows;
  Enter on frame → vars show inline; Escape → back to thread list

## Dev Notes

### GC_ROOT_JAVA_FRAME and Local Variable Limitations

hprof `GC_ROOT_JAVA_FRAME` (heap dump sub-tag `0x03`) only records object references tagged
to a thread+frame. There is **no variable name** and **no primitive values** in this record.
Primitives are simply absent from GC root tracking. In Story 3.3:
- Only object references appear as local variables
- Names are shown as `[0]`, `[1]`, `[2]` by index
- Types are shown as `"Object [expand →]"` — full type resolution (class name via instance dump)
  is deferred to Story 3.4

This is correct and expected per the hprof format spec.

### `GC_ROOT_JAVA_FRAME` Correlation Strategy

`frame_number` in the GC root record is a 0-based index into the thread's stack trace
`frame_ids` array. To map to a `frame_id`:
1. Find the `StackTrace` with `thread_serial` matching the root's `thread_serial`
2. `frame_ids[frame_number]` is the `frame_id`

Note: a thread has exactly one stack trace at the time of the heap dump. The `threads` map
stores `stack_trace_serial` which points to exactly one entry in `stack_traces`. If the stack
trace serial is 0 (sentinel for "no trace"), skip the root.

### Java Type Conversion Scope for Story 3.3

`jvm_to_java` only extracts the **simple name** (last `/`-delimited component). It does NOT:
- Preserve package names (e.g., `java.util.HashMap` — just `HashMap`)
- Convert method signatures with parameter types (method signature is not displayed)

This is intentional for display clarity (matching VisualVM's default stack view). Full qualified
names can be added later without changing the trait API.

### `StackState` Toggle Collapse Behavior

When `toggle_expand` is called with an empty vec on an already-expanded frame, the method
ignores the vec and just removes the frame from `expanded`. The cached vars remain in the
`vars` HashMap. On re-expansion, the `App` checks `is_expanded()` — if false, it re-queries
the engine and calls `toggle_expand` with fresh vars. This is acceptable since `get_local_variables`
is a cheap O(1) HashMap lookup once indexed.

A cleaner approach: pass `Option<Vec<VariableInfo>>` where `None` = collapse, `Some(vars)` =
expand. Either design is acceptable; keep it simple.

### `StackState` flat_items Design

The `flat_items()` method builds the linearized cursor list on every navigation call. This is
intentional simplicity (KISS) — frames are fixed at construction and vars are small.
No need to cache the flat list.

When a frame is expanded with 0 vars: one `OnVar` cursor is inserted to allow selection of
the "(no locals)" row. The `var_idx` is 0 but the frame's var vec is empty — `build_items`
handles this case specially to show `"(no locals)"`.

### Module Structure — Files Changed in This Story

```
crates/hprof-parser/src/
├── lib.rs                   # updated: add java_types module + jvm_to_java re-export
├── java_types.rs            # NEW: JVM name → Java name conversion
├── indexer/
│   ├── precise.rs           # updated: add java_frame_roots field
│   └── first_pass.rs        # updated: parse GC_ROOT_JAVA_FRAME, post-process correlation

crates/hprof-engine/src/
├── engine.rs                # updated: FrameInfo, VariableInfo, VariableValue, LineNumber
├── engine_impl.rs           # updated: implement get_stack_frames, get_local_variables
└── lib.rs                   # updated: re-export FrameInfo, VariableInfo, VariableValue,
                             #          LineNumber

crates/hprof-tui/src/
├── app.rs                   # updated: stack_state field, stack frame focus handling
├── views/
│   └── stack_view.rs        # REWRITTEN: StackState + StackView StatefulWidget
└── (views/mod.rs if re-exports needed)

crates/hprof-parser/src/test_utils.rs  # updated: add_java_frame_root method
```

### Previous Story Intelligence (3.2)

Key patterns established in 3.2 to follow:
- `StatefulWidget` pattern: widget struct holds config only, state is separate struct
- Encapsulate state mutation behind methods, keep fields private
- `list_state: ListState` inside state struct drives ratatui scroll offset
- `ThreadListState` is the model — `StackState` follows the same pattern
- Theme constants in `theme.rs` — no inline colors anywhere
- `App<E: NavigationEngine>` — engine is generic, no concrete type leakage
- `TerminalGuard` already handles cleanup — no changes needed there

### Git Intelligence

Recent commits show:
- Pattern: features added per-story in focused commits
- `EngineConfig` is a unit struct (use `EngineConfig` not `EngineConfig::default()` to avoid
  clippy lint — already fixed in 3.2 debug log)
- 190 tests passing after Story 3.2

### References

- [Source: docs/planning-artifacts/epics.md#Story 3.3]
- [Source: docs/planning-artifacts/architecture.md#Frontend Architecture]
- [Source: docs/planning-artifacts/architecture.md#Project Structure]
- [Source: crates/hprof-parser/src/types.rs — StackFrame, StackTrace, HprofThread]
- [Source: crates/hprof-parser/src/indexer/precise.rs — PreciseIndex fields]
- [Source: crates/hprof-parser/src/indexer/first_pass.rs — heap dump sub-record parsing loop]
- [Source: crates/hprof-engine/src/engine.rs — FrameInfo/VariableInfo stubs]
- [Source: crates/hprof-engine/src/engine_impl.rs — get_stack_frames stub]
- [Source: crates/hprof-tui/src/views/stack_view.rs — current stub]
- [Source: crates/hprof-tui/src/app.rs — App, Focus, handle_stack_frames_input stub]
- [Source: docs/implementation-artifacts/3-2-thread-list-and-search-in-tui.md — StatefulWidget
  pattern, TerminalGuard, App test structure]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

N/A — implementation proceeded without blockers.

### Completion Notes List

**Code Review Fixes (2026-03-07):**
- H1: Fixed `StackState::toggle_expand` — cursor now resets to `OnFrame` when collapsing
  from an `OnVar` position; added regression test covering navigation after collapse.
- H2: GC root correlation O(n→1): replaced linear `.values().find()` with O(1) lookup via
  `threads.get(thread_serial).stack_trace_serial → stack_traces.get(...)`.
- M1: Removed undocumented `(0x{id})` suffix from `Object [expand →]` display — matches AC #5.
- M2: Added test `toggle_expand_collapse_from_var_cursor_resets_to_frame_and_navigation_works`.
- M3: `move_down`/`move_up` now compute `flat_items()` once; cursor search inlined.

**Initial Implementation:**
- Implemented `jvm_to_java` in `hprof-parser/java_types.rs` — covers all JVM descriptor forms
  (arrays, object descriptors, binary names, primitives).
- Added `java_frame_roots: HashMap<u64, Vec<u64>>` to `PreciseIndex`; populated via post-loop
  correlation in `run_first_pass` after all stack traces are indexed.
- Replaced `FrameInfo`/`VariableInfo` stubs in `engine.rs` with full types (`LineNumber`,
  `VariableValue`). Added `from_raw(i32)` to `LineNumber` covering all sentinel values.
- Implemented `get_stack_frames` and `get_local_variables` on `Engine` in `engine_impl.rs`.
  Class name resolution uses `jvm_to_java`; source file is cleared when unresolved.
- Rewrote `stack_view.rs` as a `StatefulWidget` with `StackState` managing a flat cursor
  over frames + inline vars. Expand/collapse is toggled per frame_id.
- Updated `App` with `stack_state: Option<StackState>`, wired Enter/Up/Down/Esc/Quit in
  `handle_stack_frames_input`. Enter in ThreadList loads frames and transitions focus.
- 237 tests pass (152 parser, 33 engine, 47 TUI, 3 cli, 2 doctests). 0 clippy warnings.
- `extract_heap_object_ids` required `#[allow(clippy::too_many_arguments)]` due to the
  added `raw_frame_roots` parameter (7 args total, within reason for this internal function).

### File List

- `crates/hprof-parser/src/java_types.rs` — NEW
- `crates/hprof-parser/src/lib.rs` — updated: java_types module + jvm_to_java re-export
- `crates/hprof-parser/src/indexer/precise.rs` — updated: java_frame_roots field
- `crates/hprof-parser/src/indexer/first_pass.rs` — updated: parse GC_ROOT_JAVA_FRAME,
  post-loop correlation, raw_frame_roots collection
- `crates/hprof-parser/src/test_utils.rs` — updated: add_java_frame_root method
- `crates/hprof-engine/src/engine.rs` — updated: FrameInfo, VariableInfo, VariableValue,
  LineNumber full types replacing stubs
- `crates/hprof-engine/src/engine_impl.rs` — updated: get_stack_frames, get_local_variables
- `crates/hprof-engine/src/lib.rs` — updated: re-export LineNumber, VariableValue
- `crates/hprof-tui/src/views/stack_view.rs` — REWRITTEN: StackState + StackView StatefulWidget
- `crates/hprof-tui/src/app.rs` — updated: stack_state field, stack frame focus handling

### Code Review Fixes (AI) - Round 2 (2026-03-07)

- **M2 Fixed** — Frame line metadata no longer disappears when `source_file` is empty.
  Added dedicated formatter in `stack_view.rs` so labels like `(native)` and `(compiled)` are
  preserved even without a source filename.
- **M3 Fixed** — `GC_ROOT_JAVA_FRAME` truncation/corruption in heap sub-record parsing now emits
  an explicit warning instead of failing silently.
- Added tests:
  - `format_frame_label_keeps_line_metadata_when_source_file_missing`
  - `format_frame_label_with_source_file_and_line_number`
  - `truncated_gc_root_java_frame_sub_record_adds_warning`

## Senior Developer Review (AI)

### Review Date

2026-03-07

### Reviewer

Codex (Amelia / Dev Agent execution)

### Outcome

Approved after Round 2 fixes.

### Notes

- Story 3.3 frame/locals rendering now preserves line-state context in all source-file resolution
  cases.
- Tolerant parser behavior for `GC_ROOT_JAVA_FRAME` now reports non-fatal corruption as warnings,
  improving observability for degraded heap dumps.
- Workspace validation passed after fixes (`cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt -- --check`).

## Change Log

- 2026-03-07 — Applied Round 2 review fixes for Story 3.3:
  - Stack frame label rendering fix in `crates/hprof-tui/src/views/stack_view.rs`
  - `GC_ROOT_JAVA_FRAME` warning emission in `crates/hprof-parser/src/indexer/first_pass.rs`
  - Added regression tests for both behaviors.
