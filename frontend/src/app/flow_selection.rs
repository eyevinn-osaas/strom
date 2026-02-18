use strom_types::Flow;

use super::*;
impl StromApp {
    /// Get the currently selected flow.
    pub(super) fn current_flow(&self) -> Option<&Flow> {
        self.selected_flow_id
            .and_then(|id| self.flows.iter().find(|f| f.id == id))
    }

    /// Get the currently selected flow mutably.
    pub(super) fn current_flow_mut(&mut self) -> Option<&mut Flow> {
        self.selected_flow_id
            .and_then(|id| self.flows.iter_mut().find(|f| f.id == id))
    }

    /// Get the index of the currently selected flow (for UI rendering).
    pub(super) fn selected_flow_index(&self) -> Option<usize> {
        self.selected_flow_id
            .and_then(|id| self.flows.iter().position(|f| f.id == id))
    }

    /// Select a flow by ID.
    pub(super) fn select_flow(&mut self, flow_id: strom_types::FlowId) {
        if let Some(flow) = self.flows.iter().find(|f| f.id == flow_id) {
            self.selected_flow_id = Some(flow_id);
            self.graph.deselect_all();
            self.graph.load(flow.elements.clone(), flow.links.clone());
            self.graph.load_blocks(flow.blocks.clone());
            tracing::info!("Selected flow: {} ({})", flow.name, flow_id);
        } else {
            tracing::warn!("Cannot select flow {}: not found", flow_id);
        }
    }

    /// Clear the current flow selection.
    pub(super) fn clear_flow_selection(&mut self) {
        self.selected_flow_id = None;
        self.graph.load(vec![], vec![]);
        self.graph.load_blocks(vec![]);
    }

    /// Add a log entry, maintaining the maximum size limit.
    pub(super) fn add_log_entry(&mut self, entry: LogEntry) {
        self.log_entries.push(entry);
        // Trim to max size
        while self.log_entries.len() > self.max_log_entries {
            self.log_entries.remove(0);
        }
    }

    /// Clear all log entries.
    pub(super) fn clear_log_entries(&mut self) {
        self.log_entries.clear();
        self.error = None;
    }

    /// Get log entry counts by level.
    pub(super) fn log_counts(&self) -> (usize, usize, usize) {
        let errors = self
            .log_entries
            .iter()
            .filter(|e| e.level == LogLevel::Error)
            .count();
        let warnings = self
            .log_entries
            .iter()
            .filter(|e| e.level == LogLevel::Warning)
            .count();
        let infos = self
            .log_entries
            .iter()
            .filter(|e| e.level == LogLevel::Info)
            .count();
        (errors, warnings, infos)
    }
}
