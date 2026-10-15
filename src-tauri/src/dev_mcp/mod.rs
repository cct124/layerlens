//! 显式启用的开发测试 MCP；同一桌面实例、受控文件边界和有界协议资源。
//! 普通构建不编译此模块；业务规则仍属于核心及共享桌面入口。

mod config;
mod control;
mod fixtures;
mod model;
mod operations;
mod receipts;
mod server;
mod tools;
pub(crate) mod ui;
pub(crate) mod windows;

pub(crate) use config::Instance;
pub(crate) use server::{Running, start};

#[cfg(test)]
mod tests;
