use crate::state::{
    format_size, items, Dash, DashGlobal, Download, DownloadStatus, Invite, NodeState, Progress,
    Recipient, Share, ShareStatus, Toast, Update,
};
use crate::{autostart, settings::Settings, updater};
use engine::{
    Discoverability, DiscoveryConfigArg, PairingStatus, RelayConfigArg, TransferDirection,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress as ProgressBar;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::TabBar;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, Theme, TitleBar,
    WindowExt,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::path::PathBuf;

const TABS: [&str; 5] = ["Send", "Receive", "Devices", "History", "Settings"];
const RECEIVE_TAB: usize = 1;
/// Not a tab: the screen behind the globe in the title bar.
const CONNECTIVITY: usize = TABS.len();

pub struct Shell {
    dash: Entity<Dash>,
    tab: usize,
    ticket: Entity<InputState>,
    pair_code: Entity<InputState>,
    device_name: Entity<InputState>,
    relay_urls: Entity<InputState>,
    relay_token: Entity<InputState>,
    pkarr_url: Entity<InputState>,
    dns_origin: Entity<InputState>,
    relay_mode: usize,
    relay_fallback: bool,
    discovery_custom: bool,
    /// Paired device being renamed inline, with its name field.
    renaming: Option<(String, Entity<InputState>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

fn input(window: &mut Window, cx: &mut App, placeholder: &str, value: &str) -> Entity<InputState> {
    let (placeholder, value) = (placeholder.to_string(), value.to_string());
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dash = cx.global::<DashGlobal>().0.clone();
        let settings = dash.read(cx).settings.clone();
        let device = dash
            .read(cx)
            .device
            .as_ref()
            .map(|d| d.display_name.clone())
            .unwrap_or_default();
        let relay = settings.relay.clone();
        let discovery = settings.discovery.clone();

        // gpui-component starts in light mode; follow the system instead.
        Theme::sync_system_appearance(Some(window), cx);
        let ticket = input(window, cx, "Paste a DashBeam ticket", "");
        let subscriptions = vec![
            cx.subscribe_in(
                &ticket,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.submit_ticket(window, cx);
                    }
                },
            ),
            cx.observe_window_appearance(window, |_, window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
            }),
            cx.observe_in(&dash, window, |this: &mut Self, dash, window, cx| {
                // The node may come up after the window opened.
                let name = dash
                    .read(cx)
                    .device
                    .as_ref()
                    .map(|d| d.display_name.clone());
                if let Some(name) = name.filter(|_| this.device_name.read(cx).value().is_empty()) {
                    this.device_name
                        .update(cx, |input, cx| input.set_value(name, window, cx));
                }
                cx.notify()
            }),
            cx.subscribe_in(&dash, window, |_, _, toast: &Toast, window, cx| {
                window.push_notification(toast.0.clone(), cx);
            }),
        ];

        Self {
            tab: 0,
            ticket,
            pair_code: input(window, cx, "Paste a pairing code", ""),
            device_name: input(window, cx, "Device name", &device),
            relay_urls: input(
                window,
                cx,
                "https://relay.example.com, …",
                &relay
                    .as_ref()
                    .map(|r| r.urls.join(", "))
                    .unwrap_or_default(),
            ),
            relay_token: input(
                window,
                cx,
                "Auth token (optional)",
                relay
                    .as_ref()
                    .and_then(|r| r.auth_token.as_deref())
                    .unwrap_or_default(),
            ),
            pkarr_url: input(
                window,
                cx,
                "https://pkarr.example.com",
                discovery
                    .as_ref()
                    .and_then(|d| d.pkarr_relay_url.as_deref())
                    .unwrap_or_default(),
            ),
            dns_origin: input(
                window,
                cx,
                "DNS origin (optional)",
                discovery
                    .as_ref()
                    .and_then(|d| d.dns_origin.as_deref())
                    .unwrap_or_default(),
            ),
            relay_mode: match relay.as_ref().map(|r| r.mode.as_str()) {
                Some("custom") => 1,
                Some("disabled") => 2,
                _ => 0,
            },
            relay_fallback: relay.as_ref().and_then(|r| r.fallback.as_deref()) == Some("public"),
            discovery_custom: discovery.is_some_and(|d| d.mode == "custom"),
            dash,
            renaming: None,
            _subscriptions: subscriptions,
        }
    }

    /// Starts a download from the ticket field, clearing it if that worked.
    fn submit_ticket(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ticket = self.ticket.read(cx).value().to_string();
        let started = self
            .dash
            .update(cx, |d, cx| d.receive(ticket, None, None, cx));
        if started {
            self.ticket.update(cx, |t, cx| t.set_value("", window, cx));
        }
    }

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Share".into()),
        });
        let dash = self.dash.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                dash.update(cx, |dash, cx| dash.start_share(paths, cx));
            }
        })
        .detach();
    }

    fn pick_download_dir(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Save here".into()),
        });
        let dash = self.dash.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(mut paths))) = paths.await {
                dash.update(cx, |dash, cx| {
                    dash.settings.download_dir = paths.pop();
                    dash.save_settings();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn start_rename(
        &mut self,
        endpoint_id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        input.update(cx, |input, cx| input.focus(window, cx));
        let enter = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.finish_rename(true, cx);
            }
        });
        self.renaming = Some((endpoint_id, input, enter));
        cx.notify();
    }

    fn finish_rename(&mut self, save: bool, cx: &mut Context<Self>) {
        let Some((endpoint_id, input, _)) = self.renaming.take() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if save && !name.is_empty() {
            self.dash
                .update(cx, |d, cx| d.rename_paired(endpoint_id, name, cx));
        }
        cx.notify();
    }

    fn apply_network(&mut self, cx: &mut Context<Self>) {
        let text = |state: &Entity<InputState>, cx: &App| state.read(cx).value().trim().to_string();
        let relay = match self.relay_mode {
            1 => Some(RelayConfigArg {
                mode: "custom".into(),
                urls: text(&self.relay_urls, cx)
                    .split([',', ' ', '\n'])
                    .filter(|u| !u.is_empty())
                    .map(str::to_string)
                    .collect(),
                auth_token: Some(text(&self.relay_token, cx)).filter(|t| !t.is_empty()),
                fallback: Some(
                    if self.relay_fallback {
                        "public"
                    } else {
                        "strict"
                    }
                    .into(),
                ),
            }),
            2 => Some(RelayConfigArg {
                mode: "disabled".into(),
                urls: Vec::new(),
                auth_token: None,
                fallback: None,
            }),
            _ => None,
        };
        let discovery = self.discovery_custom.then(|| DiscoveryConfigArg {
            mode: "custom".into(),
            pkarr_relay_url: Some(text(&self.pkarr_url, cx)),
            dns_origin: Some(text(&self.dns_origin, cx)).filter(|d| !d.is_empty()),
        });
        let mut settings: Settings = self.dash.read(cx).settings.clone();
        settings.relay = relay;
        settings.discovery = discovery;
        self.dash
            .update(cx, |dash, cx| dash.apply_network(settings, cx));
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, drop) = (theme.background, theme.foreground, theme.drop_target);
        // Badge the Send and Receive tabs with what's in progress.
        let (sends, receives) = {
            let dash = self.dash.read(cx);
            (dash.active_sends(), dash.active_receives())
        };
        let tab_labels = TABS
            .iter()
            .enumerate()
            .map(|(ix, name)| match (ix, sends, receives) {
                (0, n, _) | (RECEIVE_TAB, _, n) if n > 0 => format!("{name} · {n}"),
                _ => name.to_string(),
            });
        let content = match self.tab {
            0 => self.send(cx).into_any_element(),
            1 => self.receive(cx).into_any_element(),
            2 => self.devices(cx).into_any_element(),
            3 => self.history(cx).into_any_element(),
            CONNECTIVITY => self.connectivity(cx).into_any_element(),
            _ => self.settings(window, cx).into_any_element(),
        };
        v_flex()
            .relative()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .relative()
                        .w_full()
                        .justify_center()
                        .pr_20()
                        .child(
                            TabBar::new("tabs")
                                .segmented()
                                .when(self.tab < TABS.len(), |tabs| tabs.selected_index(self.tab))
                                .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                    this.tab = *ix;
                                    cx.notify();
                                }))
                                .children(tab_labels),
                        )
                        .child(
                            div()
                                .absolute()
                                .right_3()
                                .child(self.connectivity_button(cx)),
                        ),
                ),
            )
            .child(self.banners(cx))
            .child(
                div()
                    .id("content")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_5()
                    .child(content),
            )
            .children(self.receiver_picker(cx))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                let paths = paths.paths().to_vec();
                this.tab = 0;
                this.dash.update(cx, |dash, cx| dash.start_share(paths, cx));
            }))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(drop))
    }
}

