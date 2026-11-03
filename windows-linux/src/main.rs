// No console window behind the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod dhcp;
mod net;
mod store;

use config::{mask_from_prefix, ManualConfig, DEFAULT_PREFIX};
use dhcp::{DhcpConfig, Lease};
use eframe::egui::{self, Color32, RichText};
use net::{Adapter, Mode};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
        // A tinted highlight: the stroke color is also the text of the selected
        // dropdown item, so it has to stand out against the fill.
        v.selection.bg_fill = if dark { Color32::from_rgb(0, 60, 120) } else { Color32::from_rgb(204, 228, 255) };
        v.selection.stroke.color = if dark { Color32::from_rgb(150, 200, 255) } else { ACCENT };
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
enum Tab {
    StaticIp,
    Dhcp,
    About,
}

/// The DHCP server while it runs.
struct RunningDhcp {
    server: Box<dyn net::DhcpServer>,
    adapter: String,
    leases: Vec<Lease>,
    polled: Instant,
    problem: Option<String>,
}

/// A DHCP server being started in the background: its result, the port name and the config.
type DhcpJob = (Receiver<Result<Box<dyn net::DhcpServer>, String>>, String, DhcpConfig);

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
    tab: Tab,
    icon: Option<egui::TextureHandle>,
    dhcp_server: String,
    dhcp_mask: String,
    dhcp_start: String,
    dhcp_end: String,
    dhcp_lease: String,
    dhcp: Option<RunningDhcp>,
    dhcp_job: Option<DhcpJob>,
}

