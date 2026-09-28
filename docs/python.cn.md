# cqlib-tianyan — Python 教程

本教程是使用 **cqlib-tianyan** Python 库与**天衍量子云平台**（`qc.zdxlz.com`）交互的完整指南，涵盖认证、线路提交、结果获取以及读取错误矫正等核心功能。

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
10. [运行测试](#10-运行测试)

---

## 1. 安装

### 通过 pip 安装（推荐）

```bash
pip install cqlib-tianyan
```

**依赖要求**：Python >= 3.10

### 从源码安装（开发）

```bash
# 安装 maturin
pip install maturin

# 构建并安装
cd crates/binding-python
maturin develop
```

---

## 2. 认证

### 2.1 首次登录

使用您的 API Key（OpenID）调用 `TianyanPlatform.login`。默认情况下，认证凭据会持久化存储至 `~/.cqlib/tianyan/credentials.json`（macOS/Linux）或 `%APPDATA%\cqlib\tianyan\credentials.json`（Windows），以供后续复用。

```python
import os
from cqlib_tianyan import TianyanPlatform

api_key = os.environ["TIANYAN_API_KEY"]
platform = TianyanPlatform.login(api_key)
```

### 2.2 复用已保存的凭据

后续运行时可直接从文件加载凭据，若 Token 已过期，库会自动刷新：

```python
platform = TianyanPlatform.from_credentials()
```

### 2.3 自定义认证选项

通过关键字参数控制凭据保存、自动刷新和存储路径：

```python
# 仅在内存中保存，不写入磁盘
platform = TianyanPlatform.login(
    api_key,
    save_credentials=False,
    auto_refresh=False,
)
```

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `domain` | `"qc.zdxlz.com"` | 平台域名 |
| `save_credentials` | `True` | 登录或刷新后是否将凭据写入磁盘 |
| `auto_refresh` | `True` | Token 过期时是否自动重新登录 |
| `credentials_path` | `~/.cqlib/tianyan/credentials.json` | 自定义凭据文件路径 |

### 2.4 从自定义路径加载凭据

```python
platform = TianyanPlatform.from_credentials(
    credentials_path="/secure/vault/creds.json"
)
```

---

## 3. 列举与检查后端

### 3.1 列举所有后端

```python
backends = platform.list_backends()

for b in backends:
    print(f"{b.name:30} type={b.device_type} status={b.status}")
```

`backend.device_type` 返回设备的 `DeviceType`。
可使用 `.value` 读取字符串，或直接比较，例如 `backend.device_type == "simulator"`。

| 类型 | `.value` | 设备列表                                                                              |
|---|---|---------------------------------------------------------------------------------------|
| Simulator（仿真机） | `simulator` | `tianyan_sw`, `tianyan_s`, `tianyan_tn`, `tianyan_tnn`, `tianyan_sa`, `tianyan_swn`   |
| Photonic（光量子） | `photonic` | `tianyan-p2000`                                                                       |
| Superconducting（超导） | `superconducting` | `tianyan176`, `tianyan176-2`, `tianyan24`, `tianyan504`, `tianyan-287`, `tianyan-294` |

仅 `Superconducting` 支持下载配置和读取误差校准；依赖配置的比特数查询也仅支持超导设备。

仅 `Superconducting` 和 `Simulator` 可提交任务，`Photonic` 和 `IonTrap` 在提交前报错。

`Auto` 对非超导设备跳过配置下载和校准，返回原始计数；非超导设备显式设置 `Enabled` 会在提交前报错。`Disabled` 始终返回原始计数。`is_available` 只反映运行状态，不代表设备支持提交任务。

### 3.2 获取指定后端

```python
backend = platform.get_backend("tianyan-287")
print(f"已选择: {backend.name} ({backend.status})")
```

### 3.3 查看设备拓扑

仅超导设备支持以下配置查询。校准配置中包含量子比特/耦合器拓扑及硬件特性参数：

```python
device = backend.device_config()
topo = device.topology

print(f"物理比特数     : {backend.num_qubits()}")
print(f"可用拓扑比特数 : {topo.num_qubits}")
print(f"耦合数         : {topo.num_couplings}")

if device.calibration_time:
    print(f"校准时间 : {device.calibration_time}")
```

---

## 4. 提交线路

线路使用 **QCIS 字符串**表示，门操作和测量之间用 `\n` 分隔。

### 4.1 Bell 态示例

```python
# Bell 态 |Φ+⟩ = (|00⟩ + |11⟩) / √2
circuit = "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8"
shots = 1000

task = backend.run([circuit], shots)
print(f"任务 ID: {task.task_ids}")
```

`backend.run()` 立即返回 `TaskHandle`，线路已在云端排队，结果通过轮询获取。

### 4.2 使用快捷方式提交

也可以通过平台对象直接提交，无需先获取后端：

```python
task = platform.submit(
    circuits=[circuit],
    shots=1000,
    device_name="tianyan-287"
)
```

---

## 5. 获取结果

### 5.1 阻塞等待

轮询直到所有线路完成（或超时）：

`wait()` 和 `wait_raw()` 的 `timeout`、`poll_interval` 单位均为秒，
默认值分别为 `120.0` 和 `5.0`。可以直接调用 `task.wait()` 或 `task.wait_raw()`。

```python
results = task.wait(
    timeout=120,  # 最大等待时间（秒）
    poll_interval=5  # 轮询间隔（秒）
)

for r in results:
    print(f"任务 : {r.task_id}")
    print(f"计数 : {r.counts}")
    if r.probabilities:
        print(f"概率 : {r.probabilities}")
```

**默认行为**：`wait()` 会对超导设备在有校准数据且被测比特数 ≤ 14 时自动应用读取误差矫正（见第6节）。若需要获取原始计数：

```python
raw_results = task.wait_raw(timeout=120, poll_interval=5)
```

### 5.2 非阻塞状态查询

```python
partial = task.status()  # 可能少于已提交的线路数
print(f"{len(partial)}/{len(task.task_ids)} 已完成")
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

### 6.4 自动矫正（推荐）

`wait()` 使用 `CalibrationMode.Auto`（默认）策略，仅对超导设备下载配置，并在以下条件**同时**满足时应用矫正：

1. 后端存在可用的校准数据
2. 本次电路**测量的量子比特数 ≤ 14**

> **为何有 14 比特的限制？**
> 逆混淆矩阵的内存占用为 O(4ⁿ)，其中 n 为被测比特数。n = 14 时矩阵约占 2 GiB；n = 15 时约占 8 GiB。超过此阈值，`Auto` 模式会静默回退到原始计数，以防止内存溢出（OOM）。若您确定内存充足，可改用 `CalibrationMode.Enabled` 强制应用矫正。

```python
# 默认 CalibrationMode.Auto — ≤ 14 比特时自动矫正
results = task.wait(timeout=120, poll_interval=5)
```

### 6.5 矫正前后对比

```python
cal_results = task.wait(timeout=120, poll_interval=5)
raw_results = task.wait_raw(timeout=120, poll_interval=5)

for cr, rr in zip(cal_results, raw_results):
    print(f"矫正后: {cr.probabilities}")
    print(f"原始值: {rr.probabilities}")
```

---

## 7. 批量提交

天衍平台每次请求最多接受 **50 条线路**。`cqlib-tianyan` 会自动拆分并按批次顺序提交：

```python
# 120 条线路 — 自动拆分为 50 + 50 + 20 三批
circuits = ["H Q0\nM Q0"] * 120

task = backend.run(circuits, 1000)
# len(task.task_ids) == 120
```

结果按提交顺序返回。

---

## 8. 进阶：CalibrationMode

`CalibrationMode` 控制 `wait()` 的读取矫正行为：

| 枚举值 | 行为 |
|--------|------|
| `"auto"`（**默认**） | 仅超导设备在有校准数据**且**被测比特数 ≤ 14 时应用矫正；否则静默回退到原始计数 |
| `"enabled"` | 仅超导设备可强制应用矫正，不受比特数限制；其他类型在提交前报错，缺少校准数据时也报错（调用方需自行保证内存充足） |
| `"disabled"` | 始终返回原始计数，不进行任何矫正 |

> **`auto` 的 14 比特阈值**：混淆矩阵内存为 O(4ⁿ)，n > 14 时超过 2 GiB，`auto` 模式会自动回退以防 OOM。

在提交时指定矫正模式（接受字符串或 `CalibrationMode` 对象）：

`run(circuits, shots, *, calibration_mode="auto")` 的校准参数只能按关键字传入，
`platform.submit()` 也支持同名参数。以下调用展示不同选项，每次调用都会提交新任务。
旧的 `run_raw()`、`run_with_mode()` 作为兼容别名保留，并发出 `DeprecationWarning`。

```python
from cqlib_tianyan import CalibrationMode

circuits = ["H Q1\nM Q1"]

# 强制矫正 — 无数据时报错
task = backend.run(circuits, 1000, calibration_mode="enabled")

# 跳过矫正
task = backend.run(circuits, 1000, calibration_mode="disabled")

# 传入 CalibrationMode 对象（与字符串等价）
mode = CalibrationMode("disabled")
task = backend.run(circuits, 1000, calibration_mode=mode)  # 直接传对象即可

# CalibrationMode 支持与字符串比较
assert mode == "disabled"   # True
assert mode == CalibrationMode("disabled")  # True
```

---

## 9. 错误处理

所有可能失败的操作都会抛出异常。**注意**：在 abi3 模式下，`TianyanError` 不能按类型捕获，请捕获 `Exception`：

```python
try:
    platform = TianyanPlatform.login("invalid_key")
except Exception as e:
    print(f"登录失败: {e}")
```

常见错误：

| 场景 | 异常信息 |
|------|----------|
| 登录失败或 Token 无效 | 认证相关错误 |
| 网络或 HTTP 错误 | 连接超时等 |
| 找不到指定后端 | 设备不存在 |
| 参数错误 | 空线路列表、shots=0 等 |
| 超时 | `wait()` 在 timeout 内未收到全部结果 |

---

## 10. 运行测试

### 环境变量

| 变量 | 必需 | 说明 |
|------|------|------|
| `TIANYAN_API_KEY` | 集成测试 | 天衍平台 API Key |
| `TIANYAN_DOMAIN` | 可选 | 自定义域名 |
| `TIANYAN_DEVICE` | 可选 | 目标设备名称（如 "tianyan-287"） |

### 运行测试

```bash
# 运行所有测试（需要 API Key）
TIANYAN_API_KEY="your_key" pytest tests/ -v

# 指定设备运行测试
TIANYAN_API_KEY="your_key" TIANYAN_DEVICE="tianyan-287" pytest tests/ -v

# 仅运行单元测试（无需 API Key）
pytest tests/ -v -m "not integration"

# 包含慢测试（长超时）
TIANYAN_API_KEY="your_key" PYTEST_RUN_SLOW=1 pytest tests/ -v
```

---

## 完整示例

```python
import os
from cqlib_tianyan import TianyanPlatform, CalibrationMode

# 1. 认证（凭据保存至 ~/.cqlib/tianyan/）
api_key = os.environ["TIANYAN_API_KEY"]
platform = TianyanPlatform.login(api_key)

# 2. 选择后端
backend = platform.get_backend("tianyan-287")

# 3. 查看设备信息
print(f"设备: {backend.name}")
print(f"状态: {backend.status}")
print(f"物理比特数: {backend.num_qubits()}")

# 4. 提交 Bell 态线路（tianyan-287 上 Q1 ↔ Q8 耦合）
circuit = "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8"
task = backend.run([circuit], 1000)

# 5. 等待并打印矫正后的结果（默认 CalibrationMode.Auto）
results = task.wait(timeout=120, poll_interval=5)
for r in results:
    print(f"任务 ID: {r.task_id}")
    print(f"计数: {r.counts}")
    print(f"概率: {r.probabilities}")
```
