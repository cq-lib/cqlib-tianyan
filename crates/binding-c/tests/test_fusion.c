/* Offline Fusion C ABI and ownership checks. */
#include "cqlib_tianyan.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static void expect_error(const char *part) {
    char *error = tianyan_last_error();
    assert(error != NULL);
    assert(strstr(error, part) != NULL);
    tianyan_string_free(error);
}

int main(void) {
    assert(tianyan_fusion_from_platform(NULL) == NULL);
    expect_error("platform must not be NULL");
    assert(tianyan_fusion_get(NULL, "task") == NULL);
    expect_error("fusion client must not be NULL");
    assert(tianyan_fusion_connect(NULL, "token", NULL, 1) == NULL);
    expect_error("host must not be NULL");
    assert(tianyan_fusion_connect("http://127.0.0.1:1", NULL, NULL, 1) == NULL);
    expect_error("API key or token is required");
    TianyanFusionC *client = tianyan_fusion_connect("http://127.0.0.1:1", "test-token", NULL, 1);
    assert(client != NULL);
    assert(tianyan_last_error() == NULL);
    assert(tianyan_fusion_submit(client, "{}") == NULL);
    expect_error("missing field");
    assert(tianyan_fusion_submit(client, "{\"name\":\"demo\",\"package\":\"missing.zip\",\"dispatch_mode\":\"manual\"}") == NULL);
    expect_error("manual dispatch requires");
    assert(tianyan_fusion_list(client, "[]") == NULL);
    expect_error("query must be an object");
    assert(tianyan_fusion_confirm(client, NULL, "p") == NULL);
    expect_error("task_id must not be NULL");
    assert(tianyan_fusion_wait(client, "t", -1, 1) == NULL);
    expect_error("finite and positive");
    assert(tianyan_fusion_list_jobs(client, 0, 20, "ALL") == NULL);
    expect_error("pageNo must be positive");
    assert(tianyan_fusion_list_jobs(client, 1, 101, "ALL") == NULL);
    expect_error("pageSize");
    assert(tianyan_fusion_schedule(client, "missing.zip", "balanced") == NULL);
    expect_error("readable ZIP");
    assert(tianyan_fusion_submit_decision(client, "demo", "{}") == NULL);
    expect_error("missing field");
    assert(tianyan_fusion_progress(NULL, "88") == NULL);
    expect_error("fusion client must not be NULL");
    tianyan_fusion_free(client);
    tianyan_fusion_free(NULL);
    tianyan_string_free(NULL);
    tianyan_error_clear();
    assert(tianyan_last_error() == NULL);
    puts("Fusion C ABI checks passed");
    return 0;
}
