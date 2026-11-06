#![windows_subsystem = "windows"]
mod audio_probe;
mod drag_probe;
mod catalog;
mod duration;
mod folder_picker;
mod player;
mod desktop;
mod protools;
mod semantic;
mod layer_render;
mod full_wave;
mod audio_tools;
mod audio_index;
mod model_registry;
mod model_download;
mod audio_text;
mod semantic_vector;
mod semantic_eval;
mod visual_analysis;
mod licensing;
mod workbench_jobs;
mod bwf_metadata;
use tauri::Manager;

#[tauri::command]
fn native_info() -> String {
    format!("Rust 原生调用成功\n系统：{} / {}\n版本：{}", std::env::consts::OS, std::env::consts::ARCH, env!("CARGO_PKG_VERSION"))
}

// 版本号走自有命令，不依赖 core:app 权限：界面徽标必须永远有值，而不是退化成「v—」。
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn main() {
    let _startup=match desktop::startup_guard(){Ok(Some(lock))=>lock,Ok(None)=>return,Err(e)=>{desktop::message(&e);return;}};
    tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let audio_text_dir = data.join("audio-text-v1");
            std::fs::create_dir_all(&audio_text_dir)?;
            let state = catalog::CatalogState::new(data.join("catalog.sqlite"))
                .map_err(std::io::Error::other)?;
            app.manage(state);
            app.manage(player::PlayerState::new());
            app.manage(full_wave::FullWaveState::new(app.path().app_cache_dir()?.join("full-wave-v1")));
            app.manage(audio_tools::AudioToolsState::new(app.path().app_cache_dir()?.join("audio-tools-v1")));
            app.manage(layer_render::LayerRenderState::new(app.path().app_cache_dir()?.join("layer-renders-v1")));
            app.manage(audio_index::AudioIndexState::new(data.join("audio-index-v1")));
            app.manage(model_registry::ModelRegistryState::new(data.join("model-registry-v1")));
            app.manage(model_download::ModelDownloadState::new(data.join("model-downloads-v1")));
            app.manage(audio_text::AudioTextState::new(audio_text_dir));
            app.manage(semantic_vector::SemanticVectorState::new(data.join("semantic-vector-v1")));
            app.manage(visual_analysis::VisualAnalysisState::new(app.path().app_cache_dir()?.join("visual-analysis-v1")));
            app.manage(licensing::LicenseState::new(data.join("licensing-v1")));
            app.manage(workbench_jobs::WorkbenchJobsState::new(app.path().app_cache_dir()?.join("library-maintenance-v1")));
            app.manage(semantic::SemanticState::new(data.join("semantic")));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            native_info,
            app_version,
            audio_probe::test_tone,
            audio_probe::decode_sample,
            audio_probe::play_sample,
            drag_probe::drag_sample,
            drag_probe::catalog_drag_out,
            drag_probe::catalog_drag_many,
            catalog::catalog_preview,
            catalog::catalog_list,
            catalog::catalog_get_items,
            catalog::catalog_assign_library,
            catalog::library_tools::library_overview,
            catalog::library_tools::library_pin,
            catalog::library_tools::library_pin_color,
            catalog::library_tools::library_playlist_create,
            catalog::library_tools::library_playlist_add,
            catalog::library_tools::catalog_remove_preview,
            catalog::library_tools::catalog_remove_commit,
            catalog::source_ops::source_preview,
            catalog::source_ops::source_commit,
            catalog::source_ops::source_reveal,
            catalog::source_ops::source_copy,
            catalog::catalog_start,
            catalog::catalog_jobs,
            catalog::catalog_cancel,
            catalog::catalog_resume,
            catalog::catalog_history,
            catalog::catalog_folders,
            folder_picker::catalog_pick_folder,
            catalog::catalog_set_favorite,
            catalog::catalog_set_tags,
            catalog::catalog_check_availability,
            catalog::catalog_prepare_fingerprint,
            catalog::catalog_relocate,
            desktop::catalog_reveal,
            desktop::catalog_copy_paths,
            protools::protools_status,
            protools::protools_import_preview,
            semantic::semantic_status,
            semantic::semantic_build,
            semantic::semantic_search,
            semantic::semantic_cancel,
            layer_render::layer_render_start,
            layer_render::layer_render_status,
            layer_render::layer_render_cancel,
            layer_render::layer_render_export,
            player::player_render_preview,
            full_wave::full_wave_start,
            full_wave::full_wave_status,
            full_wave::full_wave_cancel,
            player::player_preview_full_at,
            audio_tools::audio_tools_start,
            audio_tools::audio_tools_status,
            audio_tools::audio_tools_cancel,
            audio_tools::audio_tools_preview,
            audio_tools::audio_tools_export,
            audio_index::audio_index_start,
            audio_index::audio_index_status,
            audio_index::audio_index_cancel,
            audio_index::audio_index_search,
            model_registry::model_registry_list,
            model_registry::model_registry_verify,
            model_download::model_download_list,
            model_download::model_download_start,
            model_download::model_download_status,
            model_download::model_download_cancel,
            audio_text::audio_text_status,
            audio_text::audio_text_selftest,
            semantic_vector::semantic_vector_build,
            semantic_vector::semantic_vector_status,
            semantic_vector::semantic_vector_cancel,
            semantic_vector::semantic_vector_search_text,
            semantic_vector::semantic_vector_search_audio,
            semantic_eval::semantic_vector_eval,
            semantic_eval::semantic_vector_eval_template,
            visual_analysis::visual_analysis_start,
            visual_analysis::visual_analysis_status,
            visual_analysis::visual_analysis_cancel,
            licensing::licensing_status,
            licensing::licensing_configure_dev,
            licensing::licensing_request,
            licensing::licensing_import_offline,
            workbench_jobs::ucs_suggest,
            workbench_jobs::workbench_job_start,
            workbench_jobs::workbench_job_status,
            workbench_jobs::workbench_job_cancel,
            workbench_jobs::production_capabilities,
            bwf_metadata::bwf_read_metadata,
            bwf_metadata::bwf_write_metadata,
            player::player_load,
            player::player_preview_at,
            player::player_control,
            player::player_status,
            player::player_waveform
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}
