# cqlib-tianyan — Rust 教程

本教程是使用 **cqlib-tianyan** Rust 库与**天衍量子云平台**（`qc.zdxlz.com`）交互的完整指南，涵盖认证、线路提交、结果获取以及读取错误矫正等核心功能。

---

## 目录

1. [安装](#1-安装)
2. [认证](#2-认证)
3. [列举与检查后端](#3-列举与检查后端)
4. [提交线路](#4-提交线路)
5. [获取结果](#5-获取结果)
6. [读取误差矫正](#6-读取误差矫正)
7. [批量提交](#7-批量提交)
8. [进阶：CalibrationMode](#8-进阶calibrationmode)
9. [错误处理](#9-错误处理)

---

## 1. 安装

```sh
cargo add cqlib-tianyan
```

本库采用**同步（阻塞）I/O**，基于 `reqwest::blocking` 实现，无需 Tokio 异步运行时。

---

## 2. 认证

### 2.1 首次登录

使用您的 API Key（OpenID）调用 `TianyanPlatform::login`。默认情况下，认证凭据会持久化存储至 `~/.cqlib/tianyan/credentials.json`（macOS/Linux）或 `%APPDATA%\cqlib\tianyan\credentials.json`（Windows），以供后续复用。

```rust
use cqlib_tianyan::TianyanPlatform;

let api_key = std::env::var("TIANYAN_API_KEY").expect("set TIANYAN_API_KEY");
let platform = TianyanPlatform::login(&api_key)?;
```

### 2.2 复用已保存的凭据

后续运行时可直接从文件加载凭据，若 Token 已过期，库会自动刷新：

```rust
let platform = TianyanPlatform::from_credentials()?;
```

### 2.3 自定义认证选项

通过 `TianyanConfig` 控制凭据保存、自动刷新和存储路径：

```rust
use cqlib_tianyan::{TianyanPlatform, config::TianyanConfig};

// 仅在内存中保存，不写入磁盘
let cfg = TianyanConfig::default()
    .with_save_credentials(false)
    .with_auto_refresh(false);

let platform = TianyanPlatform::login_with_config(&api_key, cfg)?;
```

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `with_save_credentials(bool)` | `true` | 登录或刷新后是否将凭据写入磁盘 |
| `with_auto_refresh(bool)` | `true` | Token 过期时是否自动重新登录 |
| `with_credentials_path(path)` | `~/.cqlib/tianyan/credentials.json` | 自定义凭据文件路径 |

### 2.4 从自定义路径加载凭据

```rust
use std::path::PathBuf;
use cqlib_tianyan::{TianyanPlatform, config::TianyanConfig};

let cfg = TianyanConfig::default()
    .with_credentials_path(PathBuf::from("/secure/vault/creds.json"));

let platform = TianyanPlatform::from_credentials_with_config(cfg)?;
```

---

## 3. 列举与检查后端

### 3.1 列举所有后端

```rust
let backends = platform.list_backends()?;

for b in &backends {
    println!("{:30} type={:?} status={:?}", b.name, b.device_type, b.status);
}
```

`backend.device_type` 返回设备的 `DeviceType`。

| `DeviceType` | 设备列表 |
|---|---|
| `Simulator`（仿真机） | `tianyan_sw`, `tianyan_s`, `tianyan_tn`, `tianyan_tnn`, `tianyan_sa`, `tianyan_swn` |
| `Photonic`（光量子） | `tianyan-p2000` |
| `Superconducting`（超导） | `tianyan176`, `tianyan176-2`, `tianyan24`, `tianyan504`, `tianyan-287`|

仅 `Superconducting` 支持下载配置和读取误差校准；依赖配置的比特数查询也仅支持超导设备。

仅 `Superconducting` 和 `Simulator` 可提交任务，`Photonic` 和 `IonTrap` 在提交前报错。

`Auto` 对非超导设备跳过配置下载和校准，返回原始计数；非超导设备显式设置 `Enabled` 会在提交前报错。`Disabled` 始终返回原始计数。`is_available` 只反映运行状态，不代表设备支持提交任务。

### 3.2 获取指定后端

```rust
let backend = platform.get_backend("tianyan-287")?;
println!("已选择: {} ({:?})", backend.name, backend.status);
```

### 3.3 查看设备拓扑

仅超导设备支持以下配置查询。校准配置中包含量子比特/耦合器拓扑及硬件特性参数：

```rust
println!("物理比特数: {}", backend.num_qubits()?);

backend.with_device(|device| {
    let topo = device.topology();
    println!("可用拓扑比特数: {}", topo.num_qubits());
    println!("耦合数        : {}", topo.num_couplings());

    if let Some(t) = device.calibration_time() {
        println!("校准时间      : {}", t);
    }
})?;
```

---

## 4. 提交线路

线路使用 **QCIS 字符串**表示，门操作和测量之间用 `\n` 分隔。

### 4.1 Bell 态示例

```rust
use std::time::Duration;

// Bell 态 |Φ+⟩ = (|00⟩ + |11⟩) / √2
// CZ 是天衍原生双比特门；CNOT(控制=Q1, 目标=Q8) 分解为 H(Q8)·CZ(Q1,Q8)·H(Q8)
// H  Q1      — 对控制比特施加 Hadamard（制造叠加态）
// H  Q8      — 基变换（开始 CNOT 分解）
// CZ Q1 Q8   — 原生 CZ 门
// H  Q8      — 基变换回（完成 CNOT）
// M  Q1 / M Q8 — 分别测量两个比特
let circuit = "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8";
let shots = 1000;

let task = backend.run(vec![circuit.into()], shots)?;
println!("任务 ID: {:?}", task.task_ids());
```

`backend.run()` 立即返回 [`TaskHandle`]，线路已在云端排队，结果通过轮询获取。

### 4.2 使用 cqlib-core 中的 Circuit 对象

`CircuitInput` 也可以直接接受 `cqlib_core::circuit::Circuit` 对象：

```rust
use cqlib_tianyan::device::CircuitInput;
use cqlib_core::circuit::Circuit;

let circuit: Circuit = /* 编程构建 */;
let task = backend.run(vec![CircuitInput::from(circuit)], 1000)?;
```

---

## 5. 获取结果

### 5.1 阻塞等待

轮询直到所有线路完成（或超时）：

```rust
use std::time::Duration;

let results = task.wait(
    Duration::from_secs(120), // 最大等待时间
    Duration::from_secs(5),   // 轮询间隔
)?;

for r in &results {
    println!("任务 : {}", r.task_id());
    println!("计数 : {:?}", r.counts());
    if let Some(probs) = r.probabilities() {
        println!("概率 : {:?}", probs);
    }
}
```

**默认行为**：`wait()` 会对超导设备在有校准数据且被测比特数 ≤ 14 时自动应用读取误差矫正（见第6节）。若需要获取原始计数：

```rust
let raw_results = task.wait_raw(Duration::from_secs(120), Duration::from_secs(5))?;
```

### 5.2 非阻塞状态查询

```rust
let partial = task.status()?; // 可能少于已提交的线路数
println!("{}/{} 已完成", partial.len(), task.task_ids().len());
```

---

## 6. 读取误差矫正

量子测量并非完美。对于处于 `|0⟩`（或 `|1⟩`）态的量子比特 `k`，硬件可能以概率 `ε₀ₖ`（或 `ε₁ₖ`）产生读取错误。校准程序测量：

| 保真度 | 符号 | 含义 |
|--------|------|------|
| `f00[k]` | F(0\|0) | P(测量为 0 \| 制备态 0) |
| `f11[k]` | F(1\|1) | P(测量为 1 \| 制备态 1) |

### 6.1 混淆矩阵

对于单个量子比特，**混淆矩阵** `A_k` 为：

```
           测量为 0          测量为 1
制备态 0 [  f00[k]           1 - f00[k]  ]
制备态 1 [  1 - f11[k]       f11[k]      ]
```

`A_k` 将真实态概率映射到观测概率：

```
p_observed = A_k · p_true
```

### 6.2 多量子比特矫正（Kronecker 积）

对于 N 量子比特系统，完整混淆矩阵是各量子比特矩阵的 **张量（Kronecker）积**：

```
A = A_{N-1} ⊗ A_{N-2} ⊗ … ⊗ A_0
```

这将生成一个 2ᴺ × 2ᴺ 的矩阵，行为"制备态"比特串，列为"观测态"比特串。

### 6.3 逆矩阵矫正

**逆混淆矩阵** `A⁻¹` 将观测概率映射回估计的真实概率：

```
p_true ≈ A⁻¹ · p_observed
```

实现步骤：
1. 计算每个量子比特的 2×2 逆矩阵：`A_k⁻¹ = 1/(f00+f11-1) · [[f11, f11-1], [f00-1, f00]]`
2. 对所有测量比特取 Kronecker 积，构建完整逆矩阵。
3. 对观测概率向量施加完整逆矩阵。
4. 将负值截断为 0，并重新归一化使概率之和为 1。

### 6.4 读取校准数据

```rust
if let Some(cal) = backend.readout_calibration_data()? {
    for (i, name) in cal.qubit_names.iter().enumerate() {
        let f00 = cal.f00[i];
        let f11 = cal.f11[i];
        println!("{}: f00={:.4}  f11={:.4}  误差={:.2}%",
            name, f00, f11, (1.0 - (f00 + f11) / 2.0) * 100.0);
    }
}
```

### 6.5 自动矫正（推荐）

`wait()` 使用 `CalibrationMode::Auto`（默认）策略，仅对超导设备下载配置，并在以下条件**同时**满足时应用矫正：

1. 后端存在可用的校准数据。
2. 本次电路**测量的量子比特数 ≤ `AUTO_CALIBRATION_MAX_QUBITS`（14）**。

> **为何有 14 比特的限制？**
> 逆混淆矩阵的内存占用为 O(4ⁿ)，其中 n 为被测比特数。n = 14 时约 2 GiB；n = 15 时约 8 GiB。超过此阈值，`Auto` 模式会静默回退到原始计数以防止内存溢出（OOM）。如需对较大电路强制应用矫正，请改用 `CalibrationMode::Enabled`（调用方需自行保证内存充足）。

```rust
// 默认 CalibrationMode::Auto — ≤ 14 比特时自动矫正：
let results = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;
```

### 6.6 手动指定校准数据

通过 `backend.readout_calibration_data()` 获取 `ReadoutCalibrationData` 后传入。
库会按实际被测量的比特硬件索引自动过滤，无需手动筛选。

```rust
if let Some(cal) = backend.readout_calibration_data()? {
    let results = task.wait_with_calibration(
        Duration::from_secs(120),
        Duration::from_secs(5),
        &cal,
    )?;
}
```

### 6.7 矫正前后对比

```rust
let cal_r = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;
let raw_r = task.wait_raw(Duration::from_secs(120), Duration::from_secs(5))?;

for (cr, rr) in cal_r.iter().zip(raw_r.iter()) {
    println!("矫正后: {:?}", cr.probabilities());
    println!("原始值: {:?}", rr.probabilities());
}
```

---

## 7. 批量提交

天衍平台每次请求最多接受 **50 条线路**。`cqlib-tianyan` 会自动拆分并按批次顺序提交：

```rust
// 120 条线路 — 自动拆分为 50 + 50 + 20 三批
let circuits: Vec<_> = (0..120)
    .map(|_| "H Q0\nM Q0".into())
    .collect();

let task = backend.run(circuits, 1000)?;
// task.task_ids().len() == 120
```

结果按提交顺序返回。

---

## 8. 进阶：CalibrationMode

`CalibrationMode` 控制 `wait()` 的读取矫正行为：

| 枚举值 | 行为 |
|--------|------|
| `Auto`（**默认**） | 仅超导设备在有校准数据**且**被测比特数 ≤ 14 时应用矫正；否则静默回退到原始计数 |
| `Enabled` | 仅超导设备可强制应用矫正，不受比特数限制；其他类型在提交前报错，缺少校准数据时也报错（调用方需自行保证内存充足） |
| `Disabled` | 始终返回原始计数，不进行任何矫正 |

> **`Auto` 的 14 比特阈值**：混淆矩阵内存为 O(4ⁿ)，n > 14 时超过 2 GiB，`Auto` 会自动回退以防 OOM。详见常量 `AUTO_CALIBRATION_MAX_QUBITS`。

在提交时指定：

```rust
use cqlib_tianyan::task::CalibrationMode;

// 强制矫正 — 无数据时报错
let task = backend.run_with_mode(circuits, 1000, CalibrationMode::Enabled)?;

// 跳过矫正
let task = backend.run_raw(circuits, 1000)?;

// 也可以在提交后修改：
let mut task = backend.run(circuits, 1000)?;
task.calibration_mode = CalibrationMode::Disabled;
let results = task.wait(timeout, interval)?; // 原始计数
```

---

## 9. 错误处理

所有可能失败的操作都返回 `Result<T, TianyanError>`。错误枚举变体如下：

| 变体 | 原因 |
|------|------|
| `Auth(msg)` | 登录失败或 Token 无效 |
| `Http(err)` | 网络或 HTTP 级别错误 |
| `Json(err)` | 响应格式不符合预期 |
| `DeviceNotFound(name)` | 找不到指定名称的后端 |
| `InvalidInput(msg)` | 参数错误（空线路列表、shots=0 等） |
| `Timeout(duration)` | `wait()` 在 timeout 内未收到全部结果 |
| `Io(err)` | 凭据文件读写失败 |

```rust
use cqlib_tianyan::TianyanError;

match platform.get_backend("nonexistent") {
    Err(TianyanError::DeviceNotFound(name)) => {
        eprintln!("后端 '{}' 不存在，请检查设备列表。", name);
    }
    Err(e) => return Err(e),
    Ok(backend) => { /* 继续操作 */ }
}
```

---

## 完整示例

```rust
use cqlib_tianyan::{TianyanPlatform, TianyanError};
use std::time::Duration;

fn main() -> Result<(), TianyanError> {
    // 1. 认证（凭据保存至 ~/.cqlib/tianyan/）
    let api_key = std::env::var("TIANYAN_API_KEY").expect("set TIANYAN_API_KEY");
let platform = TianyanPlatform::login(&api_key)?;

    // 2. 选择后端
    let backend = platform.get_backend("tianyan-287")?;

    // 3. 查看读取保真度
    if let Some(cal) = backend.readout_calibration_data()? {
        println!("读取保真度：");
        for (i, q) in cal.qubit_names.iter().enumerate() {
            println!("  {} f00={:.4} f11={:.4}", q, cal.f00[i], cal.f11[i]);
        }
    }

    // 4. 提交 Bell 态线路（tianyan-287 上 Q1 ↔ Q8 耦合）
    let task = backend.run(
        vec!["H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8".into()],
        1000,
    )?;

    // 5. 等待并打印矫正后的结果（默认 CalibrationMode::Auto）
    let results = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;
    for r in &results {
        println!("计数: {:?}", r.counts());
    }

    Ok(())
}
```
