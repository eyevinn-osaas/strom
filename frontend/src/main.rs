//! Strom frontend application.

#![warn(clippy::all, rust_2018_idioms)]
// Allow dead code in frontend - code is used through WASM/eframe traits
#![allow(dead_code)]

mod api;
mod app;
mod graph;
mod palette;
mod properties;
mod sse;

#[cfg(target_arch = "wasm32")]
fn main() {
    use app::StromApp;
    use wasm_bindgen::JsCast;
    // Initialize panic handler for better error messages in browser console
    console_error_panic_hook::set_once();

    // Initialize tracing for WASM
    tracing_wasm::set_as_global_default();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");
        let canvas = document
            .get_element_by_id("strom_app_canvas")
            .expect("Failed to find strom_app_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("strom_app_canvas is not a canvas");

        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| {
                    // Set dark theme
                    cc.egui_ctx.set_visuals(egui::Visuals::dark());
                    Ok(Box::new(StromApp::new(cc)))
                }),
            )
            .await
            .expect("Failed to start eframe");
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("This frontend is designed to run as WebAssembly in a browser.");
    eprintln!("Please use `trunk serve` to run it.");
    std::process::exit(1);
}
