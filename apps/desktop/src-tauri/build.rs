fn main() {
    // Declaring the app's commands in a manifest means each one needs an explicit
    // `allow-<command>` permission in a capability file before the webview may call it (SEC-01).
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "lens_config",
            "nudge_lens",
            "set_click_through",
            "set_paused",
            "hide_lens",
            "open_ocr_language_settings",
            "open_model_manager",
            "copy_translation",
            "swap_languages",
            "set_languages",
            // `--trace-timings` only; ignored without it.
            "lens_drawn",
            // The model manager window (capabilities/models.json).
            "models_state",
            "download_models",
            "cancel_download",
            "delete_model",
            "finish_first_run",
        ]),
    ))
    .expect("failed to run tauri-build");
}
