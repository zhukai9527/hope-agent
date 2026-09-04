# Exec 执行目标

`exec` 的 `target` 用于区分宿主机、WSL 与 Docker。未传值时为 `auto`，由运行时根据工作目录、命令特征和沙箱策略自动选择，用户无需手动指定执行环境。

自动策略规则：显式 `target` 优先；`standard`、`workspace`、`trusted`、`isolated` 或显式沙箱请求始终走 Docker；Windows 原生工作区优先使用 Host，Linux/WSL 工作区可选择 WSL；`docker compose` / `docker-compose` 命令选择 Docker。自动选择不能跨环境复用 `node_modules`、原生依赖或构建产物。

| target | Windows | macOS/Linux |
|---|---|---|
| `auto` | 遵守会话/UI 默认沙箱；关闭时走 Win32 宿主 | 遵守会话/UI 默认沙箱；关闭时走原生宿主 shell |
| `host` | `cmd.exe /C` 宿主执行 | 原生 `sh` 宿主执行 |
| `wsl` | `wsl.exe`，可选 `distro` | 不支持，拒绝并提示使用 `host` |
| `docker` | 使用配置的 Docker 沙箱 | 使用配置的 Docker 沙箱 |

## 默认沙箱生效链

`target=auto` 不改变现有界面配置：会话中的 SandboxModeSwitcher 优先，其次是 Agent 的 `default_sandbox_mode`/旧 `sandbox` 默认值，最终写入 `sessions.sandbox_mode` 并由 exec 使用。显式 `target` 只改变运行目标；所有目标仍经过审批、危险命令、受保护路径、计划模式与无人值守门禁。

旧参数兼容：`sandbox=true` 在未指定 `target` 时请求 Docker；`sandbox=false` 不表示绕过会话沙箱。需要宿主机时应使用 `target=host`。

`host`/`wsl` 仅允许桌面 attended owner 执行面。server、ACP、cron、无人值守及受限子代理必须 fail closed。Windows 到 WSL 的 cwd 只接受可安全转换的本地盘路径；宿主路径与容器 `/workspace` 路径不得混用。
