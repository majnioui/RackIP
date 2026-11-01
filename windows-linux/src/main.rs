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
const ACCENT: Color32 = Color32::from_rgb(18, 97, 84);
const LABEL_WIDTH: f32 = 96.0;

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
        .with_inner_size([470.0, 450.0])
        .with_min_inner_size([470.0, 450.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    eframe::run_native(
        "RackIP",
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
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

        egui::CentralPanel::default().show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            ui.add_space(4.0);
            self.port_section(ui, &list, selected.as_ref(), loaded, error.as_deref());
            ui.add_space(6.0);
            self.address_section(ui, &list);
            ui.add_space(6.0);
            ui.separator();
            self.footer(ui, selected);
        });
    }
}

impl App {
    fn port_section(&mut self, ui: &mut egui::Ui, list: &[Adapter], selected: Option<&Adapter>, loaded: bool, error: Option<&str>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Network port").strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("⟳").on_hover_text("Refresh").clicked() {
                    let _ = self.refresh.send(());
                }
            });
        });

        let wired_count = list.iter().filter(|a| a.wired).count();
        let visible: Vec<&Adapter> = list
            .iter()
            .filter(|a| self.store.show_all || wired_count == 0 || a.wired || Some(a.name.as_str()) == self.store.selected_adapter.as_deref())
            .collect();

        egui::Grid::new("port").num_columns(2).min_col_width(LABEL_WIDTH).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Port");
            let shown = selected.map(port_label).unwrap_or_else(|| if loaded { "No ports found".into() } else { "Looking for ports…".into() });
            let mut choice = self.store.selected_adapter.clone();
            egui::ComboBox::from_id_salt("port-combo")
                .width(300.0)
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
            ui.end_row();

            ui.label("Current");
            ui.horizontal(|ui| {
                if let Some(a) = selected {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    let color = if a.connected { GREEN } else { ui.visuals().weak_text_color() };
                    ui.painter().circle_filled(rect.center(), 4.5, color);
                    resp.on_hover_text(if a.connected { "Cable connected" } else { "No link" });
                }
                ui.label(RichText::new(current_text(selected)).weak());
            });
            ui.end_row();
        });

        let mut show_all = self.store.show_all;
        if ui.checkbox(&mut show_all, "Show Wi-Fi and other ports").changed() {
            self.store.show_all = show_all;
            self.store.save();
        }
        if let Some(e) = error {
            ui.colored_label(RED, e);
        }
    }

    fn address_section(&mut self, ui: &mut egui::Ui, list: &[Adapter]) {
        ui.label(RichText::new("Address").strong().size(15.0));
        let parsed = ManualConfig::parse(&self.address, &self.mask, &self.gateway);
        let mut submit = false;

        egui::Grid::new("address").num_columns(2).min_col_width(LABEL_WIDTH).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Saved");
            ui.horizontal(|ui| {
                let profiles = self.store.sorted_profiles();
                let current = profiles.iter().find(|p| Some(p.id) == self.selected_profile);
                let mut choice = self.selected_profile;
                egui::ComboBox::from_id_salt("profile-combo")
                    .width(266.0)
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
                let delete = ui
                    .add_enabled(self.selected_profile.is_some(), egui::Button::new("🗑"))
                    .on_hover_text("Delete this saved address");
                if delete.clicked() {
                    self.delete_profile(list);
                }
            });
            ui.end_row();

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
                ui.label(label);
                let r = ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(300.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }
                ui.end_row();
            }
        });

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
        if !hint.is_empty() {
            let text = RichText::new(hint).small();
            ui.label(if is_error { text.color(RED) } else { text.weak() });
        }

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
                let apply = egui::Button::new(RichText::new("Apply").strong().color(Color32::WHITE))
                    .fill(ACCENT)
                    .min_size(egui::vec2(72.0, 0.0));
                if ui.add_enabled(enabled, apply).clicked() {
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
