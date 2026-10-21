# cqlib-tianyan

<div align="center">

![Tianyan Quantum Computing](https://jiangsu-10.zos.ctyun.cn/qccp1/uiUpdate/img/logo.png)

**天衍量子云平台**客户端库——支持 Rust、Python 和 C

[English](README.en.md) · [Rust 教程](docs/rust.cn.md) · [Python 教程](docs/python.cn.md) · [C 教程](docs/C.cn.md)

</div>

---

## 简介

`cqlib-tianyan` 是访问[天衍量子云平台](https://qc.zdxlz.com)（`qc.zdxlz.com`）的客户端库，提供：

- 🔐 **认证**：API Key 一键登录，凭据自动持久化与刷新
- 🖥️ **后端管理**：列举量子计算机，查看拓扑与校准配置
- ⚛️ **任务提交**：提交 QCIS 线路，支持批量（自动拆分，单批最多 50 条）
- 📊 **结果获取**：阻塞等待或非阻塞状态查询
- 🎯 **读取误差矫正**：逆混淆矩阵方法，默认自动启用（≤ 14 比特）

核心库用 Rust 实现，并提供 **Python**（PyO3）和 **C**（cbindgen）两种语言绑定。

---

## 快速开始

### Rust

```sh
cargo add cqlib-tianyan
```

```rust
use cqlib_tianyan::TianyanPlatform;
use std::time::Duration;

let platform = TianyanPlatform::login(&std::env::var("TIANYAN_API_KEY")?)?;
let backend  = platform.get_backend("tianyan-287")?;
let task     = backend.run(vec!["H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8".into()], 1000)?;
let results  = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;

for r in &results {
    println!("计数: {:?}", r.counts());
}
```

→ [完整 Rust 教程](docs/rust.cn.md)

### Python

```sh
pip install cqlib-tianyan
```

```python
import os
from cqlib_tianyan import TianyanPlatform

platform = TianyanPlatform.login(os.environ["TIANYAN_API_KEY"])
backend  = platform.get_backend("tianyan-287")
task     = backend.run(["H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8"], shots=1000)
results  = task.wait(timeout=120, poll_interval=5)

print(results[0].counts())
```

→ [完整 Python 教程](docs/python.cn.md)

### C

```bash
# 构建库（在 workspace 根目录执行）
cargo build -p binding-c --release
```

```c
#include "cqlib_tianyan.h"

TianyanPlatformC *p = tianyan_platform_login(getenv("TIANYAN_API_KEY"));
TianyanBackendC  *b = tianyan_platform_get_backend(p, "tianyan-287");

const char *circuits[] = {"H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8"};
TianyanTaskC     *t = tianyan_backend_run(b, circuits, 1, 1000);
TianyanResultList *r = tianyan_task_wait(t, 120.0, 5.0);

printf("counts: %s\n", tianyan_result_counts_json(r, 0));

tianyan_result_list_free(r);
tianyan_task_free(t);
tianyan_backend_free(b);
tianyan_platform_free(p);
```

→ [完整 C 教程](docs/C.cn.md)

### 测试环境与系统代理

访问自建或测试环境域名（例如内网 IP）时，若本机开着系统 HTTP 代理，Rust 的 `reqwest` 会把请求也交给代理，常见表现是登录接口返回 **502**。请先设置：

```sh
export NO_PROXY=223.244.15.5
# 或多个地址：export NO_PROXY=223.244.15.5,localhost,127.0.0.1
```

再用 `TianyanConfig::custom("223.244.15.5:30002")` 并把 `scheme` 设为 `"http"`。

---

## 语言绑定

| 语言 | crate/模块 | 文档 |
|------|-----------|------|
| **Rust** | `crates/cqlib-tianyan` | [中文](docs/rust.cn.md) · [English](docs/rust.en.md) |
| **Python** | `crates/binding-python` | [中文](docs/python.cn.md) · [English](docs/python.en.md) |
| **C** | `crates/binding-c` | [中文](docs/C.cn.md) · [English](docs/C.en.md) |

---

## 项目结构

```
cqlib-tianyan/
├── crates/
│   ├── cqlib-tianyan/     # 核心 Rust 库
│   ├── binding-python/    # Python 绑定（PyO3）
│   └── binding-c/         # C 绑定（cbindgen）
└── docs/                  # 各语言详细教程
```

---

## 构建

本项目使用 **Make** 编排多语言构建，也可直接使用 **Cargo** 构建 Rust 部分。

### 使用 Make（推荐 macOS/Linux）

```bash
make all       # 构建所有（Rust + C + Python wheel）
make rust      # 仅构建 Rust（自动排除 Python 绑定）
make python    # 构建 Python wheel（需 maturin）
make c         # 构建 C 绑定并运行测试
make test      # 运行所有测试
make clean     # 清理所有构建产物
```

### 使用 Cargo

```bash
# 构建 Rust crate（自动排除需 maturin 的 Python 绑定）
cargo build --workspace --exclude binding-python

# 运行测试
cargo test --workspace --exclude binding-python
```

### Windows 平台

Windows 未预装 Make，建议直接使用 Cargo + Maturin：

```powershell
# 构建 Rust 部分
cargo build --workspace --exclude binding-python

# 构建 Python 绑定（需先安装 maturin: pip install maturin）
cd crates/binding-python
maturin build --release
```

或使用 Git Bash / WSL 获得 Make 支持。

---

## 许可证

[Apache License 2.0](LICENSE.txt) · Copyright © China Telecom Quantum Group 2026

## 超量融合工作流

新增 `cqlib_tianyan.fusion` 工作流子包及 `cqfusion` 兼容入口。融合云 API 在 Rust 中实现，Python 与 C 共用；本地和 Slurm 编排仍由 Python 执行。示例位于 `examples/fusion/`，用法与验收状态见 [融合迁移说明](docs/fusion-migration.md)。
