//! NanoShadowrocket 主入口：单实例、托盘常驻、后台任务与事件循环。

pub mod commands;
pub mod config;
pub mod error;
pub mod events;
pub mod proxy;
pub mod rules;
pub mod state;
pub mod sysproxy;
pub mod vmess;

use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

use crate::config::ConfigStore;
use crate::events::TrafficPayload;
use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|e| format!("无法定位配置目录：{e}"))?;
            let state =
                Arc::new(AppState::new(&config_dir).map_err(|e| format!("初始化失败：{e}"))?);
            app.manage(state.clone());

            setup_tray(app)?;
            spawn_traffic(app.handle().clone(), state.clone());
            spawn_remote_scheduler(app.handle().clone(), state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 常驻托盘：关闭窗口只是隐藏，退出请用托盘菜单。
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::get_status,
            commands::get_traffic,
            commands::start_proxy,
            commands::stop_proxy,
            commands::toggle_proxy,
            commands::update_settings,
            commands::clear_rule_cache,
            commands::reset_config,
            commands::nodes::preview_vmess_link,
            commands::nodes::add_node_from_link,
            commands::nodes::save_node,
            commands::nodes::delete_node,
            commands::nodes::select_node,
            commands::nodes::test_node_latency,
            commands::nodes::test_all_latencies,
            commands::rules::save_custom_rules,
            commands::rules::rules_summary,
            commands::rules::list_remote_rules,
            commands::rules::add_remote_rule,
            commands::rules::update_remote_rule,
            commands::rules::update_all_remote_rules,
            commands::rules::remove_remote_rule,
            commands::rules::set_remote_interval,
            commands::rules::set_remote_enabled,
            commands::rules::reorder_remote_rules,
            commands::rules::test_rule,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    // 退出路径必须同步还原，避免系统代理指向已关闭的端口。
                    state.restore_all();
                }
            }
        });
}

/// 卸载程序在删除文件前调用（`--restore-proxy`）：只读配置、还原代理设置，然后退出。
///
/// 卸载时不会走 [`AppState`] 的退出路径，若进程此前被强制结束，`proxy_dirty`
/// 与 `env_dirty` 会残留成指向已消失端口的死代理，用户会在卸载后直接断网。
///
/// 返回是否真的还原了某项设置；失败时保留 dirty 标记，重装后启动仍会自愈。
pub fn restore_proxy_from_disk() -> bool {
    let Some(dir) = config_dir() else {
        return false;
    };
    let Ok(store) = ConfigStore::load(&dir) else {
        return false;
    };
    let cfg = store.snapshot();
    let mut restored = false;

    if cfg.proxy_dirty {
        if let Some(snapshot) = cfg.proxy_snapshot.as_ref() {
            match sysproxy::restore(snapshot) {
                Ok(()) => {
                    let _ = store.update(|c| {
                        c.proxy_dirty = false;
                        c.proxy_snapshot = None;
                    });
                    restored = true;
                }
                Err(err) => log::warn!("卸载前还原系统代理失败：{err}"),
            }
        }
    }

    if cfg.env_dirty {
        if let Some(snapshot) = cfg.env_snapshot.as_ref() {
            match sysproxy::env_proxy::restore(snapshot) {
                Ok(()) => {
                    let _ = store.update(|c| {
                        c.env_dirty = false;
                        c.env_snapshot = None;
                    });
                    restored = true;
                }
                Err(err) => log::warn!("卸载前还原代理环境变量失败：{err}"),
            }
        }
    }

    restored
}

/// 配置目录：`%APPDATA%\{identifier}`，与 Tauri 的 `app_config_dir()` 保持一致。
fn config_dir() -> Option<std::path::PathBuf> {
    // 类型参数只能显式标注，宏展开的 `Context::new` 无法推断运行时。
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let identifier = context.config().identifier.clone();
    let appdata = std::env::var_os("APPDATA")?;
    Some(std::path::PathBuf::from(appdata).join(identifier))
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "开启 / 关闭代理", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 NanoShadowrocket", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("缺少应用图标".into()))?;

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("NanoShadowrocket")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "toggle" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<Arc<AppState>>().inner().clone();
                    let running = state.status().await.running;
                    let result = if running {
                        state.stop().await
                    } else {
                        state.start().await
                    };
                    match result {
                        Ok(status) => events::proxy(&app, &status),
                        Err(err) => log::warn!("托盘切换代理失败：{err}"),
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn spawn_traffic(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            state.stats.sample();
            let payload = TrafficPayload {
                stats: state.stats.snapshot(),
                running: state.status().await.running,
                recent: state.log.snapshot(),
            };
            events::traffic(&app, &payload);
        }
    });
}

fn spawn_remote_scheduler(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let due = rules::due_sources(&state.config, rules::now_secs());
            if due.is_empty() {
                continue;
            }
            for id in due {
                match state.update_remote(&id).await {
                    Ok(outcome) => events::remote(&app, &outcome),
                    Err(err) => log::warn!("定时更新远程规则失败：{err}"),
                }
            }
            events::rules(&app, &state.rules_summary());
        }
    });
}
