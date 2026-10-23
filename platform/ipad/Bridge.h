#include <stdint.h>
#include <stddef.h>
void dw_ipad_request_clear(void);
int32_t dw_ipad_request_byte(uint8_t byte);
int32_t dw_ipad_call(void);
const uint8_t *dw_ipad_response(void);
size_t dw_ipad_response_len(void);
const uint8_t *dw_ipad_frame(void);
size_t dw_ipad_frame_len(void);
const float *dw_ipad_audio(void);
size_t dw_ipad_audio_len(void);