impl Drop for App {
    // Linux keeps NetworkManager's shared mode after the app quits, and Windows
    // the firewall rule, so stop the server on the way out.
    fn drop(&mut self) {
        if let Some(running) = self.dhcp.take() {
            let _ = running.server.stop();
        }
    }
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
        let store = Store::load();
        App {
            ports,
            refresh,
            dhcp_server: store.dhcp_server.clone(),
            dhcp_mask: store.dhcp_mask.clone(),
            dhcp_start: store.dhcp_start.clone(),
            dhcp_end: store.dhcp_end.clone(),
            dhcp_lease: store.dhcp_lease.clone(),
            dhcp: None,
            dhcp_job: None,
            store,
            selected_profile: None,
            name: String::new(),
            address: String::new(),
            mask: String::new(),
            gateway: String::new(),
            status: None,
            job: None,
            tab: Tab::StaticIp,
            icon: None,
        }
    }

    fn busy(&self) -> bool {
        self.job.is_some() || self.dhcp_job.is_some()
    }

    /// Changing the port's address would pull it from under the server.
    fn blocked_by_dhcp(&mut self) -> bool {
        let Some(running) = &self.dhcp else { return false };
        self.status = Some((Kind::Error, format!("The DHCP server is running on {}. Stop it first to change the address.", running.adapter)));
        true
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
        if let Some(a) = p.adapter.filter(|a| self.dhcp.is_none() && list.iter().any(|x| &x.name == a)) {
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
        if self.blocked_by_dhcp() {
            return;
        }
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
        if self.blocked_by_dhcp() {
            return;
        }
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

    fn parsed_dhcp(&self) -> Result<DhcpConfig, String> {
        DhcpConfig::parse(&self.dhcp_server, &self.dhcp_mask, &self.dhcp_start, &self.dhcp_end, &self.dhcp_lease)
    }

    fn start_dhcp(&mut self, adapter: Option<Adapter>) {
        if self.busy() || self.dhcp.is_some() {
            return;
        }
        let Some(adapter) = adapter else {
            self.status = Some((Kind::Error, "Choose a network port first.".into()));
            return;
        };
        let config = match self.parsed_dhcp() {
            Ok(c) => c,
            Err(e) => {
                self.status = Some((Kind::Error, e));
                return;
            }
        };
        // An address from DHCP means a server already runs on this cable. A second one would fight it.
        if let (Mode::Dhcp, Some(ip)) = (adapter.mode, adapter.address) {
            if !ip.is_link_local() {
                self.status = Some((
                    Kind::Error,
                    format!("{} already got {ip} from a DHCP server on this network. RackIP won't start a second one here, it would hand out conflicting addresses.", adapter.name),
                ));
                return;
            }
        }
        self.store.dhcp_server = self.dhcp_server.trim().to_string();
        self.store.dhcp_mask = self.dhcp_mask.trim().to_string();
        self.store.dhcp_start = self.dhcp_start.trim().to_string();
        self.store.dhcp_end = self.dhcp_end.trim().to_string();
        self.store.dhcp_lease = self.dhcp_lease.trim().to_string();
        self.store.save();

        self.status = Some((Kind::Info, format!("Starting the DHCP server on {}…", adapter.name)));
        let (tx, rx) = mpsc::channel();
        let name = adapter.name.clone();
        let c = config.clone();
        std::thread::spawn(move || {
            let _ = tx.send(net::start_dhcp(&adapter, &c));
        });
        self.dhcp_job = Some((rx, name, config));
    }

    fn stop_dhcp(&mut self) {
        if self.busy() {
            return;
        }
        let Some(running) = self.dhcp.take() else { return };
        let done = format!("DHCP server stopped. {} keeps its address.", running.adapter);
        self.start("Stopping the DHCP server…".into(), done, vec![], move || running.server.stop());
    }

    fn poll_dhcp(&mut self) {
        if let Some((rx, name, config)) = &self.dhcp_job {
            let result = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Starting the DHCP server was interrupted.".into())),
            };
            if let Some(result) = result {
                match result {
                    Ok(server) => {
                        self.status = Some((
                            Kind::Success,
                            format!(
                                "DHCP server running on {name} ({}/{}), handing out {} to {}.",
                                config.server, config.prefix, config.start, config.end
                            ),
                        ));
                        self.dhcp = Some(RunningDhcp { server, adapter: name.clone(), leases: Vec::new(), polled: Instant::now(), problem: None });
                    }
                    Err(e) => self.status = Some((Kind::Error, e)),
                }
                self.dhcp_job = None;
                let _ = self.refresh.send(());
            }
        }
        if let Some(running) = &mut self.dhcp {
            if running.polled.elapsed() >= Duration::from_secs(1) {
                running.leases = running.server.leases();
                running.polled = Instant::now();
                let problem = running.server.problem();
                if problem.is_some() && problem != running.problem {
                    self.status = Some((Kind::Warning, problem.clone().unwrap_or_default()));
                }
                running.problem = problem;
            }
        }
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
        self.poll_dhcp();
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.dhcp.is_some() {
            ctx.request_repaint_after(Duration::from_secs(1));
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
            self.tab_bar(ui);
            ui.add_space(10.0);
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| match self.tab {
                Tab::StaticIp => {
                    self.port_section(ui, &list, selected.as_ref(), loaded, error.as_deref());
                    ui.add_space(14.0);
                    self.address_section(ui, &list);
                    ui.add_space(14.0);
                    self.footer(ui, selected);
                }
                Tab::Dhcp => {
                    self.port_section(ui, &list, selected.as_ref(), loaded, error.as_deref());
                    ui.add_space(14.0);
                    self.dhcp_section(ui);
                    if self.dhcp.is_some() {
                        ui.add_space(14.0);
                        self.devices_section(ui);
                    }
                    ui.add_space(14.0);
                    self.dhcp_footer(ui, selected);
                }
                Tab::About => self.about(ui),
            });
        });
    }
}

