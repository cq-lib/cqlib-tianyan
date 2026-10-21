# Makefile for cqlib-tianyan C bindings
#
# Usage (run from crates/binding-c/):
#   make            # build debug library + header
#   make release    # build release library + header
#   make example    # build & run example/basic_usage.c (requires TIANYAN_API_KEY)
#   make test       # build & run tests/test_api.c (offline, no key needed)
#   make clean      # remove compiled C binaries

WORKSPACE_ROOT := ../..
INCLUDE        := include
LIB_DEBUG      := $(WORKSPACE_ROOT)/target/debug/libbinding_c.a
LIB_RELEASE    := $(WORKSPACE_ROOT)/target/release/libbinding_c.a
CC             ?= cc

# Extra system libs needed on Linux
UNAME := $(shell uname -s)
ifeq ($(UNAME),Linux)
    SYS_LIBS := -lpthread -ldl -lm $(shell pkg-config --libs openssl)
else
    # macOS: link all system frameworks pulled in transitively by Rust crates
    SYS_LIBS := -framework Security -framework CoreFoundation -framework SystemConfiguration -lc++
endif

.PHONY: all release example fusion-example test clean

## Build debug library and regenerate the C header
all:
	cargo build -p binding-c

## Build release library and regenerate the C header
release:
	cargo build -p binding-c --release

## Build and run example/basic_usage.c
## Requires TIANYAN_API_KEY environment variable to be set.
example: all
	$(CC) -I$(INCLUDE) example/basic_usage.c $(LIB_DEBUG) $(SYS_LIBS) -o example/basic_usage
	@echo ""
	@echo "Running example/basic_usage ..."
	@echo "-------------------------------------------"
	example/basic_usage

## Build the fusion example without logging in or making a network request.
fusion-example: all
	$(CC) -I$(INCLUDE) example/fusion_usage.c $(LIB_DEBUG) $(SYS_LIBS) -o example/fusion_usage

## Build and run tests/test_api.c (offline, no API key needed)
test: all
	$(CC) -I$(INCLUDE) tests/test_api.c $(LIB_DEBUG) $(SYS_LIBS) -o tests/test_api
	@echo ""
	tests/test_api
	$(CC) -I$(INCLUDE) tests/test_fusion.c $(LIB_DEBUG) $(SYS_LIBS) -o tests/test_fusion
	tests/test_fusion

## Remove compiled C binaries (does not clean Rust build artifacts)
clean:
	rm -f example/basic_usage example/fusion_usage tests/test_api tests/test_fusion
