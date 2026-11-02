// No console window behind the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod net;
mod store;

use config::{mask_from_prefix, ManualConfig, DEFAULT_PREFIX};
use eframe::egui::{self, Color32, RichText};
use net::{Adapter, Mode};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use store::{Profile, Store};

const GREEN: Color32 = Color32::from_rgb(52, 199, 89);
const ORANGE: Color32 = Color32::from_rgb(255, 159, 10);
const RED: Color32 = Color32::from_rgb(235, 70, 60);
const ACCENT: Color32 = Color32::from_rgb(0, 103, 192);
const ACCENT_HOVER: Color32 = Color32::from_rgb(0, 90, 170);
const LABEL_WIDTH: f32 = 110.0;
const ROW_HEIGHT: f32 = 38.0;
const BOLD: &str = "bold";

fn main() -> eframe::Result {
    // `rackip --list` prints the detected ports; handy for bug reports.
    if std::env::args().any(|a| a == "--list") {
        match net::adapters() {
            Ok(list) => list.iter().for_each(|a| println!("{a:?}")),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("RackIP")
        .with_app_id("rackip")
        .with_inner_size([480.0, 560.0])
        .with_min_inner_size([440.0, 540.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    eframe::run_native(
        "RackIP",
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(App::new(&cc.egui_ctx)))
        }),
    )
}

/// Reads the first font file that exists.
fn system_font(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

/// Native-looking fonts and a calmer, card-based theme, in light and dark.
fn setup_style(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    let defaults = fonts.families[&FontFamily::Proportional].clone();
    let (regular, bold) = if cfg!(windows) {
        (
            system_font(&[r"C:\Windows\Fonts\segoeui.ttf"]),
            system_font(&[r"C:\Windows\Fonts\seguisb.ttf", r"C:\Windows\Fonts\segoeuib.ttf"]),
        )
    } else if cfg!(target_os = "macos") {
        (
            system_font(&["/System/Library/Fonts/Supplemental/Arial.ttf"]),
            system_font(&["/System/Library/Fonts/Supplemental/Arial Bold.ttf"]),
        )
    } else {
        (
            system_font(&[
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ]),
            system_font(&[
                "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/google-noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            ]),
        )
    };
    let mut proportional = defaults.clone();
    if let Some(data) = regular {
        fonts.font_data.insert("system".into(), Arc::new(FontData::from_owned(data)));
        proportional.insert(0, "system".into());
    }
    let mut bold_family = proportional.clone();
    if let Some(data) = bold {
        fonts.font_data.insert("system-bold".into(), Arc::new(FontData::from_owned(data)));
        bold_family.insert(0, "system-bold".into());
    }
    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold_family);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        style.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
        style.spacing.button_padding = egui::vec2(14.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.combo_height = 300.0;
        let dark = style.visuals.dark_mode;
        let v = &mut style.visuals;
        v.panel_fill = if dark { Color32::from_gray(32) } else { Color32::from_gray(250) };
        v.window_fill = if dark { Color32::from_gray(44) } else { Color32::WHITE };
        v.extreme_bg_color = if dark { Color32::from_gray(28) } else { Color32::WHITE };
        v.selection.bg_fill = ACCENT;
        v.selection.stroke.color = if dark { Color32::from_rgb(96, 170, 255) } else { ACCENT };
        v.hyperlink_color = v.selection.stroke.color;
        let field_stroke = if dark { Color32::from_gray(70) } else { Color32::from_gray(205) };
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = 6.into();
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.corner_radius = 6.into();
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, field_stroke);
        v.widgets.inactive.weak_bg_fill = if dark { Color32::from_gray(56) } else { Color32::from_gray(232) };
        v.widgets.inactive.bg_fill = v.widgets.inactive.weak_bg_fill;
        v.widgets.hovered.weak_bg_fill = if dark { Color32::from_gray(66) } else { Color32::from_gray(222) };
        v.widgets.hovered.bg_fill = v.widgets.hovered.weak_bg_fill;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, field_stroke);
        v.widgets.active.weak_bg_fill = if dark { Color32::from_gray(76) } else { Color32::from_gray(210) };
        v.widgets.active.bg_stroke = egui::Stroke::new(1.0, field_stroke);
        v.window_corner_radius = 8.into();
        v.menu_corner_radius = 8.into();
    });
}