// Building blocks

fn card(cx: &App) -> Div {
    v_flex()
        .gap_3()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
}

fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .text_lg()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn row() -> Div {
    h_flex().gap_2().items_center()
}

/// How a transfer row's progress reads at a glance.
#[derive(Clone, Copy)]
enum Tone {
    Active,
    Held,
    Done,
    Failed,
}

/// What one row of a transfer table shows.
struct TransferCells {
    id: usize,
    icon: IconName,
    name: String,
    /// Who it's to or from.
    peer: String,
    size: u64,
    /// `None` hides the bar (nothing measurable yet).
    fraction: Option<f32>,
    status: String,
    /// Speed and time left, or why it's stuck.
    detail: String,
    tone: Tone,
}

const COL_SIZE: f32 = 64.;
const COL_PROGRESS: f32 = 124.;
const COL_STATUS: f32 = 150.;
const COL_ACTIONS: f32 = 52.;

/// Column titles over a transfer table.
fn transfer_header(cx: &App) -> Div {
    let label = |text: &'static str| {
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    };
    h_flex()
        .gap_3()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().table_head)
        .child(div().w(px(16.)))
        .child(div().flex_1().min_w_0().child(label("Name")))
        .child(
            div()
                .w(px(COL_SIZE))
                .flex()
                .justify_end()
                .child(label("Size")),
        )
        .child(div().w(px(COL_PROGRESS)).child(label("Progress")))
        .child(div().w(px(COL_STATUS)).child(label("Status")))
        .child(div().w(px(COL_ACTIONS)))
}

/// A bordered table of transfer rows, or `empty` when there are none.
fn transfer_table(rows: Vec<Div>, empty: &'static str, cx: &App) -> Div {
    let table = v_flex()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden();
    if rows.is_empty() {
        return table.child(div().p_4().flex().justify_center().child(muted(empty, cx)));
    }
    table.child(transfer_header(cx)).children(rows)
}

