use crate::state::DashGlobal;
use crate::{Open, Quit};
use gpui_kit::*;
use gpui_tray::{Icon, Tray};

struct TrayGlobal(Tray);
impl Global for TrayGlobal {}

pub fn init(cx: &mut App) {
    let png = Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/tray-template.png").to_vec(),
    );
    let icon = match Icon::from_gpui(&png, cx)
        .scale_factor(2.0)
        .template(true)
        .build()
    {
        Ok(icon) => icon,
        Err(error) => return tracing::warn!(%error, "tray icon unavailable"),
    };
    match Tray::builder()
        .icon(icon)
        .tooltip("DashBeam Lite")
        .menu(menu)
        .build(cx)
    {
        Ok(tray) => cx.set_global(TrayGlobal(tray)),
        Err(error) => tracing::warn!(%error, "system tray unavailable"),
    }
}

/// Rebuilds the menu after presence changes.
pub fn refresh(cx: &mut App) {
    if let Some(tray) = cx.try_global::<TrayGlobal>().map(|t| t.0.clone()) {
        let _ = tray.refresh_menu(cx);
    }
}

pub fn close(cx: &mut App) {
    if let Some(tray) = cx.try_global::<TrayGlobal>().map(|t| t.0.clone()) {
        let _ = tray.close(cx);
    }
}

fn menu(cx: &mut App) -> Vec<MenuItem> {
    let ((online, total), sends, receives) = cx
        .try_global::<DashGlobal>()
        .map(|d| {
            let dash = d.0.read(cx);
            (dash.presence(), dash.active_sends(), dash.active_receives())
        })
        .unwrap_or_default();
    let presence = match total {
        0 => "No paired devices".to_string(),
        _ => format!("{online} of {total} devices online"),
    };
    let activity = match (sends, receives) {
        (0, 0) => None,
        (s, 0) => Some(format!("Sending {s}")),
        (0, r) => Some(format!("Receiving {r}")),
        (s, r) => Some(format!("Sending {s} · Receiving {r}")),
    };
    let mut items = vec![MenuItem::action(presence, NoAction).disabled(true)];
    if let Some(activity) = activity {
        items.push(MenuItem::action(activity, NoAction).disabled(true));
    }
    items.extend([
        MenuItem::separator(),
        MenuItem::action("Open DashBeam Lite", Open),
        MenuItem::action("Quit", Quit),
    ]);
    items
}