fn bold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(egui::FontFamily::Name(BOLD.into())).size(size).strong()
}

fn card_fill(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode { Color32::from_gray(43) } else { Color32::from_gray(241) }
}

/// A rounded group of rows, like the grouped forms on macOS and in Windows Settings.
fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(card_fill(ui))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(14, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

/// One labelled row inside a card. Rows after the first get a divider above them.
fn row<R>(ui: &mut egui::Ui, first: bool, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if !first {
        let y = ui.cursor().top();
        let color = if ui.visuals().dark_mode { Color32::from_gray(58) } else { Color32::from_gray(222) };
        ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, color));
    }
    let width = ui.available_width();
    ui.allocate_ui_with_layout(egui::vec2(width, ROW_HEIGHT), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_height(ROW_HEIGHT);
        ui.spacing_mut().item_spacing.x = 8.0;
        let r = ui.label(label);
        ui.add_space((LABEL_WIDTH - r.rect.width() - 8.0).max(0.0));
        add(ui)
    })
    .inner
}

/// An iOS/Windows-style on/off switch.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(38.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let dark = ui.visuals().dark_mode;
    let off_fill = if dark { Color32::from_gray(80) } else { Color32::from_gray(200) };
    let fill = lerp_color(off_fill, ACCENT, t);
    let radius = rect.height() / 2.0;
    ui.painter().rect_filled(rect, radius, fill);
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), radius - 3.0, Color32::WHITE);
    response
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// A frameless square button with a hand-drawn icon, so it matches the text color in both themes.
fn icon_button(ui: &mut egui::Ui, enabled: bool, draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Stroke)) -> egui::Response {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), sense);
    let visuals = ui.style().interact(&response);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 6, visuals.weak_bg_fill);
    }
    let mut color = ui.visuals().text_color();
    if !enabled {
        color = color.gamma_multiply(0.35);
    } else if !response.hovered() {
        color = color.gamma_multiply(0.75);
    }
    draw(ui.painter(), rect.shrink(7.0), egui::Stroke::new(1.4, color));
    response
}

fn draw_trash(p: &egui::Painter, r: egui::Rect, s: egui::Stroke) {
    use egui::pos2;
    let (l, rt, t, b) = (r.left(), r.right(), r.top(), r.bottom());
    let w = r.width();
    p.line_segment([pos2(l, t + 2.5), pos2(rt, t + 2.5)], s);
    p.line_segment([pos2(l + w * 0.35, t + 2.5), pos2(l + w * 0.35, t)], s);
    p.line_segment([pos2(l + w * 0.35, t), pos2(l + w * 0.65, t)], s);
    p.line_segment([pos2(l + w * 0.65, t), pos2(l + w * 0.65, t + 2.5)], s);
    let body = [pos2(l + 1.5, t + 2.5), pos2(l + 2.5, b), pos2(rt - 2.5, b), pos2(rt - 1.5, t + 2.5)];
    p.line_segment([body[0], body[1]], s);
    p.line_segment([body[1], body[2]], s);
    p.line_segment([body[2], body[3]], s);
    for f in [0.42, 0.58] {
        p.line_segment([pos2(l + w * f, t + 5.0), pos2(l + w * f, b - 2.5)], s);
    }
}

fn draw_refresh(p: &egui::Painter, r: egui::Rect, s: egui::Stroke) {
    p.text(r.center(), egui::Align2::CENTER_CENTER, "⟳", egui::FontId::proportional(19.0), s.color);
}

#[derive(Default)]
struct Ports {
    list: Vec<Adapter>,
    loaded: bool,
    error: Option<String>,
}

/// A change running in the background: its result, then the success message and warnings.
type Job = (Receiver<Result<(), String>>, String, Vec<String>);

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

