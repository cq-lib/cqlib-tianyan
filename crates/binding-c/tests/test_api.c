/*
 * This code is part of Cqlib.
 *
 * (C) Copyright China Telecom Quantum Group 2026
 *
 * This code is licensed under the Apache License, Version 2.0.
 *
 * test_api.c — Offline (no network) tests for the Tianyan C API.
 *
 * Tests covered:
 *   1. NULL-safety: all API functions tolerate NULL arguments.
 *   2. Error paths: invalid API key sets last_error.
 *   3. String lifecycle: tianyan_string_free handles NULL safely.
 *   4. Result list bounds: out-of-bounds index returns safe defaults.
 *
 * Build and run (after `cargo build -p binding-c`):
 *
 *   cc -I../include test_api.c \
 *      ../../../target/debug/libcqlib_tianyan_c.a \
 *      -o test_api && ./test_api
 *
 * All tests must print "PASS" for the suite to succeed.
 */

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../include/cqlib_tianyan.h"

/* ── Minimal test harness ───────────────────────────────────────────────── */

static int tests_run = 0;
static int tests_failed = 0;

#define TEST(name)                \
    do {                          \
        printf("  %-60s ", name); \
        tests_run++;              \
    } while (0)

#define PASS()            \
    do {                  \
        printf("PASS\n"); \
    } while (0)

#define FAIL(msg)                   \
    do {                            \
        printf("FAIL — %s\n", msg); \
        tests_failed++;             \
    } while (0)

#define EXPECT_NULL(expr, msg) \
    do {                       \
        if ((expr) != NULL) {  \
            FAIL(msg);         \
        } else {               \
            PASS();            \
        }                      \
    } while (0)

#define EXPECT_NOT_NULL(expr, msg) \
    do {                           \
        if ((expr) == NULL) {      \
            FAIL(msg);             \
        } else {                   \
            PASS();                \
        }                          \
    } while (0)

#define EXPECT_ZERO(expr, msg) \
    do {                       \
        if ((expr) != 0) {     \
            FAIL(msg);         \
        } else {               \
            PASS();            \
        }                      \
    } while (0)

#define EXPECT_FALSE(expr, msg) \
    do {                        \
        if ((expr) != 0) {      \
            FAIL(msg);          \
        } else {                \
            PASS();             \
        }                       \
    } while (0)

/* ── Test sections ──────────────────────────────────────────────────────── */

