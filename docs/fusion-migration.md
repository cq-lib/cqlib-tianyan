# cqfusion 迁移：方案 B

状态：代码已迁入，完整构建和云上验收待补齐依赖后执行。目标分支 `feature/hybrid`。

## 交付边界

- Python 公共包为 `cqlib_tianyan.fusion`，随现有 wheel 发布；导出 `FusionWorkflow`、`Context`、`classic_stage`、`quantum_stage`、`TaskProfile`。`cqfusion` 命令保持原名。
- 工作流、`fusion.yaml`、任务包、断点、事件、本地和 Slurm 执行继续用 Python。
- 云融合 HTTP 实现在 Rust `cqlib_tianyan::fusion::FusionClient`；Python 的 `FusionClient` 是模型转换薄层，C 的 `tianyan_fusion_*` 使用同一套实现。
- Rust `platform.fusion()`、Python `FusionClient(platform=platform)`、C `tianyan_fusion_from_platform()` 共享平台客户端与认证状态。单独提供 token/API key 的构造方式仍可使用，默认不保存凭证。
- 量子执行使用新 `TianyanPlatform` → `get_backend()` → `run()` → `TaskHandle.wait()`。仅 `transpile=True` 时懒加载旧 `cqlib.TianYanPlatform`，供 `transpile_qcis` 读取配置和编译。
- `cqfusion` 兼容包将旧模块路径映射到新模块，类身份一致。旧任务包和旧 Slurm 脚本无需批量改写；新示例使用新命名空间。

## 接口契约

本轮迁移继承源项目的 SDK 协议。前端接口文档中的 `/hybrid/job/v2`、`code=200`、`tianyan.toml` 属于另一份契约，不能据此认定 SDK 接口已经上线。

| 操作 | SDK 路径（前缀 `/qccp-quantum/sdk/fusion`） | 请求要点 |
| --- | --- | --- |
| 上传 | POST `/package/upload` | multipart `file` |
| 保存 | POST `/task/save` | name、package_id、package_name、dispatch_mode、dispatch_preference、resource_hints、action=save |
| 推荐 | POST `/task/recommend` | task_id、dispatch_preference、resource_hints |
| 确认 | POST `/task/confirm` | task_id、plan_id |
| 手动提交 | POST `/task/run` | name、package_id、package_name、dispatch_mode=manual、quantum_resource、hpc_resource |
| 列表 | GET `/task/list` | name、status、dispatch_mode、limit、offset |
| 详情 / 删除 | GET / DELETE `/task/{id}` | 路径 ID 编码 |
| 事件 | GET `/task/{id}/events` | events 或数组 |
| 子任务 | GET `/task/{id}/subtasks` | items 或数组 |
| 结果 | GET `/task/{id}/results` | 文件响应 |

响应信封沿用 `code=0`、`data`、`message/msg`，也兼容源项目直接返回 data 的形式。任务、方案等保留原始字段；Rust/C 用 JSON，Python 转成原有 dataclass。错误保留 `kind`、`message`、`http_status`、`code`、`data`；Python 提供 `status_code`，并保留原生异常链。

智能提交的顺序是上传 → 保存 → 推荐 → 等待确认。Python 抛 `ConfirmationRequired`，CLI 退出码为 2，Rust/C 返回 `awaiting_confirmation=true`。仅 `--auto-confirm` 或明确 `--plan` 才自动确认。手动提交要求两类资源均明确指定。推荐/确认失败时错误数据包含已保存任务，便于查询和恢复，不能通过重复 submit 猜测执行状态。

GET 可重试最多两次网络错误、429、5xx；POST 和 DELETE 不做这类自动重放。401 仅在允许刷新且存在 API key 时重新认证一次。超时预算覆盖重试等待及认证。下载先写临时文件，完整接收后替换目标；清理服务器文件名中的目录部分，失败时保留原文件。

## 量子结果与兼容约束

量子阶段保留 `stage`、`engine=tianyan`、`machine`，返回全部 `task_ids` 与 `results`（每项含 result_index、task_id、counts、shots）。单结果额外保留顶层 counts。结果与线路的逐项对应关系未由现有 SDK 明确保证，因此不擅自将 task_id 等同于线路索引。编译映射以 `transpile_mappings` 保存。

登录键顺序：显式参数 > `TIANYAN_API_KEY` > `CQFUSION_LOGIN_KEY` > `LOGIN_KEY`。不转译和 mock 模式不初始化旧平台。

`cqfusion.client.http.HttpClient` 是被删除的内部 HTTP 实现，不作为兼容 API；工作流、运行时、客户端公开类及旧嵌套导入路径保持兼容。安装新 wheel 前应卸载独立 `cqfusion` 发行包，避免两个发行包争用同名目录。

## 构建与完整验收

必须提供与原仓库匹配的 `../cqlib2/crates/cqlib-core`，并安装提供 `cqlib.device.ExecutionResult` 的 cqlib 版本。可选转译还要求 `TianYanPlatform` 与 `mapping.transpile_qcis`。目前本地旧 cqlib 1.3.11 不提供新的 `cqlib.device`，依赖版本号不能仅凭 `cqlib>=0.1.0` 判断兼容；应在取得 cqlib2 后明确版本并锁定。

```bash
cd crates/binding-python
maturin develop
python -m pytest tests/fusion -q
cqfusion validate ../../examples/fusion/iterative
cqfusion run-local ../../examples/fusion/iterative --workdir /tmp/fusion-run --auto-confirm
cd ../..
make python-test   # 继续覆盖原 tests/test_cqlib_tianyan.py
make rust-test
make c-test
cd crates/binding-python
maturin build --release
```

注意：`binding-python` 到仓库根目录是 `../..`。CI 非交互本地执行应加 `--auto-confirm`。

wheel 必须在干净环境安装，确认 `cqlib_tianyan.fusion` 和 `cqfusion` 均能导入、旧新类身份一致、CLI 存在，运行四组样例与原有量子客户端测试。真实云验收另需测试账号：上传/推荐/确认、状态与结果、401 刷新、手动资源绑定；真实量子验证转译开关、多线路与结果映射；真实 Slurm 验证调度与执行节点依赖。

## 本次验证记录

详见 [fusion-verification.md](fusion-verification.md)。隔离测试不等价于完整 wheel、全仓库或真实云验收。