struct App {
    ports: Arc<Mutex<Ports>>,
    refresh: Sender<()>,
    store: Store,
    selected_profile: Option<u64>,
    name: String,
    address: String,
    mask: String,
    gateway: String,
    status: Option<(Kind, String)>,
    job: Option<Job>,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let ports = Arc::new(Mutex::new(Ports::default()));
        let (refresh, wake) = mpsc::channel::<()>();
        {
            // Poll every few seconds so the link indicator follows the cable.
            let ports = ports.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || loop {
                let result = net::adapters();
                {
                    let mut p = ports.lock().unwrap();
                    p.loaded = true;
                    match result {
                        Ok(list) => {
                            p.list = list;
                            p.error = None;
                        }
                        Err(e) => p.error = Some(e),
                    }
                }
                ctx.request_repaint();
                match wake.recv_timeout(Duration::from_secs(3)) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            });
        }
        App {
            ports,
            refresh,
            store: Store::load(),
            selected_profile: None,
            name: String::new(),
            address: String::new(),
            mask: String::new(),
            gateway: String::new(),
            status: None,
            job: None,
        }
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn selected<'a>(&self, list: &'a [Adapter]) -> Option<&'a Adapter> {
        let name = self.store.selected_adapter.as_deref()?;
        list.iter().find(|a| a.name == name)
    }

    fn select_adapter(&mut self, name: Option<String>) {
        if self.store.selected_adapter != name {
            self.store.selected_adapter = name;
            self.store.save();
        }
    }

    fn select_profile(&mut self, id: Option<u64>, list: &[Adapter]) {
        self.selected_profile = id;
        let Some(p) = id.and_then(|id| self.store.profiles.iter().find(|p| p.id == id)).cloned() else {
            self.name.clear();
            self.address.clear();
            self.mask.clear();
            self.gateway.clear();
            return;
        };
        // Empty when it's just the IP, so editing the IP renames it too.
        self.name = if p.is_named_after_address() { String::new() } else { p.name.clone() };
        self.address = p.address.clone();
        self.mask = p.mask.clone();
        self.gateway = p.gateway.clone();
        if let Some(a) = p.adapter.filter(|a| list.iter().any(|x| &x.name == a)) {
            self.select_adapter(Some(a));
        }
    }

    fn upsert_profile(&mut self, mark_used: bool) -> Profile {
        let (address, mask, gateway) = (self.address.trim(), self.mask.trim(), self.gateway.trim());
        let existing = self
            .store
            .profiles
            .iter()
            .position(|p| Some(p.id) == self.selected_profile)
            .or_else(|| {
                self.store
                    .profiles
                    .iter()
                    .position(|p| p.address == address && p.mask == mask && p.gateway == gateway)
            });
        let mut p = existing.map(|i| self.store.profiles[i].clone()).unwrap_or(Profile {
            id: store::new_id(),
            name: String::new(),
            address: String::new(),
            mask: String::new(),
            gateway: String::new(),
            adapter: None,
            last_used: 0,
        });
        let typed = self.name.trim();
        // Retyping a saved IP with no name keeps that address's existing name.
        let keep = typed.is_empty() && Some(p.id) != self.selected_profile && !p.is_named_after_address();
        if !keep {
            p.name = if typed.is_empty() { address.to_string() } else { typed.to_string() };
        }
        p.address = address.to_string();
        p.mask = mask.to_string();
        p.gateway = gateway.to_string();
        if mark_used {
            p.adapter = self.store.selected_adapter.clone();
            p.last_used = store::now();
        }
        match existing {
            Some(i) => self.store.profiles[i] = p.clone(),
            None => self.store.profiles.push(p.clone()),
        }
        self.store.save();
        self.selected_profile = Some(p.id);
        p
    }

    fn delete_profile(&mut self, list: &[Adapter]) {
        if let Some(id) = self.selected_profile {
            self.store.profiles.retain(|p| p.id != id);
            self.store.save();
            self.select_profile(None, list);
        }
    }

    fn save(&mut self) {
        match ManualConfig::parse(&self.address, &self.mask, &self.gateway) {
            Err(e) => self.status = Some((Kind::Error, e)),
            Ok(_) => {
                let p = self.upsert_profile(false);
                self.status = Some((Kind::Success, format!("Saved “{}”.", p.title())));
            }
        }
    }

    fn apply(&mut self, adapter: Option<Adapter>) {
        let Some(adapter) = adapter else {
            self.status = Some((Kind::Error, "Choose a network port first.".into()));
            return;
        };
        let config = match ManualConfig::parse(&self.address, &self.mask, &self.gateway) {
            Ok(c) => c,
            Err(e) => {
                self.status = Some((Kind::Error, e));
                return;
            }
        };
        self.upsert_profile(true);
        let mut done = format!("{} is now {}/{}", adapter.name, config.address, config.prefix);
        if let Some(g) = config.gateway {
            done += &format!(" via {g}");
        }
        done.push('.');
        let warnings = config.warnings.clone();
        self.start(format!("Setting {} on {}…", config.address, adapter.name), done, warnings, move || {
            net::set_static(&adapter, &config)
        });
    }

    fn use_dhcp(&mut self, adapter: Option<Adapter>) {
        let Some(adapter) = adapter else { return };
        let done = format!("{} is back on DHCP (automatic).", adapter.name);
        self.start(format!("Switching {} to DHCP…", adapter.name), done, vec![], move || net::set_dhcp(&adapter));
    }

    fn start(&mut self, progress: String, done: String, warnings: Vec<String>, work: impl FnOnce() -> Result<(), String> + Send + 'static) {
        if self.busy() {
            return;
        }
        self.status = Some((Kind::Info, progress));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        self.job = Some((rx, done, warnings));
    }

    fn poll_job(&mut self) {
        let Some((rx, done, warnings)) = &self.job else { return };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("The change was interrupted.".into()),
        };
        self.status = Some(match result {
            Err(e) => (Kind::Error, e),
            Ok(()) => match warnings.first() {
                Some(w) => (Kind::Warning, format!("{done} {w}")),
                None => (Kind::Success, done.clone()),
            },
        });
        self.job = None;
        // The OS takes a moment to publish the new address.
        let refresh = self.refresh.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            let _ = refresh.send(());
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(debug_assertions)]
        debug_screenshot(&ctx);
        self.poll_job();
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        let (list, loaded, error) = {
            let p = self.ports.lock().unwrap();
            (p.list.clone(), p.loaded, p.error.clone())
        };

        // First launch or the adapter was unplugged: prefer a wired port with link.
        if loaded && self.selected(&list).is_none() {
            let wired: Vec<&Adapter> = list.iter().filter(|a| a.wired).collect();
            let pick = wired.iter().find(|a| a.connected).or(wired.first()).copied().or(list.first());
            if let Some(a) = pick {
                self.select_adapter(Some(a.name.clone()));
            }
        }
        let selected = self.selected(&list).cloned();

        let frame = egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(18, 14));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            self.port_section(ui, &list, selected.as_ref(), loaded, error.as_deref());
            ui.add_space(14.0);
            self.address_section(ui, &list);
            ui.add_space(14.0);
            self.footer(ui, selected);
        });
    }
}