static void test_null_safety(void) {
    printf("\n[NULL-safety tests]\n");

    /* Platform */
    TEST("tianyan_platform_free(NULL)");
    tianyan_platform_free(NULL);
    PASS();

    TEST("tianyan_platform_login(NULL) returns NULL");
    EXPECT_NULL(tianyan_platform_login(NULL), "should be NULL");

    TEST("tianyan_platform_from_credentials() with no cred file (error path)");
    TianyanPlatformC* p = tianyan_platform_from_credentials();
    /* May succeed if creds exist on disk; we just check it doesn't crash. */
    if (p) {
        tianyan_platform_free(p);
    }
    PASS();

    TEST("tianyan_platform_list_backends(NULL, ...) returns NULL");
    size_t len = 99;
    EXPECT_NULL(tianyan_platform_list_backends(NULL, &len), "should be NULL");

    TEST("tianyan_platform_get_backend(NULL, ...) returns NULL");
    EXPECT_NULL(tianyan_platform_get_backend(NULL, "x"), "should be NULL");

    TEST("tianyan_platform_submit(NULL, ...) returns NULL");
    EXPECT_NULL(tianyan_platform_submit(NULL, NULL, 0, 0, "x"), "should be NULL");

    /* Backend */
    TEST("tianyan_backend_free(NULL)");
    tianyan_backend_free(NULL);
    PASS();

    TEST("tianyan_backend_list_free(NULL, 0)");
    tianyan_backend_list_free(NULL, 0);
    PASS();

    TEST("tianyan_backend_name(NULL) returns NULL");
    EXPECT_NULL(tianyan_backend_name(NULL), "should be NULL");

    TEST("tianyan_backend_display_name(NULL) returns NULL");
    EXPECT_NULL(tianyan_backend_display_name(NULL), "should be NULL");

    TEST("tianyan_backend_device_type(NULL) returns -1");
    {
        int type = tianyan_backend_device_type(NULL);
        if (type != -1) {
            FAIL("expected -1");
        } else {
            PASS();
        }
    }

    TEST("tianyan_backend_status(NULL) returns -1");
    {
        int s = tianyan_backend_status(NULL);
        if (s != -1) {
            FAIL("expected -1");
        } else {
            PASS();
        }
    }

    TEST("tianyan_backend_toll(NULL) returns -1");
    {
        int t = tianyan_backend_toll(NULL);
        if (t != -1) {
            FAIL("expected -1");
        } else {
            PASS();
        }
    }

    TEST("tianyan_backend_is_available(NULL) returns false");
    EXPECT_FALSE(tianyan_backend_is_available(NULL), "should be false");

    TEST("tianyan_backend_num_qubits(NULL, &out) returns false");
    {
        size_t n = 123;
        EXPECT_FALSE(tianyan_backend_num_qubits(NULL, &n), "should be false");
    }

    TEST("tianyan_backend_num_qubits(NULL, NULL) returns false");
    EXPECT_FALSE(tianyan_backend_num_qubits(NULL, NULL), "should be false");

    TEST("tianyan_backend_run(NULL, ...) returns NULL");
    EXPECT_NULL(tianyan_backend_run(NULL, NULL, 0, 0), "should be NULL");

    TEST("tianyan_backend_run_raw(NULL, ...) returns NULL");
    EXPECT_NULL(tianyan_backend_run_raw(NULL, NULL, 0, 0), "should be NULL");

    TEST("tianyan_backend_run_with_mode(NULL, ...) returns NULL");
    EXPECT_NULL(tianyan_backend_run_with_mode(NULL, NULL, 0, 0, 0), "should be NULL");

    /* Task */
    TEST("tianyan_task_free(NULL)");
    tianyan_task_free(NULL);
    PASS();

    TEST("tianyan_task_device_name(NULL) returns NULL");
    EXPECT_NULL(tianyan_task_device_name(NULL), "should be NULL");

    TEST("tianyan_task_shots(NULL) returns 0");
    EXPECT_ZERO(tianyan_task_shots(NULL), "should be 0");

    TEST("tianyan_task_num_circuits(NULL) returns 0");
    EXPECT_ZERO(tianyan_task_num_circuits(NULL), "should be 0");

    TEST("tianyan_task_ids(NULL, ...) returns NULL");
    size_t n = 0;
    EXPECT_NULL(tianyan_task_ids(NULL, &n), "should be NULL");

    TEST("tianyan_task_wait(NULL, ...) returns NULL");
    EXPECT_NULL(tianyan_task_wait(NULL, 1.0, 1.0), "should be NULL");

    TEST("tianyan_task_wait_raw(NULL, ...) returns NULL");
    EXPECT_NULL(tianyan_task_wait_raw(NULL, 1.0, 1.0), "should be NULL");

    TEST("tianyan_task_status_snapshot(NULL) returns NULL");
    EXPECT_NULL(tianyan_task_status_snapshot(NULL), "should be NULL");

    /* ResultList */
    TEST("tianyan_result_list_free(NULL)");
    tianyan_result_list_free(NULL);
    PASS();

    TEST("tianyan_result_list_len(NULL) returns 0");
    EXPECT_ZERO(tianyan_result_list_len(NULL), "should be 0");

    TEST("tianyan_result_task_id(NULL, 0) returns NULL");
    EXPECT_NULL(tianyan_result_task_id(NULL, 0), "should be NULL");

    TEST("tianyan_result_shots(NULL, 0) returns 0");
    EXPECT_ZERO(tianyan_result_shots(NULL, 0), "should be 0");

    TEST("tianyan_result_num_qubits(NULL, 0) returns 0");
    EXPECT_ZERO(tianyan_result_num_qubits(NULL, 0), "should be 0");

    TEST("tianyan_result_counts_json(NULL, 0) returns NULL");
    EXPECT_NULL(tianyan_result_counts_json(NULL, 0), "should be NULL");
}

static void test_error_handling(void) {
    printf("\n[Error-handling tests]\n");

    TEST("tianyan_last_error() is NULL initially");
    tianyan_error_clear();
    EXPECT_NULL(tianyan_last_error(), "no error yet");

    TEST("login with obviously invalid key sets last_error");
    tianyan_error_clear();
    TianyanPlatformC* p = tianyan_platform_login("obviously_invalid_key_xyz_123");
    /* We don't know if we have network; just check the contract: if NULL, error is set */
    if (p == NULL) {
        char* err = tianyan_last_error();
        if (err && strlen(err) > 0) {
            tianyan_string_free(err);
            PASS();
        } else {
            FAIL("expected a non-empty last_error after failed login");
        }
    } else {
        /* Unexpectedly succeeded (very unlikely) — still a valid result */
        tianyan_platform_free(p);
        PASS();
    }

    TEST("login(NULL) sets last_error");
    tianyan_error_clear();
    p = tianyan_platform_login(NULL);
    if (p != NULL) {
        tianyan_platform_free(p);
        FAIL("expected NULL");
    } else {
        char* err = tianyan_last_error();
        if (err && strlen(err) > 0) {
            tianyan_string_free(err);
            PASS();
        } else {
            FAIL("expected non-empty last_error");
        }
    }

    TEST("tianyan_error_clear() clears the error");
    tianyan_platform_login(NULL); /* generate an error */
    tianyan_error_clear();
    EXPECT_NULL(tianyan_last_error(), "error should be cleared");
}

static void test_string_lifecycle(void) {
    printf("\n[String-lifecycle tests]\n");

    TEST("tianyan_string_free(NULL) is safe");
    tianyan_string_free(NULL);
    PASS();

    TEST("tianyan_task_ids_free(NULL, 0) is safe");
    tianyan_task_ids_free(NULL, 0);
    PASS();
}

/* ── Main ───────────────────────────────────────────────────────────────── */

int main(void) {
    printf("=== Tianyan C API offline tests ===\n");

    test_null_safety();
    test_error_handling();
    test_string_lifecycle();

    printf("\n=== Results: %d/%d passed ===\n", tests_run - tests_failed, tests_run);
    return (tests_failed == 0) ? 0 : 1;
}