impl App {
    /// A segmented control, like the tab picker on macOS.
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let tabs = [(Tab::StaticIp, "Static IP"), (Tab::Dhcp, "DHCP Server"), (Tab::About, "About")];
        let segment = egui::vec2(110.0, 28.0);
        let total = egui::vec2(segment.x * tabs.len() as f32 + 4.0, segment.y + 4.0);
        let (outer, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), total.y), egui::Sense::hover());
        let bar = egui::Rect::from_center_size(outer.center(), total);
        let dark = ui.visuals().dark_mode;
        ui.painter().rect_filled(bar, 8, card_fill(ui));
        for (i, (tab, label)) in tabs.into_iter().enumerate() {
            let min = bar.min + egui::vec2(2.0 + segment.x * i as f32, 2.0);
            let rect = egui::Rect::from_min_size(min, segment);
            let response = ui.interact(rect, ui.scope_id().with(("tab", i)), egui::Sense::click());
            let active = self.tab == tab;
            if active {
                let fill = if dark { Color32::from_gray(76) } else { Color32::WHITE };
                ui.painter().rect_filled(rect, 6, fill);
            } else if response.hovered() {
                let fill = if dark { Color32::from_gray(56) } else { Color32::from_gray(228) };
                ui.painter().rect_filled(rect, 6, fill);
            }
            let color = if active { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(14.0), color);
            if response.clicked() {
                self.tab = tab;
            }
        }
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        let icon = self.icon.get_or_insert_with(|| {
            let data = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png")).unwrap_or_default();
            let image = egui::ColorImage::from_rgba_unmultiplied([data.width as usize, data.height as usize], &data.rgba);
            ui.ctx().load_texture("about-icon", image, egui::TextureOptions::LINEAR)
        });
        let icon = icon.clone();
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add(egui::Image::new(&icon).fit_to_exact_size(egui::vec2(96.0, 96.0)));
            ui.label(bold("RackIP", 20.0));
            ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).weak());
            ui.add_space(6.0);
            ui.label("Switch your Ethernet port's IP address in one click.");
            ui.add_space(14.0);
            ui.label("Made by Mo Majnioui");
            ui.hyperlink_to("majnioui.me", "https://majnioui.me");
            ui.hyperlink_to("Source on GitHub", "https://github.com/majnioui/RackIP");
            ui.add_space(14.0);
            ui.label(RichText::new("Free and open source, MIT License").small().weak());
        });
    }

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
                ui.add_enabled_ui(self.dhcp.is_none(), |ui| egui::ComboBox::from_id_salt("port-combo")
                    .width(ui.available_width())
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for a in &visible {
                            ui.selectable_value(&mut choice, Some(a.name.clone()), port_label(a))
                                .on_hover_text(&a.detail);
                        }
                    }))
                    .response
                    .on_disabled_hover_text("Stop the DHCP server to switch ports");
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

    fn dhcp_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(bold("DHCP server", 15.0));
        });
        let parsed = self.parsed_dhcp();
        let defaults = DhcpConfig::parse(&self.dhcp_server, &self.dhcp_mask, "", "", "").ok();
        let running = self.dhcp.is_some();
        let mut submit = false;
        card(ui, |ui| {
            let mask_hint = defaults.as_ref().map(|c| c.mask().to_string()).unwrap_or_else(|| mask_from_prefix(DEFAULT_PREFIX).to_string());
            let start_hint = defaults.as_ref().map(|c| c.start.to_string()).unwrap_or_default();
            let end_hint = defaults.as_ref().map(|c| c.end.to_string()).unwrap_or_default();
            let fields = [
                ("Server address", &mut self.dhcp_server, dhcp::DEFAULT_SERVER.to_string()),
                ("Subnet mask", &mut self.dhcp_mask, mask_hint),
                ("Range start", &mut self.dhcp_start, start_hint),
                ("Range end", &mut self.dhcp_end, end_hint),
                ("Lease (min)", &mut self.dhcp_lease, dhcp::DEFAULT_LEASE_MINUTES.to_string()),
            ];
            for (i, (label, value, hint)) in fields.into_iter().enumerate() {
                row(ui, i == 0, label, |ui| {
                    let edit = egui::TextEdit::singleline(value)
                        .hint_text(hint)
                        .margin(egui::Margin::symmetric(8, 7))
                        .desired_width(ui.available_width());
                    let r = ui.add_enabled(!running, edit);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                });
            }
            let (text, is_error) = match &parsed {
                Err(e) => (e.clone(), true),
                Ok(c) => {
                    let extra = if cfg!(target_os = "linux") {
                        "NetworkManager also offers this computer as gateway and DNS, and shares its internet connection."
                    } else {
                        "No gateway or DNS is handed out, so they only reach this computer."
                    };
                    (format!("The port gets {}/{}. Devices on the cable get {} to {}. {extra}", c.server, c.prefix, c.start, c.end), false)
                }
            };
            let y = ui.cursor().top();
            let color = if ui.visuals().dark_mode { Color32::from_gray(58) } else { Color32::from_gray(222) };
            ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, color));
            ui.add_space(9.0);
            let text = RichText::new(text).small();
            ui.add(egui::Label::new(if is_error { text.color(RED) } else { text.weak() }).wrap());
            ui.add_space(9.0);
        });
        if submit && !running && !self.busy() {
            let list = self.ports.lock().unwrap().list.clone();
            let selected = self.selected(&list).cloned();
            self.start_dhcp(selected);
        }
    }

    fn devices_section(&mut self, ui: &mut egui::Ui) {
        let Some(running) = &self.dhcp else { return };
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(bold("Devices", 15.0));
        });
        let leases = running.leases.clone();
        card(ui, |ui| {
            if leases.is_empty() {
                ui.add_space(10.0);
                ui.add(egui::Label::new(RichText::new("Waiting for devices. Plug one in, or restart its network so it asks for an address.").weak()).wrap());
                ui.add_space(10.0);
            }
            let now = store::now();
            for (i, lease) in leases.iter().enumerate() {
                if i > 0 {
                    let y = ui.cursor().top();
                    let color = if ui.visuals().dark_mode { Color32::from_gray(58) } else { Color32::from_gray(222) };
                    ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, color));
                }
                ui.add_space(7.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add(egui::Label::new(RichText::new(lease.ip.to_string()).strong()).selectable(true));
                        let detail = if lease.hostname.is_empty() { lease.mac.clone() } else { format!("{} · {}", lease.hostname, lease.mac) };
                        ui.add(egui::Label::new(RichText::new(detail).small().weak()).selectable(true));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(expires_text(lease.expires, now)).small().weak());
                    });
                });
                ui.add_space(7.0);
            }
        });
    }

    fn dhcp_footer(&mut self, ui: &mut egui::Ui, selected: Option<Adapter>) {
        let running = self.dhcp.is_some();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if running {
                    if ui.add_enabled(!self.busy(), egui::Button::new("Stop").min_size(egui::vec2(80.0, 0.0))).clicked() {
                        self.stop_dhcp();
                    }
                } else {
                    let enabled = !self.busy() && selected.is_some();
                    if accent_button(ui, enabled, "Start").clicked() {
                        self.start_dhcp(selected.clone());
                    }
                }
                if self.busy() {
                    ui.spinner();
                }
            });
        });
        self.status_line(ui);
    }

    fn footer(&mut self, ui: &mut egui::Ui, selected: Option<Adapter>) {
        let enabled = !self.busy() && selected.is_some();
        ui.horizontal(|ui| {
            if ui.add_enabled(enabled, egui::Button::new("Use DHCP")).on_hover_text("Switch this port back to automatic addressing").clicked() {
                self.use_dhcp(selected.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if accent_button(ui, enabled, "Apply").clicked() {
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

        self.status_line(ui);
    }

    fn status_line(&self, ui: &mut egui::Ui) {
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

/// The blue default button.
fn accent_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.weak_bg_fill = ACCENT;
        w.inactive.bg_stroke = egui::Stroke::NONE;
        w.hovered.weak_bg_fill = ACCENT_HOVER;
        w.hovered.bg_stroke = egui::Stroke::NONE;
        w.active.weak_bg_fill = ACCENT_HOVER;
        let button = egui::Button::new(RichText::new(label).color(Color32::WHITE)).min_size(egui::vec2(80.0, 0.0));
        ui.add_enabled(enabled, button)
    })
    .inner
}

fn expires_text(expires: u64, now: u64) -> String {
    if expires == u64::MAX {
        return "no expiry".into();
    }
    let minutes = expires.saturating_sub(now).div_ceil(60);
    if minutes >= 120 {
        format!("{} h left", minutes / 60)
    } else {
        format!("{minutes} min left")
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