/// One compact transfer row: two lines of text, like a torrent client.
fn transfer_row(cells: TransferCells, toggle: Button, remove: Button, cx: &App) -> Div {
    let theme = cx.theme();
    let small =
        |text: String, color: Hsla| div().text_xs().text_color(color).truncate().child(text);
    let bar_color = match cells.tone {
        Tone::Active => theme.progress_bar,
        Tone::Held => theme.warning,
        Tone::Done => theme.success,
        Tone::Failed => theme.danger,
    };
    let progress = match cells.fraction {
        Some(f) => h_flex()
            .gap_2()
            .child(
                div().flex_1().child(
                    ProgressBar::new(("bar", cells.id))
                        .color(bar_color)
                        .value(f * 100.0),
                ),
            )
            .child(
                div()
                    .w(px(34.))
                    .text_xs()
                    .text_right()
                    .child(format!("{:.0}%", (f * 100.0).floor())),
            ),
        None => h_flex().child(small("—".into(), theme.muted_foreground)),
    };
    let status_color = match cells.tone {
        Tone::Failed => theme.danger,
        _ => theme.foreground,
    };
    h_flex()
        .gap_3()
        .px_3()
        .py_1p5()
        .items_center()
        .border_b_1()
        .border_color(theme.table_row_border)
        .child(
            Icon::new(cells.icon)
                .small()
                .text_color(theme.muted_foreground),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().truncate().child(cells.name))
                .child(small(cells.peer, theme.muted_foreground)),
        )
        .child(
            div()
                .w(px(COL_SIZE))
                .text_sm()
                .text_right()
                .child(if cells.size > 0 {
                    format_size(cells.size)
                } else {
                    "—".into()
                }),
        )
        .child(div().w(px(COL_PROGRESS)).child(progress))
        .child(
            v_flex()
                .w(px(COL_STATUS))
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .truncate()
                        .text_color(status_color)
                        .child(cells.status),
                )
                .child(small(cells.detail, theme.muted_foreground)),
        )
        .child(
            h_flex()
                .w(px(COL_ACTIONS))
                .justify_end()
                .gap_1()
                .child(toggle)
                .child(remove),
        )
}

fn percent(p: &Progress) -> String {
    format!("{:.0}%", (p.fraction() * 100.0).floor())
}

/// Speed, then time left when known.
fn rate(p: &Progress) -> String {
    let speed = format!("{}/s", format_size(p.speed_bps as u64));
    match p.eta() {
        Some(eta) => format!("{speed} · {}", eta.trim_end_matches(" left")),
        None => speed,
    }
}

#[derive(Clone, Copy)]
enum Check {
    Pass,
    Warn,
    Fail,
    Pending,
    Info,
}

fn check_row(check: Check, title: &str, detail: impl Into<SharedString>, cx: &App) -> Div {
    let theme = cx.theme();
    let (icon, color) = match check {
        Check::Pass => (IconName::CircleCheck, theme.success),
        Check::Warn => (IconName::TriangleAlert, theme.warning),
        Check::Fail => (IconName::CircleX, theme.danger),
        Check::Pending => (IconName::LoaderCircle, theme.muted_foreground),
        Check::Info => (IconName::Info, theme.muted_foreground),
    };
    h_flex()
        .gap_3()
        .items_start()
        .child(div().pt(px(2.)).child(Icon::new(icon).text_color(color)))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(title.to_string())
                .child(muted(detail, cx)),
        )
}

/// Page title, with "Clear finished" once more than one item is done.
fn list_header(title: &str, finished: usize, sends: bool, cx: &Context<Shell>) -> Div {
    row()
        .child(div().flex_1().child(heading(title.to_string())))
        .when(finished > 1, |row| {
            row.child(
                Button::new("clear-finished")
                    .ghost()
                    .small()
                    .label("Clear finished")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.dash.update(cx, |d, cx| d.clear_finished(sends, cx))
                    })),
            )
        })
}

fn device_label(name: Option<&str>, endpoint_id: &str) -> String {
    name.map(str::to_string)
        .unwrap_or_else(|| format!("{}…", &endpoint_id[..8.min(endpoint_id.len())]))
}

impl Shell {
    /// Title-bar globe, tinted by the relay this device reaches. Opens the
    /// Connectivity screen.
    fn connectivity_button(&self, cx: &mut Context<Self>) -> Button {
        let theme = cx.theme();
        let color = match &self.dash.read(cx).relay {
            None => theme.muted_foreground.opacity(0.5),
            Some(s) if s.connected && s.fell_back_to_public => theme.warning,
            Some(s) if s.connected && s.kind == "custom" => theme.blue,
            Some(s) if s.connected => theme.muted_foreground,
            Some(_) => theme.muted_foreground.opacity(0.4),
        };
        Button::new("connectivity")
            .ghost()
            .small()
            .selected(self.tab == CONNECTIVITY)
            .tooltip("Connectivity")
            .icon(Icon::new(IconName::Globe).text_color(color))
            .on_click(cx.listener(|this, _, _, cx| {
                this.tab = CONNECTIVITY;
                this.dash.update(cx, |d, cx| d.check_connectivity(cx));
                cx.notify();
            }))
    }

