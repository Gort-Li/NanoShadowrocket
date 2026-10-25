// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 卸载程序在删除文件前调用：只还原代理设置，不启动界面。
    if std::env::args().skip(1).any(|arg| arg == "--restore-proxy") {
        let restored = app_lib::restore_proxy_from_disk();
        std::process::exit(if restored { 0 } else { 1 });
    }

    app_lib::run();
}
