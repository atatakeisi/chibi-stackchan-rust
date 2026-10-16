#pragma once
#include <stdint.h>
#include <stddef.h>
struct StackchanAudioProfile {
  uint32_t pre_peak, live_peak, voiced, count;
  uint32_t peaks[120];
};
static_assert(sizeof(StackchanAudioProfile) == 496, "Rust ABI layout mismatch");
extern "C" bool stackchan_audio_profile(const int16_t* samples, size_t len,
    size_t pre, int32_t threshold, StackchanAudioProfile* out);
