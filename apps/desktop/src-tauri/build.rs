fn main() {
    // Declaring the app's commands in a manifest means each one needs an explicit
    // `allow-<command>` permission in a capability file before the webview may call it (SEC-01).
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "lens_config",
            "nudge_lens",
            "set_click_through",
            "set_paused",
            "set_mode",
            "hide_lens",
            "open_ocr_language_settings",
            "open_model_manager",
            "open_settings",
            "copy_translation",
            "swap_languages",
            "set_languages",
            // `--trace-timings` only; ignored without it.
            "lens_drawn",
            // Freeze mode (MODE-03): the lens and the snapshot window (capabilities/freeze.json).
            "set_frozen",
            "freeze_snapshot",
            "freeze_ready",
            // The Settings window (capabilities/settings.json).
            "settings_state",
            "set_appearance",
            "set_hotkey",
            "record_hotkeys",
            "open_privacy_policy",
            // The model manager window (capabilities/models.json).
            "models_state",
            "download_models",
            "cancel_download",
            "delete_model",
            "import_models",
            "finish_first_run",
        ]),
    ))
    .expect("failed to run tauri-build");
}
