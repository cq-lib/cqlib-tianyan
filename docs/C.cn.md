# cqlib-tianyan — C 教程

本教程是使用 **cqlib-tianyan** C 绑定与**天衍量子云平台**（`qc.zdxlz.com`）交互的完整指南，涵盖认证、线路提交、结果获取以及内存管理等核心功能。C 绑定通过 `extern "C"` 接口暴露 Rust 库的全部能力，适合嵌入到任意 C/C++ 项目中使用。

---

## 目录

1. [构建与安装](#1-构建与安装)
2. [认证](#2-认证)
3. [列举与检查后端](#3-列举与检查后端)
4. [提交线路](#4-提交线路)
5. [获取结果](#5-获取结果)
6. [批量提交](#6-批量提交)
7. [进阶：CalibrationMode](#7-进阶calibrationmode)
8. [错误处理](#8-错误处理)
9. [内存管理](#9-内存管理)

---

## 1. 构建与安装

### 1.1 构建 Rust 库

C 绑定以 Rust 静态库或动态库的形式提供。首先在仓库根目录执行构建：

```bash
# 构建 release 版本（推荐用于生产环境）
cargo build -p binding-c --release

# 或构建 debug 版本（用于开发调试）
cargo build -p binding-c
```

构建完成后，库文件位于：

| 平台 | 静态库（release） | 动态库（release） |
|------|-------------------|-------------------|
| macOS | `target/release/libbinding_c.a` | `target/release/libbinding_c.dylib` |
| Linux | `target/release/libbinding_c.a` | `target/release/libbinding_c.so` |

头文件由 [cbindgen](https://github.com/mozilla/cbindgen) 自动生成，位于：

```
crates/binding-c/include/cqlib_tianyan.h
```

### 1.2 使用 Makefile（推荐）

`crates/binding-c/` 目录下提供了 `Makefile`，可直接使用：

```bash
cd crates/binding-c

# 构建 debug 库并生成头文件
make

# 构建 release 库
make release

# 构建并运行示例（需要 TIANYAN_API_KEY）
TIANYAN_API_KEY="your_key" make example

# 构建并运行离线测试（无需 API Key）
make test

# 清理编译产物
make clean
```

### 1.3 手动编译链接

将头文件和库文件引入您的 C 项目：

```bash
# 链接静态库（macOS）
cc -I crates/binding-c/include \
   your_program.c \
   target/release/libbinding_c.a \
   -framework Security -framework CoreFoundation -framework SystemConfiguration -lc++ \
   -o your_program

# 链接静态库（Linux）
cc -I crates/binding-c/include \
   your_program.c \
   target/release/libbinding_c.a \
   -lpthread -ldl -lm \
   -o your_program
```

在 C 源文件中包含头文件：

```c
#include "cqlib_tianyan.h"
```

---

## 2. 认证

### 2.1 首次登录

使用您的 API Key（OpenID）调用 `tianyan_platform_login`。默认情况下，认证凭据会持久化存储至 `~/.cqlib/tianyan/credentials.json`（macOS/Linux），以供后续复用。

```c
#include "cqlib_tianyan.h"
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const char *api_key = getenv("TIANYAN_API_KEY");
    if (!api_key) {
        fprintf(stderr, "请设置 TIANYAN_API_KEY 环境变量\n");
        return 1;
    }

    TianyanPlatformC *platform = tianyan_platform_login(api_key);
    if (!platform) {
        char *err = tianyan_last_error();
        fprintf(stderr, "登录失败: %s\n", err ? err : "(未知错误)");
        tianyan_string_free(err);
        return 1;
    }

    printf("登录成功\n");

    /* ... 使用 platform ... */

    tianyan_platform_free(platform);
    return 0;
}
```

**返回值约定**：所有可能失败的函数在失败时返回 `NULL`，错误信息通过 `tianyan_last_error()` 获取（详见[第8节](#8-错误处理)）。

### 2.2 复用已保存的凭据

后续运行时可直接从磁盘加载凭据，若 Token 已过期，库会自动刷新：

```c
TianyanPlatformC *platform = tianyan_platform_from_credentials();
if (!platform) {
    char *err = tianyan_last_error();
    fprintf(stderr, "加载凭据失败: %s\n", err ? err : "(未知错误)");
    tianyan_string_free(err);
    return 1;
}
```

### 2.3 自定义认证选项

通过 `TianyanConfigC` 结构体控制凭据保存、自动刷新和存储路径。所有指针字段均可设为 `NULL` 以使用内置默认值：

```c
TianyanConfigC config = {
    .domain           = NULL,   /* NULL → 使用默认域名 "qc.zdxlz.com" */
    .credentials_path = NULL,   /* NULL → 使用默认路径 ~/.cqlib/tianyan/credentials.json */
    .save_credentials = false,  /* 不将凭据写入磁盘 */
    .auto_refresh     = false,  /* 不自动刷新 Token */
};

TianyanPlatformC *platform = tianyan_platform_login_with_config(api_key, &config);
```

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `domain` | `"qc.zdxlz.com"` | 平台域名 |
| `credentials_path` | `~/.cqlib/tianyan/credentials.json` | 凭据文件路径 |
| `save_credentials` | `true` | 登录或刷新后是否将凭据写入磁盘 |
| `auto_refresh` | `true` | Token 过期时是否自动重新登录 |

### 2.4 从自定义路径加载凭据

```c
TianyanConfigC config = {
    .domain           = NULL,
    .credentials_path = "/secure/vault/creds.json",
    .save_credentials = true,
    .auto_refresh     = true,
};

TianyanPlatformC *platform = tianyan_platform_from_credentials_with_config(&config);
```

---

## 3. 列举与检查后端

### 3.1 列举所有后端

`tianyan_platform_list_backends` 返回一个 `TianyanBackendC **` 数组，通过 `out_len` 输出元素个数。使用完毕后须调用 `tianyan_backend_list_free` 释放：

```c
size_t n_backends = 0;
TianyanBackendC **backends = tianyan_platform_list_backends(platform, &n_backends);
if (!backends) {
    char *err = tianyan_last_error();
    fprintf(stderr, "获取后端列表失败: %s\n", err ? err : "(未知错误)");
    tianyan_string_free(err);
    tianyan_platform_free(platform);
    return 1;
}

printf("可用后端 (%zu 个):\n", n_backends);
for (size_t i = 0; i < n_backends; i++) {
    const char *name         = tianyan_backend_name(backends[i]);
    const char *display_name = tianyan_backend_display_name(backends[i]);
    int         status       = tianyan_backend_status(backends[i]);
    int         toll         = tianyan_backend_toll(backends[i]);
    bool        available    = tianyan_backend_is_available(backends[i]);

    printf("  [%zu] %-20s  (%s)  状态=%d  收费=%d  可用=%s\n",
           i, name, display_name ? display_name : "",
           status, toll, available ? "是" : "否");
}

/* 列表使用完毕后释放 */
tianyan_backend_list_free(backends, n_backends);
```

`tianyan_backend_device_type(backend)` 返回设备类型：

| 类型码 | 类型 | 设备列表 |
|---|---|---|
| `3` | Simulator（仿真机） | `tianyan_sw`, `tianyan_s`, `tianyan_tn`, `tianyan_tnn`, `tianyan_sa`, `tianyan_swn` |
| `1` | Photonic（光量子） | `tianyan-p2000` |
| `0` | Superconducting（超导） | `tianyan176`, `tianyan176-2`, `tianyan24`, `tianyan504`, `tianyan-287`, `tianyan-294` |

`backend` 为 `NULL` 时返回 `-1`。

后端状态码和收费码含义：

| 状态码 | 含义 |
|--------|------|
| `0` | 运行中（Running） |
| `1` | 校准中（Calibration） |
| `2` | 维护中（UnderMaintenance） |
| `3` | 离线（OffLine） |
| `4` | 升级中（Upgrading） |
| `-1` | 未知（Unknown） |

| 收费码 | 含义 |
|--------|------|
| `1` | 免费（Free） |
| `2` | 付费（Paid） |
| `-1` | 未知（Unknown） |

仅 `Superconducting` 支持下载配置和读取误差校准；依赖配置的比特数查询也仅支持超导设备。

仅 `Superconducting` 和 `Simulator` 可提交任务，`Photonic` 和 `IonTrap` 在提交前报错。

`Auto` 对非超导设备跳过配置下载和校准，返回原始计数；非超导设备显式设置 `Enabled` 会在提交前报错。`Disabled` 始终返回原始计数。`is_available` 只反映运行状态，不代表设备支持提交任务。

### 3.2 获取指定后端

通过名称精确获取某个后端，返回的 `TianyanBackendC *` 需要独立释放：

```c
TianyanBackendC *backend = tianyan_platform_get_backend(platform, "tianyan-287");
if (!backend) {
    char *err = tianyan_last_error();
    fprintf(stderr, "获取后端失败: %s\n", err ? err : "(未知错误)");
    tianyan_string_free(err);
    /* 释放其他资源 ... */
    return 1;
}

printf("已选择: %s\n", tianyan_backend_name(backend));

size_t num_qubits = 0;
if (tianyan_backend_num_qubits(backend, &num_qubits)) {
    printf("物理比特数: %zu\n", num_qubits);
} else {
    char *err = tianyan_last_error();
    fprintf(stderr, "读取比特数失败: %s\n", err ? err : "(未知错误)");
    tianyan_string_free(err);
}

/* 使用完毕后释放 */
tianyan_backend_free(backend);
```

`tianyan_backend_num_qubits()` 仅支持超导设备，其他类型返回失败并设置错误；首次调用可能会下载并解析设备配置。它返回物理总比特数，包含禁用比特；可用拓扑比特数请通过设备配置拓扑理解。

### 3.3 检查后端是否可用

在提交线路前，建议先检查后端状态：

```c
if (!tianyan_backend_is_available(backend)) {
    fprintf(stderr, "后端当前不可用（状态码: %d）\n",
            tianyan_backend_status(backend));
    tianyan_backend_free(backend);
    return 1;
}
```

---

## 4. 提交线路

线路使用 **QCIS 字符串**表示，门操作和测量之间用 `\n` 分隔。`tianyan_backend_run` 立即返回 `TianyanTaskC *` 句柄，线路在云端异步执行，结果通过轮询获取。

### 4.1 Bell 态示例

```c
/* Bell 态 |Φ+⟩ = (|00⟩ + |11⟩) / √2
 *
 * CZ 是天衍原生双比特门；CNOT(控制=Q1, 目标=Q8) 分解为 H(Q8)·CZ(Q1,Q8)·H(Q8)
 *   H  Q1       — 对控制比特施加 Hadamard（制造叠加态）
 *   H  Q8       — 基变换（开始 CNOT 分解）
 *   CZ Q1 Q8    — 原生 CZ 门
 *   H  Q8       — 基变换回（完成 CNOT）
 *   M  Q1 / M Q8 — 分别测量两个比特
 *
 * tianyan-287 上 Q1 ↔ Q8 直接耦合
 */
const char *circuits[] = {
    "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8"
};
size_t n_circuits = 1;
size_t shots      = 1000;

TianyanTaskC *task = tianyan_backend_run(backend, circuits, n_circuits, shots);
if (!task) {
    char *err = tianyan_last_error();
    fprintf(stderr, "提交失败: %s\n", err ? err : "(未知错误)");
    tianyan_string_free(err);
    tianyan_backend_free(backend);
    tianyan_platform_free(platform);
    return 1;
}

/* 查看已提交的任务 ID */
size_t n_ids = 0;
char **ids = tianyan_task_ids(task, &n_ids);
printf("已提交 %zu 条线路，任务 ID:\n", n_ids);
for (size_t i = 0; i < n_ids; i++) {
    printf("  [%zu] %s\n", i, ids[i]);
}
tianyan_task_ids_free(ids, n_ids);
```

### 4.2 通过平台对象直接提交

无需先获取后端句柄，可直接通过平台对象提交：

```c
const char *circuits[] = { "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8" };

TianyanTaskC *task = tianyan_platform_submit(
    platform,
    circuits,
    1,           /* n_circuits */
    1000,        /* shots */
    "tianyan-287" /* device_name */
);
```

### 4.3 QCIS 格式说明

天衍平台使用 QCIS（Quantum Circuit Instruction Set）格式描述线路。常用指令：

| 指令 | 说明 |
|------|------|
| `H Qi` | 对比特 `i` 施加 Hadamard 门 |
| `X Qi` | Pauli-X 门 |
| `Y Qi` | Pauli-Y 门 |
| `Z Qi` | Pauli-Z 门 |
| `RZ Qi theta` | 绕 Z 轴旋转 `theta` 弧度 |
| `CZ Qi Qj` | 受控 Z 门（天衍原生双比特门） |
| `M Qi` | 测量比特 `i` |

---

## 5. 获取结果

### 5.1 阻塞等待（推荐）

`tianyan_task_wait` 轮询直到所有线路完成或超时。**默认行为**：仅对超导设备，当校准数据可用且被测量的量子比特数 ≤ 14 时，自动应用读取误差矫正（`CalibrationMode` 为 `Auto`）：

```c
printf("等待结果（超时=120s，轮询间隔=5s）...\n");
TianyanResultList *results = tianyan_task_wait(task, 120.0, 5.0);
if (!results) {
    char *err = tianyan_last_error();
    fprintf(stderr, "等待超时或失败: %s\n", err ? err : "(未知错误)");
    tianyan_string_free(err);
    tianyan_task_free(task);
    /* 释放其他资源 ... */
    return 1;
}

/* 遍历结果 */
size_t n_results = tianyan_result_list_len(results);
printf("获得 %zu 条结果:\n", n_results);
for (size_t i = 0; i < n_results; i++) {
    const char *task_id  = tianyan_result_task_id(results, i);   /* 不需要释放 */
    size_t      r_shots  = tianyan_result_shots(results, i);
    size_t      n_qubits = tianyan_result_num_qubits(results, i);
    char       *counts   = tianyan_result_counts_json(results, i); /* 需要释放 */

    printf("  [%zu] task_id=%s  shots=%zu  qubits=%zu\n",
           i, task_id ? task_id : "(null)", r_shots, n_qubits);
    printf("       counts=%s\n", counts ? counts : "(null)");

    tianyan_string_free(counts); /* 每次迭代后立即释放 */
}

tianyan_result_list_free(results);
```

`tianyan_result_counts_json` 返回的 JSON 格式示例：

```json
{"00": 487, "11": 513}
```

### 5.2 获取原始计数（不矫正）

若需要跳过读取误差矫正，直接获取硬件原始输出：

```c
TianyanResultList *raw_results = tianyan_task_wait_raw(task, 120.0, 5.0);
```

### 5.3 非阻塞状态快照

在不阻塞当前线程的情况下查询当前已完成的线路：

```c
TianyanResultList *partial = tianyan_task_status_snapshot(task);
if (partial) {
    size_t done  = tianyan_result_list_len(partial);
    size_t total = tianyan_task_num_circuits(task);
    printf("%zu / %zu 条线路已完成\n", done, total);
    tianyan_result_list_free(partial);
}
```

---

## 6. 批量提交

天衍平台每次请求最多接受 **50 条线路**。`cqlib-tianyan` 会自动拆分并按批次顺序提交，结果按提交顺序返回：

```c
/* 构造 120 条线路数组 */
#define N_CIRCUITS 120
const char *circuits[N_CIRCUITS];
for (int i = 0; i < N_CIRCUITS; i++) {
    circuits[i] = "H Q0\nM Q0";
}

/* 自动拆分为 50 + 50 + 20 三批提交 */
TianyanTaskC *task = tianyan_backend_run(backend, circuits, N_CIRCUITS, 1000);
if (!task) {
    char *err = tianyan_last_error();
    fprintf(stderr, "批量提交失败: %s\n", err ? err : "(未知错误)");
    tianyan_string_free(err);
    return 1;
}

/* 获取所有 120 个任务 ID */
size_t n_ids = 0;
char **ids = tianyan_task_ids(task, &n_ids);
printf("已提交 %zu 条线路\n", n_ids);  /* n_ids == 120 */
tianyan_task_ids_free(ids, n_ids);
```

等待结果时，`tianyan_task_wait` 同样会等待全部 120 条线路完成，结果按原始顺序排列。

---

## 7. 进阶：CalibrationMode

`tianyan_backend_run_with_mode` 的 `mode` 参数控制读取误差矫正行为：

| `mode` 值 | 行为 |
|-----------|------|
| `0`（**Auto**，默认） | 仅超导设备在有校准数据**且**被测比特数 ≤ 14 时自动矫正；否则静默回退到原始计数 |
| `1`（**Enabled**） | 仅超导设备可强制矫正，不受比特数限制；其他类型在提交前报错，缺少校准数据时也报错（调用方需保证内存充足） |
| `2`（**Disabled**） | 始终返回原始计数，不进行任何矫正 |

> **为何有 14 比特的限制？**
> 逆混淆矩阵的内存占用为 O(4ⁿ)，其中 n 为被测比特数。n = 14 时约占 2 GiB；n = 15 时约占 8 GiB。`Auto` 模式在超过阈值时会自动回退以防内存溢出（OOM）。若您确定内存充足，可使用 `mode=1`（Enabled）强制矫正。

### 7.1 强制矫正（Enabled）

```c
const char *circuits[] = { "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8" };

/* mode=1 → Enabled：强制应用矫正 */
TianyanTaskC *task = tianyan_backend_run_with_mode(
    backend, circuits, 1, 1000, 1
);
```

### 7.2 禁用矫正（Disabled）

```c
/* mode=2 → Disabled：始终返回原始计数 */
TianyanTaskC *task = tianyan_backend_run_with_mode(
    backend, circuits, 1, 1000, 2
);
```

使用 `run_with_mode` 后，通过 `tianyan_task_wait` 等待即可（矫正模式已在提交时绑定）：

```c
TianyanResultList *results = tianyan_task_wait(task, 120.0, 5.0);
```

### 7.3 快捷方式：run_raw

等同于 `run_with_mode(..., 2)`（Disabled），直接获取原始计数：

```c
TianyanTaskC *task = tianyan_backend_run_raw(backend, circuits, 1, 1000);
TianyanResultList *results = tianyan_task_wait_raw(task, 120.0, 5.0);
```

> **注意**：`run_raw` 配合 `task_wait_raw` 效果等同于 `run_with_mode(..., 2)` 配合 `task_wait`。

### 7.4 矫正前后对比

```c
const char *circuits[] = { "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8" };

/* 提交两次：一次矫正，一次原始 */
TianyanTaskC *task_cal = tianyan_backend_run_with_mode(backend, circuits, 1, 1000, 0);
TianyanTaskC *task_raw = tianyan_backend_run_raw(backend, circuits, 1, 1000);

TianyanResultList *cal_results = tianyan_task_wait(task_cal, 120.0, 5.0);
TianyanResultList *raw_results = tianyan_task_wait_raw(task_raw, 120.0, 5.0);

char *cal_counts = tianyan_result_counts_json(cal_results, 0);
char *raw_counts = tianyan_result_counts_json(raw_results, 0);

printf("矫正后: %s\n", cal_counts ? cal_counts : "(null)");
printf("原始值: %s\n", raw_counts ? raw_counts : "(null)");

tianyan_string_free(cal_counts);
tianyan_string_free(raw_counts);
tianyan_result_list_free(cal_results);
tianyan_result_list_free(raw_results);
tianyan_task_free(task_cal);
tianyan_task_free(task_raw);
```

---

## 8. 错误处理

C API 使用**线程局部错误字符串**传递错误信息，类似 `errno` 的设计：

- 每个可能失败的函数在失败时返回 `NULL`（指针返回值）或 `-1`/`false`（值返回值）
- 错误详情通过 `tianyan_last_error()` 获取
- 错误字符串是堆分配的，**必须**用 `tianyan_string_free()` 释放
- `tianyan_error_clear()` 清除当前线程的错误状态

### 8.1 标准错误检查模式

```c
/* 辅助函数：打印最后一条错误并返回 1（用于错误退出宏） */
static int print_last_error(const char *context) {
    char *err = tianyan_last_error();
    if (err) {
        fprintf(stderr, "[%s] 错误: %s\n", context, err);
        tianyan_string_free(err);
    } else {
        fprintf(stderr, "[%s] 未知错误\n", context);
    }
    return 1;
}

/* 使用示例 */
TianyanPlatformC *platform = tianyan_platform_login(api_key);
if (!platform) {
    return print_last_error("login");
}
```

### 8.2 手动错误检查

```c
TianyanBackendC *backend = tianyan_platform_get_backend(platform, "tianyan-287");
if (!backend) {
    char *err = tianyan_last_error();
    if (err) {
        fprintf(stderr, "获取后端失败: %s\n", err);
        tianyan_string_free(err);
    }
    tianyan_platform_free(platform);
    return 1;
}
```

### 8.3 清除错误状态

```c
/* 在需要区分"无错误"与"有错误"时，先清除旧错误再执行操作 */
tianyan_error_clear();

TianyanPlatformC *p = tianyan_platform_from_credentials();
if (!p) {
    char *err = tianyan_last_error();
    /* 此时 err 必然反映本次失败，而非上一次操作的残留 */
    fprintf(stderr, "凭据加载失败: %s\n", err ? err : "(未知错误)");
    tianyan_string_free(err);
}
```

### 8.4 常见错误场景

| 场景 | 返回值 | 错误信息特征 |
|------|--------|--------------|
| API Key 为空或格式无效 | `NULL` | 认证相关 |
| 网络连接失败 | `NULL` | 连接超时或拒绝 |
| 凭据文件不存在 | `NULL` | 文件路径相关 |
| 后端名称不存在 | `NULL` | 设备未找到 |
| `circuits` 为空数组 | `NULL` | 参数无效 |
| `shots` 为 0 | `NULL` | 参数无效 |
| 等待超时 | `NULL` | 超时相关 |

---

## 9. 内存管理

C API 有明确的内存所有权规则，理解这些规则是正确使用 API 的关键。

### 9.1 所有权规则总表

| 函数 / 类型 | 返回类型 | 谁负责释放 | 释放方式 |
|-------------|----------|------------|----------|
| `tianyan_platform_login()` | `TianyanPlatformC *` | 调用方 | `tianyan_platform_free()` |
| `tianyan_platform_list_backends()` | `TianyanBackendC **` | 调用方 | `tianyan_backend_list_free(ptr, len)` |
| `tianyan_platform_get_backend()` | `TianyanBackendC *` | 调用方 | `tianyan_backend_free()` |
| `tianyan_platform_submit()` | `TianyanTaskC *` | 调用方 | `tianyan_task_free()` |
| `tianyan_backend_run()` | `TianyanTaskC *` | 调用方 | `tianyan_task_free()` |
| `tianyan_task_wait()` | `TianyanResultList *` | 调用方 | `tianyan_result_list_free()` |
| `tianyan_task_ids()` | `char **` | 调用方 | `tianyan_task_ids_free(ptr, len)` |
| `tianyan_last_error()` | `char *` | 调用方 | `tianyan_string_free()` |
| `tianyan_result_counts_json()` | `char *` | 调用方 | `tianyan_string_free()` |
| `tianyan_backend_name()` | `const char *` | **不需要释放** | 生命周期同 backend 对象 |
| `tianyan_backend_display_name()` | `const char *` | **不需要释放** | 生命周期同 backend 对象 |
| `tianyan_result_task_id()` | `const char *` | **不需要释放** | 生命周期同 result list 对象 |
| `tianyan_task_device_name()` | `const char *` | **不需要释放** | 生命周期同 task 对象 |

**核心规则**：
- 返回 `char *`（可变指针）：调用方拥有，**必须**用 `tianyan_string_free()` 释放
- 返回 `const char *`（常量指针）：调用方**不拥有**，生命周期绑定到父对象
- 所有 `_free()` 函数均为 NULL 安全（传入 `NULL` 不会崩溃）

### 9.2 正确的资源释放顺序

建议按照与分配相反的顺序释放资源：

```c
TianyanPlatformC  *platform = /* ... */;
TianyanBackendC  **backends = /* ... */;
TianyanBackendC   *backend  = /* ... */;
TianyanTaskC      *task     = /* ... */;
TianyanResultList *results  = /* ... */;

/* 释放顺序：先释放最内层对象 */
tianyan_result_list_free(results);  /* 1. 先释放结果列表 */
tianyan_task_free(task);            /* 2. 释放任务句柄 */
tianyan_backend_free(backend);      /* 3. 释放单个后端（若有） */
tianyan_backend_list_free(backends, n_backends); /* 4. 释放后端列表（若有） */
tianyan_platform_free(platform);    /* 5. 最后释放平台 */
```

### 9.3 字符串释放示例

```c
/* tianyan_result_counts_json 返回堆分配字符串 → 必须释放 */
char *counts = tianyan_result_counts_json(results, 0);
if (counts) {
    printf("计数: %s\n", counts);
    tianyan_string_free(counts); /* 不能使用 free()，必须使用 tianyan_string_free() */
    counts = NULL;               /* 良好实践：置 NULL 防止 use-after-free */
}

/* tianyan_backend_name 返回常量指针 → 不需要释放 */
const char *name = tianyan_backend_name(backend);
printf("名称: %s\n", name);    /* 直接使用，无需释放 */
/* 注意：backend 被释放后，name 指针立即失效 */
```

### 9.4 错误路径的资源管理

在错误路径中确保所有已分配的资源都被正确释放：

```c
TianyanPlatformC *platform = NULL;
TianyanBackendC  *backend  = NULL;
TianyanTaskC     *task     = NULL;
TianyanResultList *results = NULL;
int ret = 1; /* 默认失败 */

platform = tianyan_platform_login(api_key);
if (!platform) { goto cleanup; }

backend = tianyan_platform_get_backend(platform, "tianyan-287");
if (!backend) { goto cleanup; }

const char *circuits[] = { "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8" };
task = tianyan_backend_run(backend, circuits, 1, 1000);
if (!task) { goto cleanup; }

results = tianyan_task_wait(task, 120.0, 5.0);
if (!results) { goto cleanup; }

/* 处理结果 ... */
ret = 0; /* 成功 */

cleanup:
    if (ret != 0) {
        char *err = tianyan_last_error();
        if (err) { fprintf(stderr, "错误: %s\n", err); tianyan_string_free(err); }
    }
    tianyan_result_list_free(results);
    tianyan_task_free(task);
    tianyan_backend_free(backend);
    tianyan_platform_free(platform);
    return ret;
```

---

## 完整示例

以下是一个完整的 C 程序，展示从登录到获取 Bell 态线路结果的完整工作流：

```c
/*
 * bell_state.c — cqlib-tianyan C API 完整示例
 *
 * 编译（macOS）：
 *   cargo build -p binding-c --release  # 在仓库根目录执行
 *   cc -I crates/binding-c/include \
 *      bell_state.c \
 *      target/release/libbinding_c.a \
 *      -framework Security -framework CoreFoundation \
 *      -framework SystemConfiguration -lc++ \
 *      -o bell_state
 *
 * 编译（Linux）：
 *   cargo build -p binding-c --release
 *   cc -I crates/binding-c/include \
 *      bell_state.c \
 *      target/release/libbinding_c.a \
 *      -lpthread -ldl -lm \
 *      -o bell_state
 *
 * 运行：
 *   TIANYAN_API_KEY="your_key" ./bell_state
 */

#include "cqlib_tianyan.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* 打印最后一条错误信息 */
static void print_error(const char *context) {
    char *err = tianyan_last_error();
    fprintf(stderr, "[%s] %s\n", context, err ? err : "未知错误");
    tianyan_string_free(err);
}

int main(void) {
    int ret = 1;

    TianyanPlatformC  *platform  = NULL;
    TianyanBackendC  **backends  = NULL;
    size_t             n_backends = 0;
    TianyanBackendC   *backend   = NULL;
    TianyanTaskC      *task      = NULL;
    TianyanResultList *results   = NULL;

    /* ── 1. 认证 ─────────────────────────────────────────────────────────── */
    const char *api_key = getenv("TIANYAN_API_KEY");
    if (!api_key || strlen(api_key) == 0) {
        fprintf(stderr, "请设置 TIANYAN_API_KEY 环境变量\n");
        return 1;
    }

    /*
     * 首次登录：凭据持久化至 ~/.cqlib/tianyan/credentials.json
     * 后续运行可改用: platform = tianyan_platform_from_credentials();
     */
    printf("正在登录...\n");
    platform = tianyan_platform_login(api_key);
    if (!platform) {
        print_error("login");
        goto cleanup;
    }
    printf("登录成功\n\n");

    /* ── 2. 列举后端 ─────────────────────────────────────────────────────── */
    backends = tianyan_platform_list_backends(platform, &n_backends);
    if (!backends) {
        print_error("list_backends");
        goto cleanup;
    }

    printf("可用后端 (%zu 个):\n", n_backends);
    const char *target_name = NULL;
    for (size_t i = 0; i < n_backends; i++) {
        int  status    = tianyan_backend_status(backends[i]);
        int  toll      = tianyan_backend_toll(backends[i]);
        bool available = tianyan_backend_is_available(backends[i]);
        printf("  [%zu] %-20s  状态=%-2d  收费=%d  可用=%s\n",
               i,
               tianyan_backend_name(backends[i]),
               status, toll,
               available ? "是" : "否");
        /* 优先选择 tianyan-287，否则选第一个可用后端 */
        if (!target_name) {
            if (strcmp(tianyan_backend_name(backends[i]), "tianyan-287") == 0 && available)
                target_name = "tianyan-287";
            else if (available)
                target_name = tianyan_backend_name(backends[i]);
        }
    }
    printf("\n");

    if (!target_name) {
        fprintf(stderr, "没有可用的后端\n");
        goto cleanup;
    }

    /* ── 3. 获取后端句柄 ─────────────────────────────────────────────────── */
    backend = tianyan_platform_get_backend(platform, target_name);
    if (!backend) {
        print_error("get_backend");
        goto cleanup;
    }
    printf("已选择后端: %s\n\n", tianyan_backend_name(backend));

    /* ── 4. 提交 Bell 态线路 ─────────────────────────────────────────────── */
    /*
     * Bell 态 |Φ+⟩ = (|00⟩ + |11⟩) / √2
     * 线路分析（tianyan-287 上 Q1 ↔ Q8 直接耦合）：
     *   H  Q1       → 制造叠加态
     *   H  Q8       → 基变换（开始 CNOT 分解）
     *   CZ Q1 Q8    → 原生 CZ 门（天衍硬件支持）
     *   H  Q8       → 基变换回（完成 CNOT 分解）
     *   M  Q1 M Q8  → 测量
     */
    const char *circuits[] = {
        "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8"
    };
    size_t n_circuits = 1;
    size_t shots      = 1000;

    printf("提交线路（%zu 条，shots=%zu）...\n", n_circuits, shots);

    /*
     * tianyan_backend_run 使用默认 CalibrationMode=Auto：
     *   有校准数据 + 被测比特数 ≤ 14 → 自动矫正
     *   否则 → 回退至原始计数
     */
    task = tianyan_backend_run(backend, circuits, n_circuits, shots);
    if (!task) {
        print_error("backend_run");
        goto cleanup;
    }

    /* 打印任务 ID */
    size_t n_ids = 0;
    char **ids = tianyan_task_ids(task, &n_ids);
    if (ids) {
        printf("任务 ID (%zu 个):\n", n_ids);
        for (size_t i = 0; i < n_ids; i++) {
            printf("  [%zu] %s\n", i, ids[i]);
        }
        tianyan_task_ids_free(ids, n_ids);
    }
    printf("\n");

    /* ── 5. 等待结果 ─────────────────────────────────────────────────────── */
    printf("等待结果（超时=120s，轮询间隔=5s）...\n");
    results = tianyan_task_wait(task, 120.0, 5.0);
    if (!results) {
        print_error("task_wait");
        goto cleanup;
    }

    /* ── 6. 打印结果 ─────────────────────────────────────────────────────── */
    size_t n_results = tianyan_result_list_len(results);
    printf("\n共 %zu 条结果:\n", n_results);
    for (size_t i = 0; i < n_results; i++) {
        /* const char * → 生命周期绑定到 results，无需释放 */
        const char *task_id  = tianyan_result_task_id(results, i);
        size_t      r_shots  = tianyan_result_shots(results, i);
        size_t      n_qubits = tianyan_result_num_qubits(results, i);

        /* char * → 堆分配，必须用 tianyan_string_free 释放 */
        char *counts = tianyan_result_counts_json(results, i);

        printf("  ── 结果 [%zu] ──────────────────────\n", i);
        printf("  task_id  : %s\n", task_id  ? task_id  : "(null)");
        printf("  shots    : %zu\n", r_shots);
        printf("  n_qubits : %zu\n", n_qubits);
        printf("  counts   : %s\n", counts   ? counts   : "(null)");
        /*
         * Bell 态理想分布：{"00": ~500, "11": ~500}
         * 注："01" 和 "10" 的出现表示读取误差或门保真度不足
         */

        tianyan_string_free(counts);
    }

    ret = 0; /* 成功 */

cleanup:
    /* 按分配逆序释放所有资源（所有 _free 函数均为 NULL 安全） */
    tianyan_result_list_free(results);
    tianyan_task_free(task);
    tianyan_backend_free(backend);
    tianyan_backend_list_free(backends, n_backends);
    tianyan_platform_free(platform);

    printf("\n%s\n", ret == 0 ? "完成。" : "发生错误，请查看上方错误信息。");
    return ret;
}
```

### 运行输出示例

```
正在登录...
登录成功

可用后端 (3 个):
  [0] tianyan-287           状态=0   收费=1  可用=是
  [1] tianyan-128           状态=1   收费=1  可用=否
  [2] tianyan-sim           状态=0   收费=1  可用=是

已选择后端: tianyan-287

提交线路（1 条，shots=1000）...
任务 ID (1 个):
  [0] task-a1b2c3d4-...

等待结果（超时=120s，轮询间隔=5s）...

共 1 条结果:
  ── 结果 [0] ──────────────────────
  task_id  : task-a1b2c3d4-...
  shots    : 1000
  n_qubits : 2
  counts   : {"00": 491, "11": 509}

完成。
```

`"00"` 和 `"11"` 各占约 50%，符合 Bell 态 |Φ+⟩ 的理论分布。
