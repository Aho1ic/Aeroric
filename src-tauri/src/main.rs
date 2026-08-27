// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // ssh 的 ProxyCommand 会以桥模式重新拉起本程序。必须在 Tauri 启动之前处理,
    // 桥进程不该开窗口,也不该初始化任何应用状态。
    if aeroric_lib::try_run_ssh_proxy_bridge() {
        return;
    }
    // 权限面板借一个新进程问系统当前记的授权(本进程的 TCC 判定是启动时缓存的)。
    // 同样必须在 Tauri 启动之前:探测进程不开窗口、不初始化应用状态。
    if aeroric_lib::try_run_permission_probe_bridge() {
        return;
    }
    aeroric_lib::run()
}
