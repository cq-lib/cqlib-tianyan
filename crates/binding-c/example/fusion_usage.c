/* Minimal fusion submission example. Never auto-confirms a recommendation. */
#include "cqlib_tianyan.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int report_error(void) {
    char *error = tianyan_last_error();
    fprintf(stderr, "%s\n", error ? error : "unknown fusion error");
    tianyan_string_free(error);
    return 1;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: fusion_usage API_KEY task.zip\n");
        return 2;
    }
    TianyanPlatformC *platform = tianyan_platform_login(argv[1]);
    if (!platform) return report_error();
    TianyanFusionC *fusion = tianyan_fusion_from_platform(platform);
    tianyan_platform_free(platform);
    if (!fusion) return report_error();

    char *schedule = tianyan_fusion_schedule(fusion, argv[2], "balanced");
    if (!schedule) {
        tianyan_fusion_free(fusion);
        return report_error();
    }
    puts(schedule); /* Inspect candidates. After user confirmation, call
        tianyan_fusion_select (or manual), then tianyan_fusion_submit_decision.
        This example only evaluates; no task is created. */
    tianyan_string_free(schedule);
    tianyan_fusion_free(fusion);
    return 0;
}