impl App {
    fn port_section(&mut self, ui: &mut egui::Ui, list: &[Adapter], selected: Option<&Adapter>, loaded: bool, error: Option<&str>) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(bold("Network port", 15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, true, draw_refresh).on_hover_text("Refresh").clicked() {
                    let _ = self.refresh.send(());
                }
            });
        });

        let wired_count = list.iter().filter(|a| a.wired).count();
        let visible: Vec<&Adapter> = list
            .iter()
            .filter(|a| self.store.show_all || wired_count == 0 || a.wired || Some(a.name.as_str()) == self.store.selected_adapter.as_deref())
            .collect();

        card(ui, |ui| {
            row(ui, true, "Port", |ui| {
                let shown = selected.map(port_label).unwrap_or_else(|| if loaded { "No ports found".into() } else { "Looking for ports…".into() });
                let mut choice = self.store.selected_adapter.clone();
                egui::ComboBox::from_id_salt("port-combo")
                    .width(ui.available_width())
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for a in &visible {
                            ui.selectable_value(&mut choice, Some(a.name.clone()), port_label(a))
                                .on_hover_text(&a.detail);
                        }
                    });
                if choice != self.store.selected_adapter {
                    self.select_adapter(choice);
                }
            });

            row(ui, false, "Current", |ui| {
                if let Some(a) = selected {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    let color = if a.connected { GREEN } else { ui.visuals().weak_text_color() };
                    ui.painter().circle_filled(rect.center(), 4.5, color);
                    resp.on_hover_text(if a.connected { "Cable connected" } else { "No link" });
                }
                ui.add(egui::Label::new(RichText::new(current_text(selected)).weak()).truncate());
            });

            let mut show_all = self.store.show_all;
            let width = ui.available_width();
            let changed = ui
                .allocate_ui_with_layout(egui::vec2(width, ROW_HEIGHT), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_height(ROW_HEIGHT);
                    let y = ui.max_rect().top();
                    let color = if ui.visuals().dark_mode { Color32::from_gray(58) } else { Color32::from_gray(222) };
                    ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, color));
                    ui.label(RichText::new("Show Wi-Fi and other ports").weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| toggle(ui, &mut show_all).changed())
                        .inner
                })
                .inner;
            if changed {
                self.store.show_all = show_all;
                self.store.save();
            }
        });
        if let Some(e) = error {
            ui.colored_label(RED, e);
        }
    }

    fn address_section(&mut self, ui: &mut egui::Ui, list: &[Adapter]) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(bold("Address", 15.0));
        });
        let parsed = ManualConfig::parse(&self.address, &self.mask, &self.gateway);
        let mut submit = false;

        let (hint, is_error) = if self.address.trim().is_empty() {
            ("Leave the gateway empty to keep internet on Wi-Fi while you work on the server network.".to_string(), false)
        } else {
            match &parsed {
                Err(e) => (e.clone(), true),
                Ok(c) => match c.warnings.first() {
                    Some(w) => (w.clone(), true),
                    None if c.gateway.is_none() => ("No gateway: only the local subnet is reached through this port.".into(), false),
                    None => (String::new(), false),
                },
            }
        };

        card(ui, |ui| {
            row(ui, true, "Saved", |ui| {
                let profiles = self.store.sorted_profiles();
                let current = profiles.iter().find(|p| Some(p.id) == self.selected_profile);
                let mut choice = self.selected_profile;
                egui::ComboBox::from_id_salt("profile-combo")
                    .width(ui.available_width() - 36.0)
                    .selected_text(current.map(Profile::title).unwrap_or_else(|| "New address".into()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut choice, None, "New address");
                        if !profiles.is_empty() {
                            ui.separator();
                        }
                        for p in &profiles {
                            ui.selectable_value(&mut choice, Some(p.id), p.title());
                        }
                    });
                if choice != self.selected_profile {
                    self.select_profile(choice, list);
                }
                let delete = icon_button(ui, self.selected_profile.is_some(), draw_trash).on_hover_text("Delete this saved address");
                if delete.clicked() {
                    self.delete_profile(list);
                }
            });

            let ip = self.address.trim().to_string();
            let name_hint = if ip.is_empty() { "Optional, defaults to the IP".to_string() } else { ip };
            let mask_hint = match &parsed {
                Ok(c) if self.mask.is_empty() => c.mask().to_string(),
                _ => mask_from_prefix(DEFAULT_PREFIX).to_string(),
            };
            for (label, value, hint) in [
                ("Name", &mut self.name, name_hint),
                ("IP address", &mut self.address, "10.0.0.5 or 10.0.0.5/24".to_string()),
                ("Subnet mask", &mut self.mask, mask_hint),
                ("Gateway", &mut self.gateway, "Optional".to_string()),
            ] {
                row(ui, false, label, |ui| {
                    let edit = egui::TextEdit::singleline(value)
                        .hint_text(hint)
                        .margin(egui::Margin::symmetric(8, 7))
                        .desired_width(ui.available_width());
                    let r = ui.add(edit);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                });
            }

            if !hint.is_empty() {
                let y = ui.cursor().top();
                let color = if ui.visuals().dark_mode { Color32::from_gray(58) } else { Color32::from_gray(222) };
                ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, color));
                ui.add_space(9.0);
                let text = RichText::new(hint).small();
                ui.add(egui::Label::new(if is_error { text.color(RED) } else { text.weak() }).wrap());
                ui.add_space(9.0);
            }
        });

        if submit && !self.busy() {
            let selected = self.selected(list).cloned();
            self.apply(selected);
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui, selected: Option<Adapter>) {
        let enabled = !self.busy() && selected.is_some();
        ui.horizontal(|ui| {
            if ui.add_enabled(enabled, egui::Button::new("Use DHCP")).on_hover_text("Switch this port back to automatic addressing").clicked() {
                self.use_dhcp(selected.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let apply = ui.scope(|ui| {
                    let w = &mut ui.visuals_mut().widgets;
                    w.inactive.weak_bg_fill = ACCENT;
                    w.inactive.bg_stroke = egui::Stroke::NONE;
                    w.hovered.weak_bg_fill = ACCENT_HOVER;
                    w.hovered.bg_stroke = egui::Stroke::NONE;
                    w.active.weak_bg_fill = ACCENT_HOVER;
                    let button = egui::Button::new(RichText::new("Apply").color(Color32::WHITE)).min_size(egui::vec2(80.0, 0.0));
                    ui.add_enabled(enabled, button)
                });
                if apply.inner.clicked() {
                    self.apply(selected.clone());
                }
                if ui.add_enabled(enabled, egui::Button::new("Save")).on_hover_text("Save this address without applying it").clicked() {
                    self.save();
                }
                if self.busy() {
                    ui.spinner();
                }
            });
        });

        if let Some((kind, text)) = &self.status {
            let (icon, color) = match kind {
                Kind::Info => ("…", ui.visuals().weak_text_color()),
                Kind::Success => ("✔", GREEN),
                Kind::Warning => ("⚠", ORANGE),
                Kind::Error => ("✖", RED),
            };
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(format!("{icon} {text}")).color(color)).wrap());
        }
    }
}

