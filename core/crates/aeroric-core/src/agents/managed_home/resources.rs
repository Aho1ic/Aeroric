//! hook 脚本的**单一资源来源**(计划 Action 2:include_str! 的 hook 文件必须随
//! 代码移动,不维护两份会漂移的拷贝)。宿主 hooks.rs 与 omp_home.rs 经本模块
//! 消费脚本内容;改动脚本只动本目录下的文件。

pub const AERORIC_HOOK_SCRIPT: &str = include_str!("aeroric-hook.mjs");
