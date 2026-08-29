#ifndef HERDR_CORE_SPIKE_H
#define HERDR_CORE_SPIKE_H

#include <stddef.h>
#include <stdint.h>

typedef struct HerdrCore HerdrCore;

typedef struct HerdrBytes {
  uint8_t *ptr;
  size_t len;
  size_t cap;
} HerdrBytes;

typedef void (*HerdrOnChange)(void *context);

HerdrCore *herdr_core_create(const uint8_t *options_json, size_t len);
void herdr_core_dispatch(
    HerdrCore *core,
    const uint8_t *event_json,
    size_t len
);
HerdrBytes herdr_core_snapshot(HerdrCore *core);
void herdr_core_on_change(
    HerdrCore *core,
    HerdrOnChange callback,
    void *context
);
void herdr_core_free_bytes(HerdrBytes bytes);
void herdr_core_destroy(HerdrCore *core);

#endif
