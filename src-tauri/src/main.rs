// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // ssh 的 ProxyCommand 会以桥模式重新拉起本程序。必须在 Tauri 启动之前处理,
    // 桥进程不该开窗口,也不该初始化任何应用状态。
    if aeroric_lib::try_run_ssh_proxy_bridge() {
        return;
    }
    aeroric_lib::run()
}
