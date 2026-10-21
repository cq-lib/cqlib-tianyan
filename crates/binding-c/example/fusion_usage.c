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

    /* Build the JSON with your JSON library in production. This example only
       accepts a simple filename; escaping arbitrary paths requires a JSON encoder. */
    char options[4096];
    if (strpbrk(argv[2], "\\\"\n\r") != NULL) {
        fprintf(stderr, "unsupported filename characters\n");
        tianyan_fusion_free(fusion);
        return 2;
    }
    int size = snprintf(options, sizeof(options),
        "{\"name\":\"demo\",\"package\":\"%s\",\"dispatch_mode\":\"intelligent\"}", argv[2]);
    if (size < 0 || (size_t)size >= sizeof(options)) {
        tianyan_fusion_free(fusion);
        return 2;
    }
    char *outcome = tianyan_fusion_submit(fusion, options);
    if (!outcome) {
        tianyan_fusion_free(fusion);
        return report_error();
    }
    puts(outcome); /* awaiting_confirmation=true means choose a plan and call confirm later. */
    tianyan_string_free(outcome);
    tianyan_fusion_free(fusion);
    return 0;
}
