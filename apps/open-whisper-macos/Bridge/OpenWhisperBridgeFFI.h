#ifndef OPEN_WHISPER_BRIDGE_FFI_H
#define OPEN_WHISPER_BRIDGE_FFI_H

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
char *ow_run_permission_diagnostics(void);
char *ow_start_dictation(void);
char *ow_stop_dictation(void);
char *ow_cancel_dictation(void);
char *ow_get_runtime_status(void);
char *ow_get_recording_levels(void);
char *ow_validate_hotkey(const char *request_json);
char *ow_reregister_hotkey(void);
void ow_string_free(char *raw);

#endif
