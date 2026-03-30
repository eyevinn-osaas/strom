//! Permanent test verifying that pipeline lifecycle cleanup works correctly.
//!
//! Creates a real flow with PipelineManager, starts it, stops it, drops it,
//! and asserts that all GStreamer objects (pipeline + elements) are fully
//! finalized — no leaked references, no leaked OS resources.

use std::collections::HashMap;
use strom::blocks::BlockRegistry;
use strom::events::EventBroadcaster;
use strom::gst::pipeline::PipelineManager;
use strom_types::{Flow, Link};
use tempfile::NamedTempFile;

/// Build a simple flow: audiotestsrc → fakesink
/// Uses only core GStreamer elements to work in CI without plugins-good.
fn build_test_flow(name: &str) -> Flow {
    let mut flow = Flow::new(name);

    flow.elements.push(strom_types::Element {
        id: "src".to_string(),
        element_type: "audiotestsrc".to_string(),
        properties: {
            let mut p = HashMap::new();
            p.insert(
                "is-live".to_string(),
                strom_types::PropertyValue::Bool(true),
            );
            p
        },
        position: [100.0, 200.0].into(),
        pad_properties: HashMap::new(),
    });

    flow.elements.push(strom_types::Element {
        id: "sink".to_string(),
        element_type: "fakesink".to_string(),
        properties: HashMap::new(),
        position: [400.0, 200.0].into(),
        pad_properties: HashMap::new(),
    });

    flow.links.push(Link {
        from: "src:src".to_string(),
        to: "sink:sink".to_string(),
    });

    flow
}

/// Verify that stopping and dropping a PipelineManager fully finalizes the
/// GStreamer pipeline and all its elements. Any surviving GObject means a
/// strong reference cycle that will leak OS resources (sockets, threads).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pipeline_cleanup_after_stop_and_drop() {
    gstreamer::init().unwrap();

    let temp_file = NamedTempFile::new().unwrap();
    let registry = BlockRegistry::new(temp_file.path());
    let events = EventBroadcaster::new(10);
    let media_path = std::env::temp_dir();

    let flow = build_test_flow("lifecycle_test");

    let mut manager = PipelineManager::new(
        &flow,
        events,
        &registry,
        vec![],
        "all".to_string(),
        None,
        media_path,
    )
    .expect("Failed to create PipelineManager");

    let state = manager.start().expect("Failed to start pipeline");
    assert_eq!(state, strom_types::PipelineState::Playing);

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Take weak refs before stop+drop
    let pipeline_weak = manager.pipeline_weak();
    let element_weak_refs = manager.element_weak_refs();
    assert!(!element_weak_refs.is_empty(), "Flow should have elements");
    assert!(
        pipeline_weak.upgrade().is_some(),
        "Pipeline should be alive before drop"
    );

    // Stop and drop — mirrors what stop_flow() does
    manager.stop().expect("Failed to stop pipeline");
    drop(manager);

    // Verify pipeline is fully finalized
    assert!(
        pipeline_weak.upgrade().is_none(),
        "Pipeline still alive after drop — circular reference prevents finalization"
    );

    let leaked: Vec<_> = element_weak_refs
        .iter()
        .filter_map(|(name, weak)| weak.upgrade().map(|_| name.clone()))
        .collect();

    assert!(
        leaked.is_empty(),
        "Elements still alive after drop: {:?}",
        leaked
    );
}

/// Negative test: intentionally create a circular reference by capturing a
/// strong pipeline clone in a signal handler closure. Verify that our weak
/// ref detection catches the leak. If this test ever fails (i.e. no leak
/// detected), it means the detection mechanism is broken.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_leak_detection_catches_circular_reference() {
    use gstreamer::prelude::*;

    gstreamer::init().unwrap();

    let temp_file = NamedTempFile::new().unwrap();
    let registry = BlockRegistry::new(temp_file.path());
    let events = EventBroadcaster::new(10);
    let media_path = std::env::temp_dir();

    let flow = build_test_flow("leak_detection_test");

    let mut manager = PipelineManager::new(
        &flow,
        events,
        &registry,
        vec![],
        "all".to_string(),
        None,
        media_path,
    )
    .expect("Failed to create PipelineManager");

    // Intentionally create a circular reference: connect a signal handler
    // on an element that captures a strong ref to the pipeline.
    let pipeline_strong = manager.pipeline().clone();
    let src = manager.pipeline().by_name("src").expect("src element");
    src.connect_pad_added(move |_elem, _pad| {
        // This closure captures pipeline_strong, creating:
        // pipeline -> src element -> signal handler -> pipeline
        let _ = &pipeline_strong;
    });

    manager.start().expect("Failed to start pipeline");
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let pipeline_weak = manager.pipeline_weak();

    manager.stop().expect("Failed to stop pipeline");
    drop(manager);

    // The pipeline SHOULD still be alive because of the circular reference.
    // If it's not alive, our detection mechanism would miss real leaks.
    assert!(
        pipeline_weak.upgrade().is_some(),
        "Pipeline was finalized despite circular reference — \
         leak detection would miss real leaks!"
    );
}
