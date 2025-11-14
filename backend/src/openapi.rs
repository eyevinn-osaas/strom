//! OpenAPI documentation configuration.

use strom_types::api::{
    CreateFlowRequest, ElementInfoResponse, ElementListResponse, ErrorResponse, FlowListResponse,
    FlowResponse,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::flows::list_flows,
        crate::api::flows::create_flow,
        crate::api::flows::get_flow,
        crate::api::flows::update_flow,
        crate::api::flows::delete_flow,
        crate::api::flows::start_flow,
        crate::api::flows::stop_flow,
        crate::api::elements::list_elements,
        crate::api::elements::get_element_info,
    ),
    components(
        schemas(
            CreateFlowRequest,
            FlowResponse,
            FlowListResponse,
            ElementListResponse,
            ElementInfoResponse,
            ErrorResponse,
        )
    ),
    tags(
        (name = "flows", description = "GStreamer flow management endpoints"),
        (name = "elements", description = "GStreamer element discovery endpoints")
    ),
    info(
        title = "Strom GStreamer Flow Engine API",
        version = "0.1.0",
        description = "REST API for managing GStreamer pipelines through a visual flow interface",
        license(
            name = "MIT OR Apache-2.0"
        )
    )
)]
pub struct ApiDoc;
