#include "herdr_core.h"

#include <stddef.h>
#include <stdint.h>
#include <string.h>

static size_t change_count = 0;

static void record_change(void *context) {
    size_t *count = context;
    *count += 1;
}

int main(void) {
    const char options[] =
        "{\"schema_version\":1,\"herdr_socket_path\":\"/tmp/herdr.sock\","
        "\"remote_targets\":[],\"app_state_path\":\"/tmp/herdr-state.json\"}";
    HerdrCore *core = herdr_core_create((const uint8_t *)options, strlen(options));
    if (core == NULL) {
        return 1;
    }

    herdr_core_on_change(core, record_change, &change_count);

    const char event[] =
        "{\"schema_version\":1,\"kind\":\"focus_pane\","
        "\"payload\":{\"pane_id\":\"ffi-smoke\"}}";
    herdr_core_dispatch(core, (const uint8_t *)event, strlen(event));
    if (change_count != 1) {
        return 2;
    }

    HerdrBytes snapshot = herdr_core_snapshot(core);
    if (snapshot.ptr == NULL || snapshot.len == 0 || snapshot.cap < snapshot.len) {
        return 3;
    }
    herdr_core_free_bytes(snapshot);

    herdr_core_on_change(core, NULL, NULL);
    herdr_core_destroy(core);
    return 0;
}