    fn banners(&mut self, cx: &mut Context<Self>) -> Div {
        let dash = self.dash.read(cx);
        let any = !dash.invites.is_empty()
            || dash.verifying.is_some()
            || !matches!(dash.update, Update::None);
        let mut banners = v_flex().gap_2().px_5().when(any, |b| b.pt_4());
        if let Some(invite) = dash.invites.first() {
            let id = invite.endpoint_id().to_string();
            let text = match invite {
                Invite::Files {
                    sender,
                    file_count,
                    total_size,
                    paired,
                    ..
                } => format!(
                    "{sender} wants to send you {} ({}){}",
                    items(*file_count),
                    format_size(*total_size),
                    if *paired {
                        String::new()
                    } else {
                        format!(" · code {}", invite.code())
                    }
                ),
                Invite::Pair {
                    sender,
                    device_type,
                    ..
                } => format!(
                    "{sender} ({device_type}) wants to pair · code {}",
                    invite.code()
                ),
            };
            let (accept_id, decline_id) = (id.clone(), id);
            let files = matches!(invite, Invite::Files { .. });
            banners =
                banners.child(
                    card(cx)
                        .flex_row()
                        .items_center()
                        .child(div().flex_1().min_w_0().child(text))
                        .child(
                            Button::new("decline")
                                .label("Decline")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.dash
                                        .update(cx, |d, cx| d.answer_invite(&decline_id, false, cx))
                                })),
                        )
                        .child(Button::new("accept").primary().label("Accept").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.dash
                                    .update(cx, |d, cx| d.answer_invite(&accept_id, true, cx));
                                if files {
                                    // Show the download that just started.
                                    this.tab = RECEIVE_TAB;
                                }
                            }),
                        )),
                );
        }
        if let Some((_, code)) = &dash.verifying {
            banners = banners.child(
                card(cx)
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(format!("Check the other device shows this code: {code}")),
                    )
                    .child(
                        Button::new("verified")
                            .label("Dismiss")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dash.update(cx, |d, cx| {
                                    d.verifying = None;
                                    cx.notify();
                                })
                            })),
                    ),
            );
        }
        match &dash.update {
            Update::None => {}
            Update::Available(release) => {
                let page = release.page_url.clone();
                banners = banners.child(
                    card(cx)
                        .flex_row()
                        .items_center()
                        .child(div().flex_1().min_w_0().child(format!(
                            "DashBeam Lite build {} is available.",
                            release.build
                        )))
                        .child(
                            Button::new("whats-new")
                                .ghost()
                                .label("What's new")
                                .on_click(move |_, _, cx| cx.open_url(&page)),
                        )
                        .child(
                            Button::new("install-update")
                                .primary()
                                .label("Install and restart")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dash.update(cx, |d, cx| d.install_update(cx))
                                })),
                        ),
                );
            }
            Update::Installing(build) => {
                banners = banners.child(
                    card(cx)
                        .flex_row()
                        .items_center()
                        .child(div().flex_1().child(format!("Installing build {build}…")))
                        .child(Button::new("installing").label("Installing").loading(true)),
                );
            }
        }
        banners
    }

    fn send(&mut self, cx: &mut Context<Self>) -> Div {
        let dash = self.dash.read(cx);
        let drop_bar = h_flex()
            .gap_3()
            .items_center()
            .justify_center()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_2()
            .border_dashed()
            .border_color(cx.theme().border)
            .child(muted("Drop files or folders here, or", cx))
            .child(
                Button::new("choose")
                    .primary()
                    .small()
                    .label("Choose…")
                    .on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
            );
        let finished = dash.shares.iter().filter(|s| !s.active()).count();
        let rows: Vec<Div> = dash
            .shares
            .iter()
            .map(|share| self.share_row(share, cx))
            .collect();
        v_flex()
            .gap_3()
            .child(list_header("Send", finished, true, cx))
            .child(drop_bar)
            .child(transfer_table(rows, "Nothing sent yet", cx))
    }

    fn share_row(&self, share: &Share, cx: &Context<Self>) -> Div {
        let tag = share.tag;
        let id = tag as usize;
        let choosing = self.dash.read(cx).choosing.contains(&tag);
        let to = match &share.recipient {
            Some(Recipient::Device { name, .. }) => format!("To {name}"),
            Some(Recipient::Ticket) => "To whoever uses the ticket".to_string(),
            None => "No receiver yet".to_string(),
        };
        let p = &share.progress;
        let (status, detail): (String, String) = match share.status {
            ShareStatus::Preparing if choosing => ("Choose receiver".into(), "Preparing…".into()),
            ShareStatus::Preparing => ("Preparing".into(), percent(p)),
            _ if share.paused && share.active() => ("Paused".into(), String::new()),
            ShareStatus::Waiting if choosing => ("Choose receiver".into(), String::new()),
            ShareStatus::Waiting => (
                "Waiting".into(),
                match (&share.recipient, &share.invite) {
                    (_, Some((_, text))) => text.to_string(),
                    (Some(Recipient::Ticket), _) => "Ticket copied".into(),
                    _ => String::new(),
                },
            ),
            ShareStatus::Transferring => ("Sending".into(), rate(p)),
            ShareStatus::Interrupted => ("Disconnected".into(), "Waiting for the receiver".into()),
            ShareStatus::Done => ("Sent".into(), String::new()),
        };
        let size = share.meta.as_ref().map(|m| m.size).unwrap_or(p.total);
        let fraction = match share.status {
            ShareStatus::Done => Some(1.0),
            ShareStatus::Transferring | ShareStatus::Interrupted => Some(p.fraction()),
            ShareStatus::Preparing if p.total > 0 => Some(p.fraction()),
            _ => None,
        };
        let paused = share.paused;
        let toggle = Button::new(("toggle", id))
            .ghost()
            .xsmall()
            .icon(if paused {
                IconName::Play
            } else {
                IconName::Pause
            })
            .tooltip(if paused { "Resume" } else { "Pause" })
            .disabled(!share.can_pause())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dash
                    .update(cx, |d, cx| d.pause_share(tag, !paused, cx))
            }));
        let remove = Button::new(("remove", id))
            .ghost()
            .xsmall()
            .icon(IconName::Close)
            .tooltip("Remove")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dash.update(cx, |d, cx| d.stop_share(tag, cx))
            }));
        transfer_row(
            TransferCells {
                id,
                icon: IconName::ArrowUp,
                name: share.title.clone(),
                peer: to,
                size,
                fraction,
                status,
                detail,
                tone: match (share.status, paused) {
                    (ShareStatus::Done, _) => Tone::Done,
                    (_, true) | (ShareStatus::Interrupted, _) => Tone::Held,
                    _ => Tone::Active,
                },
            },
            toggle,
            remove,
            cx,
        )
    }

    fn receive(&mut self, cx: &mut Context<Self>) -> Div {
        let dash = self.dash.read(cx);
        let folder = dash.settings.download_dir();
        let error = dash.ticket_error.clone();
        let form = v_flex()
            .gap_1()
            .child(
                row()
                    .child(div().flex_1().child(Input::new(&self.ticket).small()))
                    .child(
                        Button::new("receive")
                            .primary()
                            .small()
                            .label("Receive")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit_ticket(window, cx)),
                            ),
                    ),
            )
            .child(match error {
                Some(e) => div().text_xs().text_color(cx.theme().danger).child(e),
                None => div()
                    .text_xs()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("Saving to {}", folder.display())),
            });
        let finished = dash.downloads.iter().filter(|d| d.finished()).count();
        let rows: Vec<Div> = dash
            .downloads
            .iter()
            .map(|download| self.download_row(download, cx))
            .collect();
        v_flex()
            .gap_3()
            .child(list_header("Receive", finished, false, cx))
            .child(form)
            .child(transfer_table(rows, "Nothing received yet", cx))
    }

    fn download_row(&self, download: &Download, cx: &Context<Self>) -> Div {
        let tag = download.tag;
        let id = tag as usize;
        let p = &download.progress;
        let from = match download.peer.as_ref().and_then(|p| p.display_name.clone()) {
            Some(name) => format!("From {name}"),
            None => "From a ticket".to_string(),
        };
        let (status, detail): (String, String) = match &download.status {
            DownloadStatus::Running if download.reconnecting => {
                ("Reconnecting".into(), "Lost the sender".into())
            }
            DownloadStatus::Running if p.total == 0 => ("Connecting".into(), String::new()),
            DownloadStatus::Running => ("Receiving".into(), rate(p)),
            DownloadStatus::Paused => ("Paused".into(), String::new()),
            DownloadStatus::Done if download.conflicts > 0 => (
                "Received".into(),
                format!("{} renamed", items(download.conflicts)),
            ),
            DownloadStatus::Done => ("Received".into(), String::new()),
            DownloadStatus::Failed(error) => ("Failed".into(), error.clone()),
        };
        let fraction = match download.status {
            DownloadStatus::Done => Some(1.0),
            _ if p.total > 0 => Some(p.fraction()),
            _ => None,
        };
        let resumable = matches!(
            download.status,
            DownloadStatus::Paused | DownloadStatus::Failed(_)
        );
        let toggle = Button::new(("toggle", id))
            .ghost()
            .xsmall()
            .icon(if resumable {
                IconName::Play
            } else {
                IconName::Pause
            })
            .tooltip(if resumable { "Resume" } else { "Pause" })
            .disabled(!resumable && !download.can_pause())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dash.update(cx, |d, cx| {
                    if resumable {
                        d.resume_download(tag, cx)
                    } else {
                        d.pause_download(tag, cx)
                    }
                })
            }));
        let remove = Button::new(("remove", id))
            .ghost()
            .xsmall()
            .icon(IconName::Close)
            .tooltip("Remove")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dash.update(cx, |d, cx| d.remove_download(tag, cx))
            }));
        transfer_row(
            TransferCells {
                id,
                icon: IconName::ArrowDown,
                name: download
                    .name
                    .clone()
                    .unwrap_or_else(|| "Incoming files".into()),
                peer: from,
                size: p.total,
                fraction,
                status,
                detail,
                tone: match download.status {
                    DownloadStatus::Done => Tone::Done,
                    DownloadStatus::Failed(_) => Tone::Failed,
                    DownloadStatus::Paused => Tone::Held,
                    DownloadStatus::Running if download.reconnecting => Tone::Held,
                    DownloadStatus::Running => Tone::Active,
                },
            },
            toggle,
            remove,
            cx,
        )
    }

    /// Asks who the oldest share without a receiver is for. Covers the window
    /// so nothing else happens until it's answered.
    fn receiver_picker(&self, cx: &Context<Self>) -> Option<Stateful<Div>> {
        let dash = self.dash.read(cx);
        let tag = *dash.choosing.first()?;
        let share = dash.shares.iter().find(|s| s.tag == tag)?;
        let devices: Vec<(String, String, &'static str)> = dash
            .paired
            .iter()
            .filter(|d| d.online && d.pairing_status == PairingStatus::Active)
            .map(|d| (d.endpoint_id.clone(), d.display_name.clone(), "Paired"))
            .chain(dash.nearby.iter().filter(|d| d.identified).map(|d| {
                (
                    d.endpoint_id.clone(),
                    device_label(d.display_name.as_deref(), &d.endpoint_id),
                    "Nearby",
                )
            }))
            .collect();
        let theme = cx.theme();
        let mut list = v_flex().gap_1();
        if devices.is_empty() {
            list = list.child(muted(
                "No devices online. Pair one in Devices, or send a ticket.",
                cx,
            ));
        }
        for (ix, (endpoint_id, name, kind)) in devices.into_iter().enumerate() {
            let recipient = Recipient::Device {
                endpoint_id,
                name: name.clone(),
            };
            list = list.child(
                Button::new(("receiver", ix))
                    .ghost()
                    .w_full()
                    .justify_start()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(div().flex_1().text_left().child(name))
                            .child(muted(kind, cx)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let recipient = recipient.clone();
                        this.dash
                            .update(cx, |d, cx| d.choose_receiver(tag, recipient, cx))
                    })),
            );
        }
        let size = share
            .meta
            .as_ref()
            .map(|m| format!(" ({})", format_size(m.size)))
            .unwrap_or_default();
        Some(
            div()
                .id("receiver-picker")
                .absolute()
                .inset_0()
                .occlude()
                .bg(hsla(0., 0., 0., 0.55))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    card(cx)
                        .bg(theme.popover)
                        .w(px(380.))
                        .max_w_full()
                        .child(heading(format!("Send {}{size} to…", share.title)))
                        .child(list)
                        .child(
                            row()
                                .child(
                                    h_flex().flex_1().child(
                                        Button::new("use-ticket")
                                            .ghost()
                                            .label("Copy a ticket")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.dash.update(cx, |d, cx| {
                                                    d.choose_receiver(tag, Recipient::Ticket, cx)
                                                })
                                            })),
                                    ),
                                )
                                .child(Button::new("cancel-send").label("Cancel").on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.dash.update(cx, |d, cx| d.stop_share(tag, cx))
                                    }),
                                )),
                        ),
                ),
        )
    }

    fn devices(&mut self, cx: &mut Context<Self>) -> Div {
        let dash = self.dash.read(cx);
        let page = v_flex().gap_4();
        let node_error = match &dash.node {
            NodeState::Starting => Some("Starting device networking…".to_string()),
            NodeState::Failed(e) => Some(e.clone()),
            NodeState::Ready(_) => None,
        };
        if let Some(error) = node_error {
            return page.child(heading("Devices")).child(muted(error, cx));
        }

        let code = dash
            .device
            .as_ref()
            .and_then(|d| engine::short_fingerprint(&d.endpoint_id))
            .unwrap_or_default();
        let this_device = card(cx)
            .child(heading("This device"))
            .child(
                row()
                    .child(div().flex_1().child(Input::new(&self.device_name)))
                    .child(Button::new("rename").label("Rename").on_click(cx.listener(
                        |this, _, _, cx| {
                            let name = this.device_name.read(cx).value().trim().to_string();
                            this.dash.update(cx, |d, cx| d.rename_device(name, cx))
                        },
                    ))),
            )
            .child(muted(format!("Verification code {code}"), cx));

        let pairing = match dash.pairing_code.clone() {
            Some(code) => card(cx)
                .child(heading("Pair a device"))
                .child(muted(
                    "Enter this code on the other device. It expires in 3 minutes.",
                    cx,
                ))
                .child(div().text_xs().font_family("Menlo").child(code.clone()))
                .child(
                    row()
                        .justify_end()
                        .child(
                            Button::new("cancel-pair")
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dash.update(cx, |d, cx| d.close_pairing(cx))
                                })),
                        )
                        .child(
                            Button::new("copy-code")
                                .primary()
                                .label("Copy code")
                                .on_click(move |_, window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                                    window.push_notification("Pairing code copied", cx);
                                }),
                        ),
                ),
            None => card(cx)
                .child(heading("Pair a device"))
                .child(
                    row()
                        .child(div().flex_1().child(Input::new(&self.pair_code)))
                        .child(Button::new("join").label("Pair").on_click(cx.listener(
                            |this, _, window, cx| {
                                let code = this.pair_code.read(cx).value().to_string();
                                this.pair_code
                                    .update(cx, |t, cx| t.set_value("", window, cx));
                                this.dash.update(cx, |d, cx| d.join_pairing(code, cx))
                            },
                        ))),
                )
                .child(
                    row().child(Button::new("host").label("Show my pairing code").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.dash.update(cx, |d, cx| d.open_pairing(cx))
                        }),
                    )),
                ),
        };

        let mut paired = card(cx).child(heading("Paired devices"));
        if dash.paired.is_empty() {
            paired = paired.child(muted("No paired devices yet", cx));
        }
        for (ix, device) in dash.paired.iter().enumerate() {
            let status = match device.pairing_status {
                PairingStatus::Active if device.online => "Online",
                PairingStatus::Active => "Offline",
                PairingStatus::UnpairedRemotely => "Unpaired",
                PairingStatus::StaleLocalIdentity => "Re-pair required",
            };
            let editing = self
                .renaming
                .as_ref()
                .filter(|(id, _, _)| id.eq_ignore_ascii_case(&device.endpoint_id))
                .map(|(_, input, _)| input.clone());
            if let Some(input) = editing {
                paired = paired.child(
                    row()
                        .child(div().flex_1().child(Input::new(&input)))
                        .child(
                            Button::new(("cancel-rename", ix))
                                .ghost()
                                .label("Cancel")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.finish_rename(false, cx)),
                                ),
                        )
                        .child(
                            Button::new(("save-rename", ix))
                                .primary()
                                .label("Save")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.finish_rename(true, cx)),
                                ),
                        ),
                );
                continue;
            }
            let (trust_id, forget_id) = (device.endpoint_id.clone(), device.endpoint_id.clone());
            let (rename_id, name) = (device.endpoint_id.clone(), device.display_name.clone());
            paired = paired.child(
                row()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(device.display_name.clone())
                            .child(muted(format!("{} · {status}", device.device_type), cx)),
                    )
                    .child(
                        Switch::new(("trust", ix))
                            .label("Auto-accept")
                            .checked(device.trusted)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                let (id, trust) = (trust_id.clone(), *checked);
                                this.dash.update(cx, |d, cx| d.trust(id, trust, cx))
                            })),
                    )
                    .child(
                        Button::new(("rename", ix))
                            .ghost()
                            .label("Rename")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_rename(rename_id.clone(), name.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new(("forget", ix))
                            .ghost()
                            .label("Forget")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = forget_id.clone();
                                this.dash.update(cx, |d, cx| d.forget(id, cx))
                            })),
                    ),
            );
        }

        let mut nearby = card(cx).child(heading("Nearby"));
        if let Some(reason) = &dash.nearby_unavailable {
            nearby = nearby.child(muted(format!("Nearby discovery unavailable: {reason}"), cx));
        } else if dash.nearby.is_empty() {
            nearby = nearby.child(muted("Looking for devices on this network…", cx));
        }
        for (ix, device) in dash.nearby.iter().enumerate() {
            let id = device.endpoint_id.clone();
            nearby = nearby.child(
                row()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(device_label(
                                device.display_name.as_deref(),
                                &device.endpoint_id,
                            ))
                            .child(muted(
                                format!(
                                    "{} · code {}",
                                    device.device_type.as_deref().unwrap_or("Unknown device"),
                                    device.fingerprint
                                ),
                                cx,
                            )),
                    )
                    .child(
                        Button::new(("pair", ix))
                            .label("Pair")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = id.clone();
                                this.dash.update(cx, |d, cx| d.pair_nearby(id, cx))
                            })),
                    ),
            );
        }

        page.child(this_device)
            .child(pairing)
            .child(paired)
            .child(nearby)
    }

    fn connectivity(&mut self, cx: &mut Context<Self>) -> Div {
        let dash = self.dash.read(cx);
        let checking = dash.connectivity_checking;
        let page = v_flex().gap_4().child(
            row()
                .child(div().flex_1().child(heading("Connectivity")))
                .child(
                    Button::new("recheck-connectivity")
                        .label("Check again")
                        .loading(checking)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.dash.update(cx, |d, cx| d.check_connectivity(cx))
                        })),
                ),
        );
        let node = match &dash.node {
            NodeState::Starting => Err("Device networking is starting…".to_string()),
            NodeState::Failed(e) => Err(format!("Device networking failed: {e}")),
            NodeState::Ready(_) => Ok(()),
        };
        if let Err(message) = node {
            return page.child(card(cx).child(check_row(
                Check::Fail,
                "Device networking",
                message,
                cx,
            )));
        }
        let Some(report) = &dash.connectivity else {
            return page.child(card(cx).child(check_row(
                Check::Pending,
                "Checking your network",
                "This takes a few seconds.",
                cx,
            )));
        };
        let net = &report.net;
        let relays_off = dash
            .settings
            .relay
            .as_ref()
            .is_some_and(|r| r.mode == "disabled");
        let udp = net.udp_v4 || net.udp_v6;

        let (summary, summary_text) = if !net.report_ready {
            (
                Check::Pending,
                "Still measuring your network. Check again in a moment.",
            )
        } else if udp && net.nat_varies == Some(false) {
            (
                Check::Pass,
                "Good. Other devices can usually connect to this one directly.",
            )
        } else if net.relay_connected {
            (
                Check::Warn,
                "Limited. Connections will often go through a relay, which is slower.",
            )
        } else {
            (
                Check::Fail,
                "Not reachable from the internet. Only devices on your local network can connect.",
            )
        };

        let mut list = card(cx);
        if !net.report_ready {
            list = list.child(check_row(Check::Pending, "Internet", "Measuring…", cx));
        } else {
            let udp_detail = match (net.udp_v4, net.udp_v6) {
                (true, true) => "UDP works over IPv4 and IPv6.",
                (true, false) => "UDP works over IPv4. No IPv6.",
                (false, true) => "UDP works over IPv6 only.",
                (false, false) => "UDP looks blocked, so transfers must go through a relay.",
            };
            list = list.child(check_row(
                if udp { Check::Pass } else { Check::Fail },
                "Direct internet (UDP)",
                udp_detail,
                cx,
            ));
            let public: Vec<&str> = [&net.public_v4, &net.public_v6]
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect();
            list = list.child(if public.is_empty() {
                check_row(Check::Warn, "Public address", "None found.", cx)
            } else {
                check_row(Check::Pass, "Public address", public.join("  "), cx)
            });
            list = list.child(match net.nat_varies {
                Some(false) => check_row(
                    Check::Pass,
                    "NAT",
                    "Your router keeps the same public port, so hole punching should work.",
                    cx,
                ),
                Some(true) => check_row(
                    Check::Warn,
                    "NAT",
                    "Your router picks a new public port per destination (hard NAT). Direct connections may fail and fall back to a relay.",
                    cx,
                ),
                None => check_row(Check::Info, "NAT", "Couldn't determine the NAT type.", cx),
            });
            if net.captive_portal == Some(true) {
                list = list.child(check_row(
                    Check::Fail,
                    "Captive portal",
                    "Something (like a Wi-Fi login page) is intercepting web traffic. Sign in to the network first.",
                    cx,
                ));
            }
        }

        list = list.child(if relays_off {
            check_row(
                Check::Info,
                "Relay",
                "Relays are turned off in Settings.",
                cx,
            )
        } else if dash.relay.as_ref().is_some_and(|r| r.fell_back_to_public) {
            check_row(
                Check::Warn,
                "Relay",
                "Your custom relay is unreachable, so this device is using a public relay.",
                cx,
            )
        } else if net.relay_connected {
            let latency = net
                .relay_latency_ms
                .map(|ms| format!(" ({ms} ms)"))
                .unwrap_or_default();
            check_row(
                Check::Pass,
                "Relay",
                format!(
                    "Connected to {}{latency}",
                    net.home_relay.as_deref().unwrap_or("a relay")
                ),
                cx,
            )
        } else {
            let reason = net
                .relay_error
                .clone()
                .unwrap_or_else(|| "Not connected yet.".into());
            check_row(Check::Fail, "Relay", reason, cx)
        });

        let custom_discovery = dash
            .settings
            .discovery
            .as_ref()
            .is_some_and(|d| d.mode == "custom");
        list = list.child(match &report.findable {
            None => check_row(Check::Pending, "Findable by ID", "Looking this device up by its ID…", cx),
            Some(Ok(())) => check_row(
                Check::Pass,
                "Findable by ID",
                if custom_discovery {
                    "Your discovery server is reachable, so paired devices can find this one."
                } else {
                    "Paired devices anywhere can look this device up by its ID."
                },
                cx,
            ),
            Some(Err(error)) => check_row(
                Check::Warn,
                "Findable by ID",
                format!("Lookup failed: {error}. Paired devices may not find this one outside your network."),
                cx,
            ),
        });

        list = list.child(
            match (&dash.nearby_unavailable, dash.settings.discoverability) {
                (Some(reason), _) => check_row(
                    Check::Fail,
                    "Local network",
                    format!("Nearby is unavailable: {reason}"),
                    cx,
                ),
                (None, Discoverability::Off) => check_row(
                    Check::Info,
                    "Local network",
                    "Hidden from Nearby (Settings).",
                    cx,
                ),
                (None, Discoverability::PairedOnly) => check_row(
                    Check::Pass,
                    "Local network",
                    "Visible on this network to paired devices.",
                    cx,
                ),
                (None, Discoverability::Everyone) => check_row(
                    Check::Pass,
                    "Local network",
                    "Visible on this network to everyone.",
                    cx,
                ),
            },
        );

        if !net.local_addrs.is_empty() {
            list = list.child(check_row(
                Check::Info,
                "Local addresses",
                net.local_addrs.join("  "),
                cx,
            ));
        }

        page.child(card(cx).child(check_row(summary, "Overall", summary_text, cx)))
            .child(list)
            .child(muted(
                "Relays only help devices find each other and connect. Once a direct \
                 connection is up, or on the same network, files go straight between \
                 devices. Transfers are end-to-end encrypted, so relays never see your files.",
                cx,
            ))
    }

    fn history(&mut self, cx: &mut Context<Self>) -> Div {
        let records = self.dash.read(cx).history();
        let mut list = v_flex().gap_2();
        if records.is_empty() {
            list = list.child(muted("Nothing sent or received yet", cx));
        }
        for (ix, record) in records.iter().enumerate() {
            let arrow = match record.direction {
                TransferDirection::Send => "↑",
                TransferDirection::Receive => "↓",
            };
            let peer = record
                .peer
                .as_ref()
                .and_then(|p| p.display_name.clone())
                .map(|name| format!(" · {name}"))
                .unwrap_or_default();
            let target = record.save_path.as_ref().map(|dir| {
                let path = PathBuf::from(dir);
                match record.file_names.as_slice() {
                    [one] => path.join(one),
                    _ => path,
                }
            });
            let id = record.id.clone();
            list = list.child(
                row()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(div().w_5().child(arrow))
                    .child(
                        v_flex()
                            .flex_1()
                            .overflow_hidden()
                            .child(div().truncate().child(record.root_name.clone()))
                            .child(muted(
                                format!(
                                    "{} · {:?}{peer} · {}",
                                    format_size(record.payload_bytes),
                                    record.status,
                                    ago(record.started_at)
                                ),
                                cx,
                            )),
                    )
                    .children(target.filter(|p| p.exists()).map(|path| {
                        Button::new(("open", ix))
                            .ghost()
                            .label("Show")
                            .on_click(move |_, _, cx| cx.reveal_path(&path))
                    }))
                    .child(
                        Button::new(("delete", ix))
                            .ghost()
                            .label("Remove")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = id.clone();
                                this.dash.update(cx, |d, cx| d.delete_record(&id, cx))
                            })),
                    ),
            );
        }
        v_flex()
            .gap_4()
            .child(
                row().child(div().flex_1().child(heading("History"))).child(
                    Button::new("clear")
                        .ghost()
                        .label("Clear all")
                        .disabled(records.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.dash.update(cx, |d, cx| d.clear_history(cx))
                        })),
                ),
            )
            .child(list)
    }

    fn settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let dash = self.dash.read(cx);
        let folder = dash.settings.download_dir();
        let discoverability = match dash.settings.discoverability {
            Discoverability::Everyone => 0,
            Discoverability::PairedOnly => 1,
            Discoverability::Off => 2,
        };

        let general = card(cx)
            .child(heading("General"))
            .child(
                row()
                    .child(
                        v_flex()
                            .flex_1()
                            .child("Download folder")
                            .child(muted(folder.display().to_string(), cx)),
                    )
                    .child(
                        Button::new("folder")
                            .label("Change…")
                            .on_click(cx.listener(|this, _, _, cx| this.pick_download_dir(cx))),
                    ),
            )
            .child(
                Switch::new("autostart")
                    .label("Start DashBeam Lite when I sign in")
                    .checked(autostart::is_enabled())
                    .on_click(cx.listener(|_, checked: &bool, window, cx| {
                        if let Err(error) = autostart::set(*checked) {
                            window.push_notification(error.to_string(), cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child("Who can see this device nearby")
                    .child(
                        TabBar::new("discoverability")
                            .segmented()
                            .selected_index(discoverability)
                            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                let setting = match ix {
                                    0 => Discoverability::Everyone,
                                    1 => Discoverability::PairedOnly,
                                    _ => Discoverability::Off,
                                };
                                this.dash
                                    .update(cx, |d, cx| d.set_discoverability(setting, cx))
                            }))
                            .children(["Everyone", "Paired devices", "No one"]),
                    ),
            );

        let mut relay = card(cx).child(heading("Relay servers")).child(
            TabBar::new("relay-mode")
                .segmented()
                .selected_index(self.relay_mode)
                .on_click(cx.listener(|this, ix: &usize, _, cx| {
                    this.relay_mode = *ix;
                    cx.notify();
                }))
                .children(["Public", "Custom", "Disabled"]),
        );
        if self.relay_mode == 1 {
            relay = relay
                .child(Input::new(&self.relay_urls))
                .child(Input::new(&self.relay_token).mask_toggle())
                .child(
                    Switch::new("fallback")
                        .label("Use public relays if mine are unreachable")
                        .checked(self.relay_fallback)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.relay_fallback = *checked;
                            cx.notify();
                        })),
                );
        }
        relay = relay.child(
            Switch::new("discovery")
                .label("Custom discovery server")
                .checked(self.discovery_custom)
                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                    this.discovery_custom = *checked;
                    cx.notify();
                })),
        );
        if self.discovery_custom {
            relay = relay
                .child(Input::new(&self.pkarr_url))
                .child(Input::new(&self.dns_origin));
        }
        relay = relay.child(
            row().justify_end().child(
                Button::new("apply")
                    .primary()
                    .label("Apply")
                    .on_click(cx.listener(|this, _, _, cx| this.apply_network(cx))),
            ),
        );

        v_flex().gap_4().child(general).child(relay).child(
            row()
                .child(div().flex_1().child(muted(
                    match updater::BUILD {
                        0 => format!("DashBeam Lite {} (local build)", env!("CARGO_PKG_VERSION")),
                        build => format!(
                            "DashBeam Lite {} (build {build})",
                            env!("CARGO_PKG_VERSION")
                        ),
                    },
                    cx,
                )))
                .when(updater::enabled(), |row| {
                    row.child(
                        Button::new("check-updates")
                            .ghost()
                            .label("Check for updates")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dash.update(cx, |d, cx| d.check_for_update(cx))
                            })),
                    )
                }),
        )
    }
}

fn ago(ms: u64) -> String {
    let secs = engine::unix_now_ms().saturating_sub(ms) / 1000;
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}
