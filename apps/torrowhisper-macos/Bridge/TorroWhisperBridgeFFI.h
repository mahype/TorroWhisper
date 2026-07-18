#ifndef TORROWHISPER_BRIDGE_FFI_H
#define TORROWHISPER_BRIDGE_FFI_H

#include <stdint.h>

char *ow_load_settings(void);
char *ow_save_settings(const char *settings_json);
char *ow_list_input_devices(void);
char *ow_notify_device_change(void);
char *ow_get_model_status(void);
char *ow_get_model_status_list(void);
char *ow_start_model_download(const char *request_json);
char *ow_delete_model(const char *request_json);
char *ow_get_llm_status_list(void);
char *ow_start_llm_download(const char *request_json);
char *ow_delete_llm_model(const char *request_json);
char *ow_get_custom_llm_status_list(void);
char *ow_start_custom_llm_download(const char *request_json);
char *ow_delete_custom_llm_model(const char *request_json);
char *ow_list_remote_models(const char *request_json);
char *ow_get_llm_registry(void);
char *ow_set_llm_api_key(const char *request_json);
char *ow_delete_llm_api_key(const char *request_json);
char *ow_get_llm_api_key_status(void);
char *ow_list_pipeline_stages(void);
char *ow_run_permission_diagnostics(void);
char *ow_start_dictation(void);
char *ow_stop_dictation(void);
char *ow_cancel_dictation(void);
char *ow_get_runtime_status(void);
char *ow_get_recording_levels(void);
char *ow_get_last_timing(void);
char *ow_run_whisper_benchmark(const char *request_json);
char *ow_validate_hotkey(const char *request_json);
char *ow_reregister_hotkey(void);
char *ow_suspend_hotkey(void);
char *ow_load_history(void);
char *ow_delete_history_entry(const char *request_json);
char *ow_clear_history(void);
char *ow_get_log_path(void);
char *ow_log_message(const char *request_json);
char *ow_plugin_log(const char *request_json);
char *ow_session_started(void);
char *ow_session_ended_cleanly(void);
char *ow_write_diagnostics_log(void);
void ow_string_free(char *raw);

#endif
