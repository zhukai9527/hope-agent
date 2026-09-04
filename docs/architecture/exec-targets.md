# Exec 执行目标

`exec` 的 `target` 用于区分宿主机、WSL 与 Docker。未传值时为 `auto`，由运行时根据工作目录、命令特征、主机工具可用性与沙箱策略自动选择，用户无需手动指定执行环境。

## 自动策略（`auto`，默认）

fallback 顺序 **Host → WSL → Docker**：

1. **显式隔离保持权威**：`sandbox=true`、显式 `target=docker`，或会话处于 `isolated` 模式时，一律走 Docker，自动策略绝不绕过隔离边界。
2. **容器专属命令**：命令含 `docker compose` / `docker-compose` 时走 Docker。
3. **工具探测优先 Host**：解析命令首个可执行 token（`pnpm`、`npm`、`node`、`cargo`、`git`、`adb`、`vite`、`tsc`、`python*` 等，去路径、小写），在 Windows 主机 PATH 命中则走 Host（原生 `cmd /C`），macOS/Linux 走原生 `sh`。
4. **WSL 回退**：Host 未命中且 WSL 可用、WSL 内工具命中时走 `wsl.exe`（仅 Windows）。
5. **Docker 兜底**：Host 与 WSL 均无该工具时走 Docker，安全 fail closed。

Windows 原生工作区优先 Host，防止 `node_modules`、原生依赖与构建产物跨环境混用；Linux/WSL 工作区（路径以 `/home/`、`/mnt/` 开头）走 WSL。

| target | Windows | macOS/Linux |
|---|---|---|
| `auto` | 探测 Host PATH → 命中走 Win32 宿主；否则 WSL → Docker | 探测 Host PATH → 命中走原生宿主 shell；否则 Docker |
| `host` | `cmd.exe /C` 宿主执行 | 原生 `sh` 宿主执行 |
| `wsl` | `wsl.exe`，可选 `distro` | 不支持，拒绝并提示使用 `host` |
| `docker` | 使用配置的 Docker 沙箱 | 使用配置的 Docker 沙箱 |

## 沙箱模式与执行层的解耦

会话 `sandbox_mode`（`standard` / `workspace` / `trusted`）**不再决定执行目标**，只影响审批放宽（`relaxes_soft_approvals`）；自动选择器在这些模式下同样先探测 Host。仅 `isolated` 强制 Docker。Docker 分支选择沙箱模式时，`enabled` 用原值、否则 `standard`。

`target=auto` 不改变现有界面配置：会话中的 SandboxModeSwitcher 优先，其次是 Agent 的 `default_sandbox_mode`/旧 `sandbox` 默认值，最终写入 `sessions.sandbox_mode` 并由 exec 使用。显式 `target` 只改变运行目标；所有目标仍经过审批、危险命令、受保护路径、计划模式与无人值守门禁。

旧参数兼容：`sandbox=true` 在未指定 `target` 时请求 Docker；`sandbox=false` 不表示绕过会话沙箱。需要宿主机时应使用 `target=host`。

`host`/`wsl` 仅允许桌面 attended owner 执行面。server、ACP、cron、无人值守及受限子代理必须 fail closed。Windows 到 WSL 的 cwd 只接受可安全转换的本地盘路径；宿主路径与容器 `/workspace` 路径不得混用。

## 探测与日志

`command_on_path` / `wsl_available` / `wsl_tool_on_path`（`ha-base/src/platform`）只探测可执行文件是否存在，不读取、不记录环境变量、令牌或凭据。exec 结构化日志含 `probeResult`（如 `host:pnpm=true`），仅记录工具命中布尔，便于排查“为何走了 Docker”，不含路径以外的敏感信息。
