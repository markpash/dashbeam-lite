mod autostart;
mod backend;
mod settings;
mod state;
mod tray;
mod ui;
mod updater;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use state::{Dash, DashGlobal};

actions!(dashbeam_lite, [Open, Quit]);

/// Bundle identifier; also names the data folder and the login item.
pub const APP_ID: &str = "me.markpash.dashbeam-lite";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=warn,iroh_blobs=warn".into()),
        )
        .init();
    // reqwest (the updater) uses whatever rustls provider is installed;
    // ring is already in the build through iroh.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Launch-at-login starts us with `--hidden`: tray only, no window.
    let hidden = std::env::args().any(|arg| arg == "--hidden");
    // Any other arguments are paths to share right away.
    let paths: Vec<std::path::PathBuf> = std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .map(Into::into)
        .collect();
    let data_dir = dirs::data_dir()
        .expect("no Application Support directory")
        .join(APP_ID);
    std::fs::create_dir_all(&data_dir).expect("cannot create data directory");

    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    app.on_reopen(open_window);
    // Files dropped on the Dock icon or opened with the app arrive as URLs.
    let (opened_tx, mut opened) = tokio::sync::mpsc::unbounded_channel::<Vec<String>>();
    app.on_open_urls(move |urls| {
        let _ = opened_tx.send(urls);
    });
    app.run(move |cx| {
        gpui_kit::init(cx);
        let dash = cx.new(|cx| Dash::new(data_dir, cx));
        cx.set_global(DashGlobal(dash.clone()));
        let quitting = dash.clone();
        if !paths.is_empty() {
            dash.update(cx, |dash, cx| dash.start_share(paths, cx));
        }
        let sharing = dash.clone();
        cx.spawn(async move |cx| {
            while let Some(urls) = opened.recv().await {
                let paths: Vec<std::path::PathBuf> = urls
                    .iter()
                    .filter_map(|url| url::Url::parse(url).ok()?.to_file_path().ok())
                    .collect();
                if paths.is_empty() {
                    continue;
                }
                let dash = sharing.clone();
                cx.update(|cx| {
                    open_window(cx);
                    dash.update(cx, |dash, cx| dash.start_share(paths, cx));
                });
            }
        })
        .detach();

        cx.on_action(|_: &Open, cx| open_window(cx));
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus(vec![
            Menu::new("DashBeam Lite").items([MenuItem::action("Quit DashBeam Lite", Quit)])
        ]);
        cx.on_system_notification_response(move |response, cx| {
            match (
                response.tag.strip_prefix("invite:"),
                response.action_id.as_deref(),
            ) {
                (Some(id), Some(action)) => {
                    let id = id.to_string();
                    dash.update(cx, |dash, cx| {
                        dash.answer_invite(&id, action == "accept", cx)
                    });
                }
                _ => open_window(cx),
            }
        });

        // Every quit path (menu, tray, Dock, logout) ends here.
        cx.on_app_quit(move |cx| {
            quitting.update(cx, |dash, _| dash.shutdown());
            tray::close(cx);
            async {}
        })
        .detach();

        tray::init(cx);
        if !hidden {
            open_window(cx);
        }
    });
}

/// Focuses the window, creating it if it was closed. Closing drops every view,
/// so a backgrounded app keeps only the engine and the tray alive.
fn open_window(cx: &mut App) {
    cx.activate(true);
    if let Some(window) = cx.windows().first() {
        let _ = window.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(720.), px(560.)), cx)),
        window_min_size: Some(size(px(560.), px(440.))),
        ..TitleBar::window_options()
    };
    if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| ui::Shell::new(window, cx))
    }) {
        tracing::error!(%error, "failed to open window");
    }
}
