//! Local-only browser application. The encoder and PNG decoder are shared with the CLI.

#[cfg(any(target_arch = "wasm32", test))]
mod state;

#[cfg(target_arch = "wasm32")]
mod browser;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    browser::start()
}
