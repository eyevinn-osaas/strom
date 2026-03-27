//! Vision Mixer control page — serves the web-based switcher UI.

use axum::{
    extract::{Path, State},
    response::Html,
};
use strom_types::FlowId;

use crate::blocks::builtin::vision_mixer::overlay;
use crate::blocks::builtin::vision_mixer::properties as vm_props;
use crate::state::AppState;

const VISION_MIXER_HTML: &str = include_str!("../../static/vision-mixer.html");

/// Serve the vision mixer control page.
/// GET /player/vision-mixer/{flow_id}
pub async fn vision_mixer_page(
    State(state): State<AppState>,
    Path(flow_id): Path<FlowId>,
) -> Html<String> {
    let flows = state.get_flows().await;

    let Some(flow) = flows.iter().find(|f| f.id == flow_id) else {
        return Html(format!(
            "<html><body>Flow {} not found</body></html>",
            flow_id
        ));
    };

    let Some(vm_block) = flow
        .blocks
        .iter()
        .find(|b| b.block_definition_id == "builtin.vision_mixer")
    else {
        return Html(
            "<html><body>No vision mixer block found in this flow</body></html>".to_string(),
        );
    };

    let block_id = &vm_block.id;
    let num_inputs = vm_props::parse_num_inputs(&vm_block.properties);
    let labels = vm_props::parse_input_labels(&vm_block.properties, num_inputs);
    let num_dsk_inputs = vm_props::parse_num_dsk_inputs(&vm_block.properties);

    // Find the multiview WHEP endpoint
    let multiview_endpoint = flow
        .blocks
        .iter()
        .filter(|b| b.block_definition_id == "builtin.whep_output")
        .find_map(|b| {
            b.runtime_data
                .as_ref()
                .and_then(|rd| rd.get("whep_endpoint_id"))
                .filter(|eid| eid.contains("multiview"))
                .map(|eid| format!("/whep/{}", eid))
        })
        .unwrap_or_default();

    // Get current state from live overlay state or fall back to defaults
    let overlay = overlay::get_overlay_state(block_id);
    let initial_pgm_group = overlay.as_ref().map(|s| s.pgm_group()).unwrap_or_else(|| {
        vec![vm_props::parse_initial_pgm(
            &vm_block.properties,
            num_inputs,
        )]
    });
    let initial_pvw_group = overlay.as_ref().map(|s| s.pvw_group()).unwrap_or_else(|| {
        vec![vm_props::parse_initial_pvw(
            &vm_block.properties,
            num_inputs,
        )]
    });
    let ftb_active = overlay
        .as_ref()
        .map(|s| s.ftb_active.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(false);
    let dsk_states: Vec<bool> = overlay
        .as_ref()
        .map(|s| {
            s.dsk_enabled
                .iter()
                .map(|a| a.load(std::sync::atomic::Ordering::Relaxed))
                .collect()
        })
        .unwrap_or_else(|| vec![true; num_dsk_inputs]);
    let background_input: Option<usize> = overlay.as_ref().and_then(|s| s.background_input());

    // Build a single JSON config object (safe injection via <script type="application/json">)
    let config = serde_json::json!({
        "flow_id": flow_id.to_string(),
        "block_id": block_id,
        "num_inputs": num_inputs,
        "multiview_endpoint": multiview_endpoint,
        "input_labels": labels,
        "initial_pgm": initial_pgm_group.first().copied().unwrap_or(0),
        "initial_pvw": initial_pvw_group.first().copied().unwrap_or(1),
        "initial_pgm_group": initial_pgm_group,
        "initial_pvw_group": initial_pvw_group,
        "num_dsk_inputs": num_dsk_inputs,
        "ftb_active": ftb_active,
        "dsk_states": dsk_states,
        "background_input": background_input,
    });

    let html = VISION_MIXER_HTML.replace("{{VM_CONFIG_JSON}}", &config.to_string());

    Html(html)
}
