//! OpenAPI documentation configuration.

use strom_types::api::{
    CreateFlowRequest, ElementInfoResponse, ElementListResponse, ErrorResponse, FlowListResponse,
    FlowResponse,
};
use strom_types::block::{
    BlockCategoriesResponse, BlockDefinition, BlockInstance, BlockListResponse, BlockResponse,
    CreateBlockRequest, ExposedProperty, ExternalPad, ExternalPads, PropertyMapping, PropertyType,
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
        crate::api::blocks::list_blocks,
        crate::api::blocks::get_block,
        crate::api::blocks::create_block,
        crate::api::blocks::update_block,
        crate::api::blocks::delete_block,
        crate::api::blocks::get_categories,
    ),
    components(
        schemas(
            CreateFlowRequest,
            FlowResponse,
            FlowListResponse,
            ElementListResponse,
            ElementInfoResponse,
            ErrorResponse,
            BlockDefinition,
            BlockInstance,
            BlockResponse,
            BlockListResponse,
            CreateBlockRequest,
            BlockCategoriesResponse,
            ExposedProperty,
            PropertyMapping,
            PropertyType,
            ExternalPads,
            ExternalPad,
        )
    ),
    tags(
        (name = "flows", description = "GStreamer flow management endpoints"),
        (name = "elements", description = "GStreamer element discovery endpoints"),
        (name = "blocks", description = "Reusable block management endpoints")
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