fn port_label(a: &Adapter) -> String {
    if a.connected {
        format!("{} · connected", a.name)
    } else {
        a.name.clone()
    }
}

fn current_text(selected: Option<&Adapter>) -> String {
    let Some(a) = selected else { return "No port selected".into() };
    let mode = match a.mode {
        Mode::Dhcp => "DHCP",
        Mode::Manual => "Manual",
        Mode::Unknown => "Not configured",
    };
    let Some(ip) = a.address else { return format!("{mode} · no address") };
    let mut text = format!("{mode} · {ip}");
    if let Some(p) = a.prefix {
        text += &format!("/{p}");
    }
    text += &match a.gateway {
        Some(g) => format!(" · gw {g}"),
        None => " · no gateway".into(),
    };
    text
}

/// Debug builds only: `RACKIP_SCREENSHOT=out.bmp cargo run` saves the window
/// after it settles and exits. Used to check the layout from scripts.
#[cfg(debug_assertions)]
fn debug_screenshot(ctx: &egui::Context) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REQUESTED: AtomicBool = AtomicBool::new(false);
    let Ok(path) = std::env::var("RACKIP_SCREENSHOT") else { return };
    if ctx.input(|i| i.time) < 2.5 {
        ctx.request_repaint_after(Duration::from_millis(200));
        return;
    }
    if REQUESTED.swap(true, Ordering::SeqCst) {
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |image| {
        let [w, h] = image.size;
        let le = |v: u32| v.to_le_bytes();
        let mut bmp = Vec::with_capacity(54 + w * h * 4);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&le((54 + w * h * 4) as u32));
        bmp.extend_from_slice(&[0, 0, 0, 0]);
        bmp.extend_from_slice(&le(54));
        bmp.extend_from_slice(&le(40));
        bmp.extend_from_slice(&le(w as u32));
        bmp.extend_from_slice(&(-(h as i32)).to_le_bytes());
        bmp.extend_from_slice(&[1, 0, 32, 0]);
        bmp.extend_from_slice(&[0; 24]);
        for p in &image.pixels {
            bmp.extend_from_slice(&[p.b(), p.g(), p.r(), 255]);
        }
        let _ = std::fs::write(&path, bmp);
        std::process::exit(0);
    })));
    ctx.request_repaint();
}
