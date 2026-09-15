// 此文件由 Rust DTO 自动生成，请勿手工编辑。
// 更新：cargo run -p layerlens-core --example export_bindings

export const IPC_PROTOCOL_VERSION = 1;

export type AppInfoRequest = {
/**
 * 调用方使用的 IPC 契约版本。
 */
protocolVersion: number, };

export type AppInfo = {
/**
 * 面向用户的应用名称。
 */
appName: string,
/**
 * 当前应用版本，与桌面 crate 共用 workspace 版本。
 */
appVersion: string,
/**
 * 本次响应采用的 IPC 契约版本。
 */
protocolVersion: number, };

export type CommandErrorCode = "PROTOCOL_MISMATCH";

export type CommandError = {
/**
 * 稳定的机器可读错误代码。
 */
code: CommandErrorCode,
/**
 * 包含原因及恢复提示的用户消息。
 */
message: string, };
