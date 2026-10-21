# 融合迁移验收记录（2026-10-12）

目标工作树：`feature/hybrid`。这份记录只描述本机实际执行的检查。原仓库没有 Git 提交历史；迁移前归档位于 `/var/folders/2r/n57nwqbn02q557zrzlz3czpr0000gn/T/cqlib-fusion-baseline-7we32c57/source.tar.gz`，源目录 `cqfusion` 未修改。未提交、未推送、未访问真实云。

## 已通过

| 检查 | 结果 | 验证范围 |
| --- | --- | --- |
| Rust 核心隔离编译与单测 | 25 passed | 将目标仓库实际 `auth/config/error/client/fusion` 源文件编入临时 crate；本地 HTTP 服务验证写请求不重放、智能提交等待确认、手动资源、401 刷新、错误恢复数据、下载原子性和超时等。量子核心未参与。 |
| PyO3 / C 源文件编译 | 通过 | 使用临时极简平台适配器编译目标仓库实际 `binding-python/src/fusion.rs`、`binding-c/src/fusion.rs` 与相同 Rust 融合核心；没有编译原仓库量子平台。 |
| Python 融合测试 | 41 passed | 用上述临时扩展执行目标仓库 `tests/fusion`；Python 薄层通过真实 Rust HTTP 调用本地服务；旧 `cqfusion` 导入与类身份、旧任务包 Slurm 子进程、批量量子结果均检查。量子平台对象在单测中模拟。 |
| C ABI 离线测试 | 通过 | `tests/test_fusion.c` 与隔离 C 静态库链接，验证 NULL、参数校验、错误字符串/句柄释放等；另通过动态库调用本地 HTTP 服务完成 C 智能提交，确认 3 次请求后返回 awaiting_confirmation，且没有确认请求；`fusion_usage.c 使用生成头文件通过 `-Wall -Wextra -Werror` 编译。 |
| `cqfusion validate` | 通过 | 在隔离扩展环境验证 `examples/fusion/iterative`。 |
| `cqfusion run-local` | 通过 | 在隔离扩展环境以 mock 量子后端、`--auto-confirm` 执行 iterative，结果 success、2 轮、3 次经典和 3 次量子子任务。 |
| 隔离 wheel 打包 | 通过 | 用同一份 Python 包目录和相同的 module-name/python-packages/include 选项打包临时最小 Rust 扩展；检查 wheel 含 `cqlib_tianyan/fusion`、`cqfusion`、原生模块、现有 `.pyi`、`cqfusion` 命令入口和声明依赖。不是目标仓库正式 wheel。 |
| Python 语法检查 | 通过 | 融合包、兼容包、示例和迁移测试全部 `compileall`。 |

临时验证工具、编译目标和 wheel 放在 `/tmp/cqlib-fusion-tools`，不属于交付内容；Rust 工具链也只装在该临时目录。

## 尚未通过完整交付门槛

`cargo check --workspace` 失败于工作区既有路径依赖 `../cqlib2/crates/cqlib-core/Cargo.toml` 不存在，错误发生在加载 Cargo manifest 阶段。相同原因使目标仓库的 `maturin develop`、正式 wheel、`make rust-test`、`make c-test`、`make python-test` 无法执行。没有伪造 `cqlib-core` 来代替生产依赖。尝试对 `https://gitee.com/zdxlz/cqlib2.git` 做只读引用查询，Gitee 要求认证，未取得源码。

已安装的旧 `cqlib` 1.3.11 有 `TianYanPlatform` 和 `mapping.transpile_qcis`，可供旧拓扑编译路径使用，但不提供现有新绑定在 `TaskHandle.wait()` 中导入的 `cqlib.device.ExecutionResult`。因此真实量子执行还需确认配套 `cqlib` 发行版与版本约束。当前无法证明原测试 `tests/test_cqlib_tianyan.py`、正式 wheel 安装、真实天衍线路运行、真实 Slurm 节点运行和云端融合 SDK 路径已通过。

补齐依赖后按 [迁移说明](fusion-migration.md) 的构建和完整验收命令执行，再开展真实云手工验收。云上融合服务是否支持这些 SDK 路径、字段及错误信封仍需服务端或测试环境验证。用户提供的前端 `/hybrid/job/v2` 文档不是此 SDK 路径的直接证据。
