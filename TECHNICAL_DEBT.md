# Technical Debt & Refactoring Opportunities

## 1. localStorage as IPC Mechanism (High Priority)

### Current Issue
The frontend uses `localStorage` as an inter-process communication bridge between async callbacks and the main UI loop. This is a workaround for WASM/egui constraints where `spawn_local` async blocks cannot capture `&mut self`.

### Pattern Used Throughout Codebase
```rust
// In async callback - can't access &mut self
fn load_data(&mut self) {
    spawn_local(async move {
        let data = api.fetch().await;
        // Store as "message" in localStorage
        storage.set_item("strom_data_key", &json);
    });
}

// In update() loop - acts as message pump
fn update(&mut self) {
    if let Ok(Some(json)) = storage.get_item("strom_data_key") {
        self.state.store(data);  // Now we have &mut self
        storage.remove_item("strom_data_key");
    }
}
```

### Locations Using This Pattern
- `load_flows()` → `strom_flows_data`
- `load_elements()` → `strom_elements_data`
- `load_blocks()` → `strom_blocks_data`
- `load_element_properties()` → `strom_element_properties_{name}`
- `load_element_pad_properties()` → `strom_element_pad_properties_{name}`
- `setup_sse_connection()` → Various event keys
- `save_current_flow()` → `strom_needs_refresh`
- SDP fetching → `strom_sdp_{flow_id}_{block_id}`

### Problems
1. **Conceptual confusion**: localStorage implies persistent storage, not IPC
2. **Type safety**: Requires serialization/deserialization on every pass
3. **Debugging difficulty**: State changes hidden in browser storage
4. **Browser pollution**: Fills localStorage with transient data
5. **Error-prone**: Easy to forget to clear keys, leading to stale data

### Proposed Solutions

#### Option 1: Standard Library Channels (Recommended)
```rust
use std::sync::mpsc::{channel, Receiver, Sender};

struct StromApp {
    flows_rx: Receiver<Vec<Flow>>,
    elements_rx: Receiver<Vec<ElementInfo>>,
    pad_properties_rx: Receiver<ElementInfo>,
    // ... one receiver per data type
}

impl StromApp {
    fn new() -> Self {
        let (flows_tx, flows_rx) = channel();
        let (elements_tx, elements_rx) = channel();
        // ...
        Self { flows_rx, elements_rx, /* ... */ }
    }

    fn load_flows(&self, ctx: &Context) {
        let tx = self.flows_tx.clone();
        spawn_local(async move {
            if let Ok(flows) = api.list_flows().await {
                let _ = tx.send(flows);  // Send through channel
            }
        });
    }

    fn update(&mut self, ctx: &Context) {
        // Check all channels
        while let Ok(flows) = self.flows_rx.try_recv() {
            self.flows = flows;
        }
        while let Ok(element_info) = self.pad_properties_rx.try_recv() {
            self.palette.cache_element_pad_properties(element_info);
        }
        // ...
    }
}
```

**Pros:**
- Type-safe
- Clear data flow
- No browser pollution
- Better debugging
- Standard Rust pattern

**Cons:**
- Requires refactoring all async operations
- Need one channel per data type (or use enum wrapper)

#### Option 2: Arc<Mutex<Vec<T>>> Shared State
```rust
struct StromApp {
    pending_pad_properties: Arc<Mutex<Vec<ElementInfo>>>,
}

fn load_pad_properties(&self) {
    let pending = self.pending_pad_properties.clone();
    spawn_local(async move {
        let data = api.fetch().await;
        pending.lock().unwrap().push(data);
    });
}

fn update(&mut self) {
    let mut pending = self.pending_pad_properties.lock().unwrap();
    for element_info in pending.drain(..) {
        self.palette.cache_element_pad_properties(element_info);
    }
}
```

**Pros:**
- Simple shared state
- Works well with WASM

**Cons:**
- Mutex overhead (though minimal in WASM single-threaded context)
- Slightly more complex lifetime management

#### Option 3: Async Channel (flume crate)
```rust
use flume::{Receiver, Sender};

struct StromApp {
    pad_properties_rx: Receiver<ElementInfo>,
}
```

**Pros:**
- Better async integration
- No blocking

**Cons:**
- Additional dependency
- Overkill for single-threaded WASM

### Refactoring Scope
- **Files to modify:**
  - `frontend/src/app.rs` (main file, ~1200 lines)
  - `frontend/src/sse.rs` (SSE event handling)

- **Functions to refactor:** ~8-10 async operations

- **Estimated effort:** 2-4 hours

- **Testing required:**
  - All async data loading paths
  - SSE event handling
  - Property lazy-loading
  - Flow state updates

### Recommendation
Use **Option 1 (Standard Library Channels)** because:
1. No additional dependencies
2. Type-safe and clear
3. Standard Rust pattern
4. Works perfectly in WASM single-threaded context
5. Easy to understand and maintain

### Implementation Plan (When Prioritized)
1. Add channel infrastructure to `StromApp` struct
2. Refactor one async operation as proof-of-concept (e.g., `load_elements`)
3. Verify it works, then refactor remaining operations
4. Remove all localStorage IPC code
5. Add clear comments about the pattern for future developers
6. Test thoroughly in browser

---

## Other Technical Debt Items

### 2. Error Handling Patterns (Medium Priority)
Currently mixing `Option`, `Result`, and localStorage error flags. Could standardize on a unified error handling approach.

### 3. Property Update API (Low Priority)
The property update mechanism sends individual property changes. Could batch updates for better performance.

### 4. Code Organization (Low Priority)
`app.rs` is becoming large (~1200 lines). Could split into modules:
- `app/state.rs` - Application state
- `app/handlers.rs` - Event handlers
- `app/render.rs` - Rendering functions
- `app/async_ops.rs` - Async operations

---

*Document created: 2025-01-17*
*Last updated: 2025-01-17*
