use std::ffi::CString;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::Rgb as VteRgb;
use arboard::Clipboard;
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext, Version,
};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
#[cfg(target_os = "linux")]
use winit::platform::wayland::WindowAttributesExtWayland;
#[cfg(target_os = "linux")]
use winit::platform::x11::WindowAttributesExtX11;
use winit::window::{CursorIcon, Fullscreen, Icon, ResizeDirection, Window, WindowId};

use crate::config::keybind::{Action, SplitDirection, SplitFocusTarget, match_keybind};
use crate::config::theme::ThemePalette;
use crate::config::{Config, ZawgyiMode};
use crate::confirm::{ConfirmChoice, ConfirmDialog, ConfirmLayout};
use crate::font::FontSystem;
use crate::help::{HelpClickOutcome, HelpDialog, HelpKeyOutcome};
use crate::menu::{ContextMenu, MenuClickOutcome, MenuKeyOutcome};
use crate::renderer::{GlRenderer, PaneScene, RenderEngine, TabTitle};
use crate::split::{
    PaneLayout, SplitAxis, SplitDivider, SplitNode, compute_split_layout, find_neighbor_pane,
    pane_at_point,
};
use crate::statusbar::{StatusBarContext, StatusBarGeometry, StatusBarHover, StatusClickTarget};
use crate::terminal::input::{
    encode_key_event, encode_mouse_button, encode_mouse_wheel, encode_paste,
};
use crate::terminal::{AppEvent, TerminalSession, TerminalSnapshot};

pub struct LaunchOptions {
    pub config_path: Option<PathBuf>,
    pub command: Option<(String, Vec<String>)>,
    pub working_directory: Option<PathBuf>,
    pub editor_file: Option<PathBuf>,
    pub initial_editor_mode: bool,
}

struct GlState {
    window: Window,
    gl_context: PossiblyCurrentContext,
    gl_surface: Surface<WindowSurface>,
    gl: glow::Context,
    renderer: GlRenderer,
}

struct Tab {
    root: SplitNode,
    panes: Vec<TerminalSession>,
    active_pane_id: u64,
}

impl Tab {
    fn new(session: TerminalSession) -> Self {
        let id = session.id;
        Self {
            root: SplitNode::Leaf(id),
            panes: vec![session],
            active_pane_id: id,
        }
    }

    fn active_pane(&self) -> Option<&TerminalSession> {
        self.panes
            .iter()
            .find(|p| p.id == self.active_pane_id)
            .or_else(|| self.panes.first())
    }

    fn pane_by_id(&self, id: u64) -> Option<&TerminalSession> {
        self.panes.iter().find(|p| p.id == id)
    }

    fn pane_by_id_mut(&mut self, id: u64) -> Option<&mut TerminalSession> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    fn title(&self) -> &str {
        self.active_pane()
            .map(|p| p.title.as_str())
            .unwrap_or("ocean")
    }

    fn shutdown_all(&self) {
        for pane in &self.panes {
            pane.shutdown();
        }
    }
}

struct DividerDragState {
    path: Vec<bool>,
    axis: SplitAxis,
    container_start: f32,
    container_span: f32,
}

pub struct OceanApp {
    pub config: Config,
    pub base_font_size: f32,
    pub launch_opts: LaunchOptions,
    pub proxy: EventLoopProxy<AppEvent>,
    pub wakeup_queued: Arc<AtomicBool>,
    pub editor: crate::editor::EditorWorkspace,
    ipc_socket: Option<PathBuf>,

    gl_state: Option<GlState>,
    font_sys: Option<FontSystem>,
    render_engine: RenderEngine,
    tabs: Vec<Tab>,
    active_tab: usize,
    next_pane_id: u64,
    snapshot_bufs: Vec<TerminalSnapshot>,
    clipboard: Option<Clipboard>,

    // Interaction & UI state
    context_menu: Option<ContextMenu>,
    theme_before_preview: Option<String>,
    help_dialog: Option<HelpDialog>,
    confirm_dialog: Option<ConfirmDialog>,
    available_update: Option<crate::update::UpdateInfo>,
    status_hover: StatusBarHover,
    dragging_divider: Option<DividerDragState>,
    selecting_pane_id: Option<u64>,
    modifiers: ModifiersState,
    cursor_pos: PhysicalPosition<f64>,
    current_cursor_icon: CursorIcon,
    mouse_left_down: bool,
    last_click_time: Instant,
    last_click_cell: (u64, usize, usize),
    click_streak: u8,
    last_tabbar_empty_click: Instant,
    pixel_scroll_accum: f64,
    mouse_hidden: bool,
    focused: bool,
    occluded: bool,

    // IME state (Burmese / multilingual input methods)
    ime_preedit: Option<String>,
    last_ime_spot: (u32, u32),
    ime_allowed: bool,

    // Blink & config auto-reload timers
    cursor_blink_on: bool,
    last_blink_toggle: Instant,
    last_config_poll: Instant,
}

impl OceanApp {
    pub fn new(
        config: Config,
        launch_opts: LaunchOptions,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Self {
        let base_font_size = config.font_size;
        let past = Instant::now() - Duration::from_secs(5);
        crate::update::start_background_update_check(
            config.auto_update_check,
            crate::update::running_version(),
            proxy.clone(),
        );

        let launch_cwd = launch_opts
            .working_directory
            .clone()
            .or_else(|| std::env::current_dir().ok());

        let mut editor = crate::editor::EditorWorkspace::new();
        editor.vim.enabled = config.editor_keymap == crate::config::EditorKeymap::Vim;
        if launch_opts.initial_editor_mode {
            editor.mode = crate::editor::WorkspaceMode::PureEditor;
            editor.editor_focused = true;
        }
        if let Some(ref ed_path) = launch_opts.editor_file {
            editor.mode = crate::editor::WorkspaceMode::PureEditor;
            editor.editor_focused = true;
            if ed_path.is_dir() {
                editor.open_folder(ed_path);
            } else {
                let _ = editor.open_file(ed_path, proxy.clone());
                let folder = ed_path
                    .parent()
                    .and_then(|p| {
                        if p.as_os_str().is_empty() {
                            None
                        } else {
                            Some(p)
                        }
                    })
                    .or_else(|| launch_cwd.as_deref())
                    .unwrap_or_else(|| std::path::Path::new("."));
                editor.open_folder(folder);
            }
        } else if let Some(ref cwd) = launch_cwd {
            editor.open_folder(cwd);
        } else {
            editor.open_folder(".");
        }

        let ipc_socket = crate::ipc::start_listener(proxy.clone()).ok();

        Self {
            config,
            base_font_size,
            launch_opts,
            proxy,
            wakeup_queued: Arc::new(AtomicBool::new(false)),
            editor,
            ipc_socket,
            gl_state: None,
            font_sys: None,
            render_engine: RenderEngine::new(),
            tabs: Vec::new(),
            active_tab: 0,
            next_pane_id: 1,
            snapshot_bufs: Vec::new(),
            clipboard: Clipboard::new().ok(),
            context_menu: None,
            theme_before_preview: None,
            help_dialog: None,
            confirm_dialog: None,
            available_update: None,
            status_hover: StatusBarHover::default(),
            dragging_divider: None,
            selecting_pane_id: None,
            modifiers: ModifiersState::default(),
            cursor_pos: PhysicalPosition::new(0.0, 0.0),
            current_cursor_icon: CursorIcon::Text,
            mouse_left_down: false,
            last_click_time: past,
            last_click_cell: (0, 0, 0),
            click_streak: 0,
            last_tabbar_empty_click: past,
            pixel_scroll_accum: 0.0,
            mouse_hidden: false,
            focused: true,
            occluded: false,
            ime_preedit: None,
            last_ime_spot: (u32::MAX, u32::MAX),
            ime_allowed: true,
            cursor_blink_on: true,
            last_blink_toggle: Instant::now(),
            last_config_poll: Instant::now(),
        }
    }

    fn init_window_and_gl(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let prelim_fonts = FontSystem::new(&self.config, 1.0);
        let init_status_h = RenderEngine::status_bar_height(&self.config, &prelim_fonts, 1.0);
        let init_w = (self.config.window_width as f32 * prelim_fonts.metrics.cell_width
            + (self.config.window_padding_x.0 + self.config.window_padding_x.1) as f32)
            .round()
            .max(320.0) as u32;
        let init_h = (self.config.window_height as f32 * prelim_fonts.metrics.cell_height
            + (self.config.window_padding_y.0 + self.config.window_padding_y.1) as f32
            + init_status_h)
            .round()
            .max(200.0) as u32;

        let title = self
            .config
            .title
            .clone()
            .unwrap_or_else(|| "ocean".to_string());

        let transparent = self.config.background_opacity < 0.999;
        let decorations = self.config.effective_decorations();
        let win_attrs = Window::default_attributes()
            .with_title(title)
            .with_window_icon(load_window_icon())
            .with_inner_size(LogicalSize::new(init_w, init_h))
            .with_decorations(decorations)
            .with_transparent(transparent);
        #[cfg(target_os = "linux")]
        let win_attrs = WindowAttributesExtWayland::with_name(win_attrs, "ocean", "ocean");
        #[cfg(target_os = "linux")]
        let win_attrs = WindowAttributesExtX11::with_name(win_attrs, "ocean", "ocean");

        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(transparent);

        let display_builder = DisplayBuilder::new().with_window_attributes(Some(win_attrs.clone()));
        let (maybe_window, gl_config) = display_builder
            .build(event_loop, template, |configs| {
                configs
                    .reduce(|accum, cfg| {
                        let transparency_match = cfg.supports_transparency().unwrap_or(false)
                            & !accum.supports_transparency().unwrap_or(false);
                        if transparency_match || cfg.num_samples() < accum.num_samples() {
                            cfg
                        } else {
                            accum
                        }
                    })
                    .expect("No compatible OpenGL/EGL config found")
            })
            .map_err(|e| format!("Failed creating window/GL display: {e}"))?;

        let window = match maybe_window {
            Some(w) => w,
            None => glutin_winit::finalize_window(event_loop, win_attrs, &gl_config)
                .map_err(|e| format!("finalize_window failed: {e}"))?,
        };

        window.set_decorations(decorations);
        // Enable Input Method Editor (IME) for Burmese / multilingual input on Wayland & X11
        window.set_ime_allowed(true);
        window.set_cursor(CursorIcon::Text);

        let raw_win_handle = window
            .window_handle()
            .map_err(|e| format!("window_handle: {e}"))?
            .as_raw();
        let gl_display = gl_config.display();

        let ctx_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(raw_win_handle));
        let fallback_gles_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
            .build(Some(raw_win_handle));

        let not_current = unsafe {
            gl_display
                .create_context(&gl_config, &ctx_attrs)
                .or_else(|_| gl_display.create_context(&gl_config, &fallback_gles_attrs))
                .map_err(|e| format!("Failed creating GL 3.3 / GLES 3.0 context: {e}"))?
        };

        let phys = window.inner_size();
        let surf_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_win_handle,
            NonZeroU32::new(phys.width.max(1)).unwrap(),
            NonZeroU32::new(phys.height.max(1)).unwrap(),
        );
        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surf_attrs)
                .map_err(|e| format!("create_window_surface: {e}"))?
        };
        let gl_context = not_current
            .make_current(&gl_surface)
            .map_err(|e| format!("make_current: {e}"))?;

        let swap_interval = if self.config.window_vsync {
            SwapInterval::Wait(NonZeroU32::new(1).unwrap())
        } else {
            SwapInterval::DontWait
        };
        let _ = gl_surface.set_swap_interval(&gl_context, swap_interval);

        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|name| gl_display.get_proc_address(name))
        };
        let renderer = GlRenderer::new(&gl)?;

        let scale_factor = window.scale_factor();
        let font_sys = FontSystem::new(&self.config, scale_factor);

        self.font_sys = Some(font_sys);
        self.gl_state = Some(GlState {
            window,
            gl_context,
            gl_surface,
            gl,
            renderer,
        });

        self.spawn_new_tab(true)?;
        Ok(())
    }

    fn inherited_cwd(&self, is_initial: bool) -> Option<PathBuf> {
        if is_initial {
            return self.launch_opts.working_directory.clone();
        }
        if self.config.working_directory.is_some() {
            return None;
        }
        self.active_session()
            .and_then(TerminalSession::current_working_directory)
    }

    fn spawn_new_tab(&mut self, is_initial: bool) -> Result<(), String> {
        let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &self.font_sys) else {
            return Ok(());
        };

        let phys = gl_state.window.inner_size();
        let sf = gl_state.window.scale_factor();
        let prospective_tabs = self.tabs.len() + 1;
        let (_, _, cols, rows) = RenderEngine::compute_grid_geometry(
            &self.config,
            font_sys,
            phys.width.max(64),
            phys.height.max(64),
            sf,
            prospective_tabs,
        );

        let id = self.next_pane_id;
        self.next_pane_id += 1;

        let cmd = if is_initial {
            self.launch_opts.command.clone()
        } else {
            None
        };
        let cwd = self.inherited_cwd(is_initial);

        let session = TerminalSession::spawn(
            id,
            &self.config,
            cmd,
            cwd,
            cols,
            rows,
            font_sys.metrics.cell_width as u16,
            font_sys.metrics.cell_height as u16,
            self.ipc_socket.as_deref(),
            self.proxy.clone(),
            Arc::clone(&self.wakeup_queued),
        )?;

        self.tabs.push(Tab::new(session));
        self.active_tab = self.tabs.len() - 1;
        self.resize_all_tabs();
        self.sync_window_title();
        self.request_redraw();
        Ok(())
    }

    fn spawn_tab_with_command(
        &mut self,
        prog: String,
        args: Vec<String>,
        title: Option<String>,
    ) -> Result<(), String> {
        let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &self.font_sys) else {
            return Ok(());
        };

        let phys = gl_state.window.inner_size();
        let sf = gl_state.window.scale_factor();
        let prospective_tabs = self.tabs.len() + 1;
        let (_, _, cols, rows) = RenderEngine::compute_grid_geometry(
            &self.config,
            font_sys,
            phys.width.max(64),
            phys.height.max(64),
            sf,
            prospective_tabs,
        );

        let id = self.next_pane_id;
        self.next_pane_id += 1;

        let cwd = self.inherited_cwd(false);
        let mut session = TerminalSession::spawn(
            id,
            &self.config,
            Some((prog.clone(), args)),
            cwd,
            cols,
            rows,
            font_sys.metrics.cell_width as u16,
            font_sys.metrics.cell_height as u16,
            self.ipc_socket.as_deref(),
            self.proxy.clone(),
            Arc::clone(&self.wakeup_queued),
        )?;
        session.title = title.unwrap_or_else(|| prog.clone());

        self.tabs.push(Tab::new(session));
        self.active_tab = self.tabs.len() - 1;
        self.resize_all_tabs();
        self.sync_window_title();
        self.request_redraw();
        Ok(())
    }

    pub fn open_config_in_editor(&mut self) {
        let config_path = match self.config.ensure_config_file() {
            Ok(p) => p,
            Err(err) => {
                eprintln!("ocean: failed resolving config file: {err}");
                return;
            }
        };

        let path_str = config_path.display().to_string();
        let editor = editor_program();
        let title = format!("{editor} — config");

        if let Err(err) = self.spawn_tab_with_command(editor, vec![path_str], Some(title)) {
            eprintln!("ocean: failed to launch config editor: {err}");
        }
    }

    pub fn trigger_app_update(&mut self) {
        self.close_context_menu();
        self.help_dialog = None;

        if let Some(info) = &self.available_update {
            let (prog, args) = crate::update::generate_update_command(info);
            let title = format!("Ocean Update {}", info.tag_name);
            if let Err(err) = self.spawn_tab_with_command(prog, args, Some(title)) {
                eprintln!("ocean: failed to launch update tab: {err}");
            }
        } else {
            if let Err(err) = self.spawn_tab_with_command(
                "ocean".to_string(),
                vec!["+update".to_string()],
                Some("Ocean Updater".to_string()),
            ) {
                eprintln!("ocean: failed to launch update tab: {err}");
            }
        }
    }

    fn split_active_pane(&mut self, direction: SplitDirection) -> Result<(), String> {
        let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &self.font_sys) else {
            return Ok(());
        };
        if self.tabs.is_empty() {
            return self.spawn_new_tab(false);
        }

        let phys = gl_state.window.inner_size();
        let sf = gl_state.window.scale_factor();
        let num_tabs = self.tabs.len();
        let tab_h = RenderEngine::tab_bar_height(&self.config, font_sys, num_tabs);
        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);
        let active_pane_id = self.tabs[self.active_tab].active_pane_id;

        let resolved_dir = if direction == SplitDirection::Auto {
            let (cur_layouts, _) = compute_split_layout(
                &self.tabs[self.active_tab].root,
                &self.config,
                font_sys,
                phys.width.max(64),
                phys.height.max(64),
                sf,
                tab_h,
                status_h,
            );
            let is_wider = cur_layouts
                .iter()
                .find(|l| l.pane_id == active_pane_id)
                .is_none_or(|l| l.bounds_w >= l.bounds_h);
            if is_wider {
                SplitDirection::Right
            } else {
                SplitDirection::Down
            }
        } else {
            direction
        };

        let new_id = self.next_pane_id;
        self.next_pane_id += 1;

        let mut prospective_root = self.tabs[self.active_tab].root.clone();
        if !prospective_root.split_leaf(active_pane_id, new_id, resolved_dir) {
            return Ok(());
        }
        let (prospective_layouts, _) = compute_split_layout(
            &prospective_root,
            &self.config,
            font_sys,
            phys.width.max(64),
            phys.height.max(64),
            sf,
            tab_h,
            status_h,
        );
        let (cols, rows) = prospective_layouts
            .iter()
            .find(|l| l.pane_id == new_id)
            .map(|l| (l.cols, l.rows))
            .unwrap_or((40, 12));

        let cwd = self.inherited_cwd(false);
        let session = TerminalSession::spawn(
            new_id,
            &self.config,
            None,
            cwd,
            cols,
            rows,
            font_sys.metrics.cell_width as u16,
            font_sys.metrics.cell_height as u16,
            self.ipc_socket.as_deref(),
            self.proxy.clone(),
            Arc::clone(&self.wakeup_queued),
        )?;

        let tab = &mut self.tabs[self.active_tab];
        tab.root = prospective_root;
        tab.panes.push(session);
        tab.active_pane_id = new_id;

        self.resize_tab(self.active_tab);
        self.sync_window_title();
        self.request_redraw();
        Ok(())
    }

    fn active_tab_layout(&self) -> (Vec<PaneLayout>, Vec<SplitDivider>) {
        let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &self.font_sys) else {
            return (Vec::new(), Vec::new());
        };
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return (Vec::new(), Vec::new());
        };
        let phys = gl_state.window.inner_size();
        if phys.width == 0 || phys.height == 0 {
            return (Vec::new(), Vec::new());
        }
        let sf = gl_state.window.scale_factor();
        let tab_h = RenderEngine::tab_bar_height(&self.config, font_sys, self.tabs.len());
        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);

        match self.editor.mode {
            crate::editor::WorkspaceMode::PureEditor => (Vec::new(), Vec::new()),
            crate::editor::WorkspaceMode::Split => {
                let (_ed_vp, _div, term_rect) = self.editor.compute_layout(
                    phys.width as f32,
                    phys.height as f32,
                    tab_h,
                    status_h,
                );
                if let Some((tx, ty, tw, th)) = term_rect {
                    crate::split::compute_split_layout_in_rect(
                        &tab.root,
                        &self.config,
                        font_sys,
                        tx,
                        ty,
                        tw,
                        th,
                        sf,
                    )
                } else {
                    (Vec::new(), Vec::new())
                }
            }
            crate::editor::WorkspaceMode::PureTerminal => compute_split_layout(
                &tab.root,
                &self.config,
                font_sys,
                phys.width,
                phys.height,
                sf,
                tab_h,
                status_h,
            ),
        }
    }

    fn resize_tab(&mut self, tab_idx: usize) {
        let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &self.font_sys) else {
            return;
        };
        let phys = gl_state.window.inner_size();
        if phys.width == 0 || phys.height == 0 || tab_idx >= self.tabs.len() {
            return;
        }
        let sf = gl_state.window.scale_factor();
        let num_tabs = self.tabs.len();
        let tab_h = RenderEngine::tab_bar_height(&self.config, font_sys, num_tabs);
        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);
        let cw = font_sys.metrics.cell_width as u16;
        let ch = font_sys.metrics.cell_height as u16;

        let tab = &mut self.tabs[tab_idx];
        let layouts = match self.editor.mode {
            crate::editor::WorkspaceMode::PureEditor => Vec::new(),
            crate::editor::WorkspaceMode::Split => {
                let (_ed_vp, _div, term_rect) = self.editor.compute_layout(
                    phys.width as f32,
                    phys.height as f32,
                    tab_h,
                    status_h,
                );
                if let Some((tx, ty, tw, th)) = term_rect {
                    let (layouts, _) = crate::split::compute_split_layout_in_rect(
                        &tab.root,
                        &self.config,
                        font_sys,
                        tx,
                        ty,
                        tw,
                        th,
                        sf,
                    );
                    layouts
                } else {
                    Vec::new()
                }
            }
            crate::editor::WorkspaceMode::PureTerminal => {
                let (layouts, _) = compute_split_layout(
                    &tab.root,
                    &self.config,
                    font_sys,
                    phys.width,
                    phys.height,
                    sf,
                    tab_h,
                    status_h,
                );
                layouts
            }
        };

        for layout in layouts {
            if let Some(pane) = tab.pane_by_id_mut(layout.pane_id) {
                pane.resize(layout.cols, layout.rows, cw, ch);
            }
        }
    }

    fn resize_all_tabs(&mut self) {
        for idx in 0..self.tabs.len() {
            self.resize_tab(idx);
        }
    }

    fn close_pane_by_id(&mut self, pane_id: u64, event_loop: &ActiveEventLoop) {
        let Some(tab_idx) = self
            .tabs
            .iter()
            .position(|t| t.panes.iter().any(|p| p.id == pane_id))
        else {
            return;
        };

        if self.tabs.len() <= 1 && self.tabs[tab_idx].panes.len() <= 1 && self.editor.has_unsaved()
        {
            self.request_exit(event_loop);
            return;
        }

        self.close_context_menu();
        self.dragging_divider = None;
        if self.selecting_pane_id == Some(pane_id) {
            self.selecting_pane_id = None;
            self.mouse_left_down = false;
        }

        let tab = &mut self.tabs[tab_idx];
        if let Some(pos) = tab.panes.iter().position(|p| p.id == pane_id) {
            let removed = tab.panes.remove(pos);
            removed.shutdown();
        }

        let old_root = std::mem::replace(&mut tab.root, SplitNode::Leaf(0));
        let (maybe_new_root, suggested_focus) = old_root.remove_leaf(pane_id);

        if let Some(new_root) = maybe_new_root
            && !tab.panes.is_empty()
        {
            tab.root = new_root;
            if tab.active_pane_id == pane_id {
                tab.active_pane_id = suggested_focus.unwrap_or(tab.panes[0].id);
            }
            self.resize_tab(tab_idx);
            self.sync_window_title();
            self.request_redraw();
        } else {
            self.tabs.remove(tab_idx);
            if self.tabs.is_empty() {
                event_loop.exit();
                return;
            }
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            } else if tab_idx < self.active_tab {
                self.active_tab -= 1;
            }
            self.resize_all_tabs();
            self.sync_window_title();
            self.request_redraw();
        }
    }

    fn close_tab_at(&mut self, tab_idx: usize, event_loop: &ActiveEventLoop) {
        if tab_idx >= self.tabs.len() {
            return;
        }
        if self.tabs.len() <= 1 && self.editor.has_unsaved() {
            self.request_exit(event_loop);
            return;
        }
        self.close_context_menu();
        self.dragging_divider = None;
        self.selecting_pane_id = None;
        self.mouse_left_down = false;

        let removed = self.tabs.remove(tab_idx);
        removed.shutdown_all();
        if self.tabs.is_empty() {
            event_loop.exit();
            return;
        }
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if tab_idx < self.active_tab {
            self.active_tab -= 1;
        }
        self.resize_all_tabs();
        self.sync_window_title();
        self.request_redraw();
    }

    fn focus_split(&mut self, target: SplitFocusTarget) {
        let (layouts, _) = self.active_tab_layout();
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if let Some(next_id) = find_neighbor_pane(&layouts, tab.active_pane_id, target) {
            tab.active_pane_id = next_id;
            self.sync_window_title();
            self.reset_cursor_blink();
            self.request_redraw();
        }
    }

    fn sync_window_title(&self) {
        let Some(gl_state) = &self.gl_state else {
            return;
        };
        if let Some(fixed) = &self.config.title {
            gl_state.window.set_title(fixed);
        } else if let Some(tab) = self.tabs.get(self.active_tab) {
            gl_state.window.set_title(tab.title());
        }
    }

    fn request_redraw(&self) {
        if self.occluded {
            return;
        }
        if let Some(gl_state) = &self.gl_state {
            gl_state.window.request_redraw();
        }
    }

    fn active_session(&self) -> Option<&TerminalSession> {
        self.tabs.get(self.active_tab).and_then(Tab::active_pane)
    }

    fn set_cursor_icon(&mut self, icon: CursorIcon) {
        if self.current_cursor_icon == icon {
            return;
        }
        self.current_cursor_icon = icon;
        if let Some(gl_state) = &self.gl_state {
            gl_state.window.set_cursor(icon);
        }
    }

    fn set_mouse_hidden(&mut self, hidden: bool) {
        if self.mouse_hidden == hidden {
            return;
        }
        self.mouse_hidden = hidden;
        if let Some(gl_state) = &self.gl_state {
            gl_state.window.set_cursor_visible(!hidden);
        }
    }

    fn current_status_bar_geometry(&self) -> Option<StatusBarGeometry> {
        let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &self.font_sys) else {
            return None;
        };
        let phys = gl_state.window.inner_size();
        if phys.width == 0 || phys.height == 0 {
            return None;
        }
        let sf = gl_state.window.scale_factor();
        let (layouts, _) = self.active_tab_layout();
        let active_pane_id = self
            .tabs
            .get(self.active_tab)
            .map(|t| t.active_pane_id)
            .unwrap_or(0);
        let active_idx = layouts
            .iter()
            .position(|l| l.pane_id == active_pane_id)
            .unwrap_or(0);
        let (display_offset, tui_mode) = self
            .active_session()
            .map(|s| {
                let term = s.term.lock();
                let offset = term.grid().display_offset();
                let tui = term
                    .mode()
                    .intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE);
                (offset, tui)
            })
            .unwrap_or((0, false));

        let ctx = StatusBarContext {
            active_pane_index: active_idx,
            num_panes: layouts.len().max(1),
            num_tabs: self.tabs.len().max(1),
            display_offset,
            tui_mode,
            context_menu: self.context_menu.as_ref(),
            help_dialog: self.help_dialog.as_ref(),
            available_update: self.available_update.as_ref(),
        };
        StatusBarGeometry::compute(
            &self.config,
            &ctx,
            font_sys.metrics.cell_width * crate::statusbar::STATUS_BAR_TEXT_SCALE,
            font_sys.metrics.cell_height * crate::statusbar::STATUS_BAR_TEXT_SCALE,
            phys.width,
            phys.height,
            sf,
        )
    }

    fn active_cursor_menu_anchor(&self) -> Option<(f32, f32)> {
        let font_sys = self.font_sys.as_ref()?;
        let tab = self.tabs.get(self.active_tab)?;
        let (layouts, _) = self.active_tab_layout();
        let layout = layouts
            .iter()
            .find(|l| l.pane_id == tab.active_pane_id)
            .or_else(|| layouts.first())?;
        let session = tab.pane_by_id(layout.pane_id)?;
        let (cursor_col, cursor_row) = {
            let term = session.term.lock();
            let grid = term.grid();
            let pt = grid.cursor.point;
            let c = pt.column.0.min(grid.columns().saturating_sub(1));
            let r = (pt.line.0.max(0) as usize).min(grid.screen_lines().saturating_sub(1));
            (c, r)
        };
        let cw = font_sys.metrics.cell_width;
        let ch = font_sys.metrics.cell_height;
        let max_x = (layout.bounds_x + layout.bounds_w - 8.0).max(layout.bounds_x + 4.0);
        let max_y = (layout.bounds_y + layout.bounds_h - 4.0).max(layout.bounds_y + 4.0);
        let x = (layout.origin_x + cursor_col as f32 * cw).clamp(layout.bounds_x + 4.0, max_x);
        let y = (layout.origin_y + (cursor_row + 1) as f32 * ch + 4.0)
            .clamp(layout.bounds_y + 4.0, max_y);
        Some((x, y))
    }

    fn open_context_menu(&mut self, px: f32, py: f32, via_keyboard: bool, open_themes: bool) {
        let has_selection = self
            .active_session()
            .is_some_and(TerminalSession::has_selection);
        let num_panes = self
            .tabs
            .get(self.active_tab)
            .map(|t| t.panes.len())
            .unwrap_or(1);
        if self.theme_before_preview.is_none() {
            self.theme_before_preview = Some(self.config.theme_name.clone());
        }
        let mut menu = ContextMenu::build(
            px,
            py,
            has_selection,
            num_panes,
            self.config.window_show_status_bar,
            &self.config.keybinds,
            self.available_update.as_ref(),
        );
        if open_themes {
            menu.open_theme_submenu(&self.config.theme_name);
        } else if via_keyboard {
            menu.select_first_enabled();
        } else if let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &self.font_sys) {
            let phys = gl_state.window.inner_size();
            let sf = gl_state.window.scale_factor();
            let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);
            let geom = menu.compute_geometry(
                font_sys.metrics.cell_width,
                font_sys.metrics.cell_height,
                phys.width,
                phys.height,
                sf,
                status_h,
            );
            let _ = menu.update_hover(&geom, px, py);
        }
        self.help_dialog = None;
        self.context_menu = Some(menu);
        self.sync_theme_preview();
        self.status_hover.clear();
        self.mouse_left_down = false;
        self.selecting_pane_id = None;
        self.dragging_divider = None;
        self.set_cursor_icon(CursorIcon::Default);
        self.request_redraw();
    }

    fn open_context_menu_at_cursor(&mut self, open_themes: bool) {
        let (px, py) = self.active_cursor_menu_anchor().unwrap_or((48.0, 48.0));
        self.open_context_menu(px, py, true, open_themes);
    }

    fn sync_theme_preview(&mut self) {
        let target_theme = if let Some(menu) = &self.context_menu {
            menu.hovered_theme_id().map(|s| s.to_string())
        } else {
            None
        };

        if let Some(theme_id) = target_theme {
            if self.config.theme_name != theme_id {
                self.apply_theme_preview(&theme_id);
            }
        } else if let Some(orig) = &self.theme_before_preview {
            if &self.config.theme_name != orig {
                let orig_clone = orig.clone();
                self.apply_theme_preview(&orig_clone);
            }
        }
    }

    fn apply_theme_preview(&mut self, name: &str) {
        if let Some(pal) = ThemePalette::from_name(name) {
            self.config.theme_name = pal.name.clone();
            self.config.colors = pal;
            self.request_redraw();
        }
    }

    fn close_context_menu(&mut self) {
        self.context_menu = None;
        if let Some(orig) = self.theme_before_preview.take() {
            if self.config.theme_name != orig {
                self.apply_theme(&orig);
            }
        }
    }

    fn apply_theme(&mut self, name: &str) {
        if let Some(pal) = ThemePalette::from_name(name) {
            self.config.theme_name = pal.name.clone();
            self.config.colors = pal;
            self.request_redraw();
        }
    }

    fn borderless_resize_at(&self, px: f32, py: f32) -> Option<(ResizeDirection, CursorIcon)> {
        if self.config.effective_decorations() {
            return None;
        }
        let gl_state = self.gl_state.as_ref()?;
        if gl_state.window.fullscreen().is_some() || gl_state.window.is_maximized() {
            return None;
        }
        let phys = gl_state.window.inner_size();
        let w = phys.width as f32;
        let h = phys.height as f32;
        let sf = gl_state.window.scale_factor() as f32;
        let edge = (5.0 * sf).round().max(4.0);
        let corner = (14.0 * sf).round().max(10.0);

        let near_l = px < edge;
        let near_r = px >= w - edge;
        let near_t = py < edge;
        let near_b = py >= h - edge;
        if !(near_l || near_r || near_t || near_b) {
            return None;
        }

        let in_l_corner = px < corner;
        let in_r_corner = px >= w - corner;
        let in_t_corner = py < corner;
        let in_b_corner = py >= h - corner;

        if (near_l && in_t_corner) || (near_t && in_l_corner) {
            Some((ResizeDirection::NorthWest, CursorIcon::NwResize))
        } else if (near_r && in_t_corner) || (near_t && in_r_corner) {
            Some((ResizeDirection::NorthEast, CursorIcon::NeResize))
        } else if (near_l && in_b_corner) || (near_b && in_l_corner) {
            Some((ResizeDirection::SouthWest, CursorIcon::SwResize))
        } else if (near_r && in_b_corner) || (near_b && in_r_corner) {
            Some((ResizeDirection::SouthEast, CursorIcon::SeResize))
        } else if near_l {
            Some((ResizeDirection::West, CursorIcon::WResize))
        } else if near_r {
            Some((ResizeDirection::East, CursorIcon::EResize))
        } else if near_t {
            Some((ResizeDirection::North, CursorIcon::NResize))
        } else {
            Some((ResizeDirection::South, CursorIcon::SResize))
        }
    }

    fn reload_configuration(&mut self) {
        let new_cfg = Config::load(self.launch_opts.config_path.as_deref());
        self.base_font_size = new_cfg.font_size;
        self.config = new_cfg;
        self.editor.vim.enabled = self.config.editor_keymap == crate::config::EditorKeymap::Vim;
        self.editor.vim.reset_to_normal();
        if let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &mut self.font_sys) {
            let sf = gl_state.window.scale_factor();
            font_sys.update_size_or_config(&self.config, sf, true);
            gl_state
                .window
                .set_decorations(self.config.effective_decorations());
        }
        for tab in &self.tabs {
            for pane in &tab.panes {
                pane.update_config(&self.config);
            }
        }
        self.render_engine.invalidate_caches();
        self.resize_all_tabs();
        self.sync_window_title();
        self.request_redraw();
    }

    fn adjust_font_size(&mut self, new_size: f32) {
        let clamped = new_size.clamp(6.0, 96.0);
        if (clamped - self.config.font_size).abs() < 0.01 {
            return;
        }
        self.config.font_size = clamped;
        if let (Some(gl_state), Some(font_sys)) = (&self.gl_state, &mut self.font_sys) {
            let sf = gl_state.window.scale_factor();
            font_sys.update_size_or_config(&self.config, sf, false);
        }
        self.render_engine.invalidate_caches();
        self.resize_all_tabs();
        self.request_redraw();
    }

    fn execute_action(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::CopyToClipboard => {
                if let Some(session) = self.active_session()
                    && let Some(text) =
                        session.selected_text(self.config.clipboard_trim_trailing_spaces)
                    && let Some(cb) = &mut self.clipboard
                {
                    let _ = cb.set_text(text);
                }
            }
            Action::PasteFromClipboard => {
                let text = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok());
                if let Some(txt) = text
                    && !txt.is_empty()
                    && let Some(session) = self.active_session()
                {
                    let bracketed = session
                        .term
                        .lock()
                        .mode()
                        .contains(TermMode::BRACKETED_PASTE);
                    session.scroll(Scroll::Bottom);
                    session.write_bytes(encode_paste(&txt, bracketed));
                }
            }
            Action::IncreaseFontSize(step) => {
                self.adjust_font_size(self.config.font_size + step);
            }
            Action::DecreaseFontSize(step) => {
                self.adjust_font_size(self.config.font_size - step);
            }
            Action::ResetFontSize => {
                self.adjust_font_size(self.base_font_size);
            }
            Action::ScrollPageUp => {
                if let Some(s) = self.active_session() {
                    s.scroll(Scroll::PageUp);
                    self.request_redraw();
                }
            }
            Action::ScrollPageDown => {
                if let Some(s) = self.active_session() {
                    s.scroll(Scroll::PageDown);
                    self.request_redraw();
                }
            }
            Action::ScrollToTop => {
                if let Some(s) = self.active_session() {
                    s.scroll(Scroll::Top);
                    self.request_redraw();
                }
            }
            Action::ScrollToBottom => {
                if let Some(s) = self.active_session() {
                    s.scroll(Scroll::Bottom);
                    self.request_redraw();
                }
            }
            Action::ScrollPageLines(lines) => {
                if let Some(s) = self.active_session() {
                    s.scroll(Scroll::Delta(-lines));
                    self.request_redraw();
                }
            }
            Action::ClearScreen => {
                if let Some(s) = self.active_session() {
                    s.write_bytes(b"\x0c".to_vec());
                }
            }
            Action::ReloadConfig => {
                self.reload_configuration();
            }
            Action::NewTab => {
                let _ = self.spawn_new_tab(false);
            }
            Action::NewSplit(dir) => {
                let _ = self.split_active_pane(dir);
            }
            Action::GotoSplit(target) => {
                self.focus_split(target);
            }
            Action::ResizeSplit(dir, px) => {
                let span = self
                    .gl_state
                    .as_ref()
                    .map(|gs| {
                        let phys = gs.window.inner_size();
                        let sf = gs.window.scale_factor() as f32;
                        let dim = match dir {
                            SplitDirection::Left | SplitDirection::Right | SplitDirection::Auto => {
                                phys.width as f32
                            }
                            SplitDirection::Up | SplitDirection::Down => phys.height as f32,
                        };
                        (dim / sf.max(0.5)).max(200.0)
                    })
                    .unwrap_or(800.0);
                let delta = (px.max(4) as f32 / span).clamp(0.02, 0.30);
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let pane_id = tab.active_pane_id;
                    if tab.root.resize_for_leaf(pane_id, dir, delta) {
                        self.resize_tab(self.active_tab);
                        self.request_redraw();
                    }
                }
            }
            Action::EqualizeSplits => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.root.equalize();
                    self.resize_tab(self.active_tab);
                    self.request_redraw();
                }
            }
            Action::CloseSurface => {
                if let Some(pane_id) = self.active_session().map(|s| s.id) {
                    self.close_pane_by_id(pane_id, event_loop);
                }
            }
            Action::CloseTab => {
                if !self.tabs.is_empty() {
                    self.close_tab_at(self.active_tab, event_loop);
                }
            }
            Action::NextTab => {
                if self.tabs.len() > 1 {
                    self.active_tab = (self.active_tab + 1) % self.tabs.len();
                    self.close_context_menu();
                    self.sync_window_title();
                    self.request_redraw();
                }
            }
            Action::PreviousTab => {
                if self.tabs.len() > 1 {
                    self.active_tab = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
                    self.close_context_menu();
                    self.sync_window_title();
                    self.request_redraw();
                }
            }
            Action::GotoTab(idx) => {
                if idx < self.tabs.len() {
                    self.active_tab = idx;
                    self.close_context_menu();
                    self.sync_window_title();
                    self.request_redraw();
                }
            }
            Action::NewWindow => {
                if let Ok(exe) = std::env::current_exe() {
                    let mut cmd = Command::new(exe);
                    if let Some(cfg_path) = &self.launch_opts.config_path {
                        cmd.arg("--config-file").arg(cfg_path);
                    }
                    cmd.arg(format!("--theme={}", self.config.theme_name));
                    let _ = cmd.spawn();
                }
            }
            Action::ToggleFullscreen => {
                if let Some(gl_state) = &self.gl_state {
                    let next = if gl_state.window.fullscreen().is_some() {
                        None
                    } else {
                        Some(Fullscreen::Borderless(None))
                    };
                    gl_state.window.set_fullscreen(next);
                }
            }
            Action::ToggleWindowDecorations => {
                let next = !self.config.effective_decorations();
                self.config.window_titlebar = next;
                self.config.window_decoration = next;
                if let Some(gl_state) = &self.gl_state {
                    gl_state.window.set_decorations(next);
                }
                self.request_redraw();
            }
            Action::ToggleStatusBar => {
                self.config.window_show_status_bar = !self.config.window_show_status_bar;
                self.status_hover.clear();
                self.resize_all_tabs();
                self.request_redraw();
            }
            Action::ToggleContextMenu => {
                if self.context_menu.is_some() {
                    self.close_context_menu();
                    self.request_redraw();
                } else {
                    self.open_context_menu_at_cursor(false);
                }
            }
            Action::ToggleMyanmarZawgyi => {
                self.config.myanmar_zawgyi = match self.config.myanmar_zawgyi {
                    ZawgyiMode::Off => ZawgyiMode::Auto,
                    ZawgyiMode::Auto | ZawgyiMode::Force => ZawgyiMode::Off,
                };
                self.render_engine.shaper.clear_cache();
                self.request_redraw();
            }
            Action::SetTheme(name) => {
                self.theme_before_preview = None;
                self.apply_theme(&name);
                if let Err(err) = self.config.save_theme(&name) {
                    eprintln!("ocean: failed saving theme to config: {err}");
                }
            }
            Action::NextTheme => {
                let names = ThemePalette::builtin_names();
                let next_idx = names
                    .iter()
                    .position(|&n| n == self.config.theme_name)
                    .map(|i| (i + 1) % names.len())
                    .unwrap_or(0);
                if let Some(&next_name) = names.get(next_idx) {
                    self.theme_before_preview = None;
                    self.apply_theme(next_name);
                    if let Err(err) = self.config.save_theme(next_name) {
                        eprintln!("ocean: failed saving theme to config: {err}");
                    }
                }
            }
            Action::ShowHelp => {
                if self.help_dialog.is_some() {
                    self.help_dialog = None;
                } else {
                    self.help_dialog = Some(HelpDialog::new());
                }
                self.close_context_menu();
                self.dragging_divider = None;
                self.mouse_left_down = false;
                self.selecting_pane_id = None;
                self.set_cursor_icon(CursorIcon::Default);
                self.request_redraw();
            }
            Action::EditConfig => {
                self.close_context_menu();
                self.help_dialog = None;
                self.open_config_in_editor();
            }
            Action::Quit => {
                self.request_exit(event_loop);
            }
            Action::ToggleEditor => {
                self.editor.mode = match self.editor.mode {
                    crate::editor::WorkspaceMode::PureEditor => {
                        crate::editor::WorkspaceMode::PureTerminal
                    }
                    _ => crate::editor::WorkspaceMode::PureEditor,
                };
                if self.editor.is_active() && !self.editor.explorer.is_open() {
                    let ws_dir = self
                        .inherited_cwd(false)
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    self.editor.open_folder(&ws_dir);
                }
                if self.editor.is_active() && self.editor.documents.is_empty() {
                    let _ = self
                        .editor
                        .open_file(std::path::Path::new("untitled.txt"), self.proxy.clone());
                }
                self.editor.editor_focused = self.editor.is_active();
                self.resize_all_tabs();
                self.sync_window_title();
                self.request_redraw();
            }
            Action::ToggleSplitMode => {
                self.editor.mode = match self.editor.mode {
                    crate::editor::WorkspaceMode::Split => {
                        crate::editor::WorkspaceMode::PureTerminal
                    }
                    _ => crate::editor::WorkspaceMode::Split,
                };
                if self.editor.is_active() && !self.editor.explorer.is_open() {
                    let ws_dir = self
                        .inherited_cwd(false)
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    self.editor.open_folder(&ws_dir);
                }
                if self.editor.is_active() && self.editor.documents.is_empty() {
                    let _ = self
                        .editor
                        .open_file(std::path::Path::new("untitled.txt"), self.proxy.clone());
                }
                self.editor.editor_focused = self.editor.is_active();
                self.resize_all_tabs();
                self.sync_window_title();
                self.request_redraw();
            }
            Action::SendText(bytes) => {
                if let Some(s) = self.active_session() {
                    s.write_bytes(bytes);
                }
            }
            Action::TriggerUpdate => {
                self.trigger_app_update();
            }
            Action::Ignore | Action::Unbind => {}
        }
    }

    fn reset_cursor_blink(&mut self) {
        self.cursor_blink_on = true;
        self.last_blink_toggle = Instant::now();
    }

    fn current_confirm_layout(&self) -> Option<ConfirmLayout> {
        let gl_state = self.gl_state.as_ref()?;
        let font_sys = self.font_sys.as_ref()?;
        let dialog = self.confirm_dialog.as_ref()?;
        let sf = gl_state.window.scale_factor() as f32;
        let phys = gl_state.window.inner_size();
        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf as f64);
        Some(dialog.layout(
            phys.width as f32,
            phys.height as f32,
            font_sys.metrics.cell_width,
            font_sys.metrics.cell_height,
            sf,
            status_h,
        ))
    }

    fn open_close_document_confirm(&mut self) {
        let document = self.editor.active_doc_ref();
        let files = vec![document.file_name()];
        let can_save = document.path().is_some();
        self.confirm_dialog = Some(ConfirmDialog::close_document(files, can_save));
        self.request_redraw();
    }

    fn handle_ipc_editor_request(&mut self, request: crate::ipc::IpcEditorRequest) {
        if let Some(tab_idx) = self
            .tabs
            .iter()
            .position(|tab| tab.panes.iter().any(|pane| pane.id == request.pane_id))
        {
            self.active_tab = tab_idx;
            if let Some(tab) = self.tabs.get_mut(tab_idx) {
                tab.active_pane_id = request.pane_id;
            }
        }

        self.editor.mode = crate::editor::WorkspaceMode::PureEditor;
        self.editor.editor_focused = true;
        self.editor.active_panel = crate::editor::SidebarPanel::Explorer;

        let path = request.path.as_ref().map(PathBuf::from);
        match path {
            Some(path) if path.is_dir() => {
                self.editor.open_folder(&path);
            }
            Some(path) => {
                let parent = path.parent().map(Path::to_path_buf);
                let _ = self.editor.open_file(&path, self.proxy.clone());
                if let Some(parent) = parent {
                    self.editor.open_folder(parent);
                }
            }
            None => {
                if let Some(cwd) = request.cwd {
                    self.editor.open_folder(cwd);
                }
            }
        }

        self.resize_all_tabs();
        self.sync_window_title();
        self.request_redraw();
        if let Some(gl_state) = &self.gl_state {
            gl_state.window.focus_window();
        }
    }

    fn request_exit(&mut self, event_loop: &ActiveEventLoop) {
        self.close_context_menu();
        self.help_dialog = None;
        let dirty = self.editor.dirty_documents();
        if dirty.is_empty() {
            self.perform_exit(event_loop);
            return;
        }
        let can_save_all = dirty.iter().all(|(_, has_path)| *has_path);
        let files = dirty.into_iter().map(|(name, _)| name).collect();
        self.confirm_dialog = Some(ConfirmDialog::quit(files, can_save_all));
        self.request_redraw();
    }

    fn perform_exit(&mut self, event_loop: &ActiveEventLoop) {
        for tab in &self.tabs {
            tab.shutdown_all();
        }
        if let Some(path) = &self.ipc_socket {
            let _ = std::fs::remove_file(path);
        }
        if let Some(gl_state) = self.gl_state.take() {
            gl_state.renderer.destroy(&gl_state.gl);
        }
        event_loop.exit();
    }

    fn apply_confirm_choice(&mut self, choice: ConfirmChoice, event_loop: &ActiveEventLoop) {
        let Some(mut dialog) = self.confirm_dialog.take() else {
            return;
        };
        match choice {
            ConfirmChoice::Cancel => {}
            ConfirmChoice::Save => {
                if let Err(err) = self.editor.save_active_document() {
                    dialog.set_error(err);
                    self.confirm_dialog = Some(dialog);
                } else {
                    self.editor.close_active_document();
                }
            }
            ConfirmChoice::Discard => {
                self.editor.close_active_document();
            }
            ConfirmChoice::SaveAll => {
                if let Err(err) = self.editor.save_all_dirty() {
                    dialog.set_error(err);
                    self.confirm_dialog = Some(dialog);
                } else {
                    self.perform_exit(event_loop);
                    return;
                }
            }
            ConfirmChoice::DiscardAll => {
                self.perform_exit(event_loop);
                return;
            }
        }
        self.request_redraw();
    }

    fn sync_ime_policy(&mut self) {
        let editor_owns_input = self.editor.is_active() && self.editor.editor_focused;
        let allowed = !editor_owns_input || self.editor.accepts_text_input();
        if self.ime_allowed == allowed {
            return;
        }
        self.ime_allowed = allowed;
        if let Some(gl_state) = &self.gl_state {
            gl_state.window.set_ime_allowed(allowed);
        }
    }

    fn render_frame(&mut self) {
        if self.occluded {
            return;
        }
        self.sync_ime_policy();
        let (layouts, dividers) = self.active_tab_layout();
        let (Some(gl_state), Some(font_sys)) = (&mut self.gl_state, &mut self.font_sys) else {
            return;
        };
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };

        let phys: PhysicalSize<u32> = gl_state.window.inner_size();
        if phys.width == 0 || phys.height == 0 {
            return;
        }
        let sf = gl_state.window.scale_factor();
        let editor_exclusive = self.editor.mode == crate::editor::WorkspaceMode::PureEditor;
        let tab_h = if editor_exclusive {
            0.0
        } else {
            RenderEngine::tab_bar_height(&self.config, font_sys, self.tabs.len())
        };
        let status_h = if editor_exclusive {
            0.0
        } else {
            RenderEngine::status_bar_height(&self.config, font_sys, sf)
        };

        let (ed_vp, split_div) = if self.editor.is_active() {
            let (vp, div, _) =
                self.editor
                    .compute_layout(phys.width as f32, phys.height as f32, tab_h, status_h);
            (Some(vp), div)
        } else {
            (None, None)
        };

        if self.snapshot_bufs.len() < layouts.len() {
            self.snapshot_bufs
                .resize_with(layouts.len(), TerminalSnapshot::default);
        }

        let active_pane_id = tab.active_pane_id;
        let mut active_ime_px: Option<(u32, u32)> = None;

        for (i, layout) in layouts.iter().enumerate() {
            if let Some(pane) = tab.pane_by_id(layout.pane_id) {
                pane.snapshot(&self.config, &mut self.snapshot_bufs[i]);
                if layout.pane_id == active_pane_id {
                    let snap = &self.snapshot_bufs[i];
                    let cx = (layout.origin_x
                        + snap.cursor_col as f32 * font_sys.metrics.cell_width)
                        .round()
                        .max(0.0) as u32;
                    let cy = (layout.origin_y
                        + snap.cursor_row as f32 * font_sys.metrics.cell_height)
                        .round()
                        .max(0.0) as u32;
                    active_ime_px = Some((cx, cy));
                }
            }
        }

        // Anchor IME to editor cursor when editor is active and focused
        if self.editor.is_active() && self.editor.editor_focused {
            if let Some(ed_viewport) = ed_vp {
                let gutter_digits = (self.editor.active_doc().buffer.len_lines() + 1)
                    .to_string()
                    .len()
                    .max(3);
                let line_h = font_sys.metrics.cell_height;
                let gutter_w = (gutter_digits as f32 + 3.0) * font_sys.metrics.cell_width;
                let (rel_cx, rel_cy) = self
                    .editor
                    .active_doc()
                    .cursor_visual_pos(font_sys, gutter_w);
                let ed_tab_h = crate::editor::EditorWorkspace::editor_tab_height(line_h);
                let crumb_h = self.editor.breadcrumb_height(line_h);
                let activity_w = if self.editor.activity_bar_visible {
                    44.0
                } else {
                    0.0
                };
                let panel_open = self.editor.explorer.visible
                    && (self.editor.active_panel == crate::editor::SidebarPanel::Problems
                        || self.editor.explorer.is_open());
                let sidebar_w = if panel_open {
                    if self.editor.active_panel == crate::editor::SidebarPanel::Problems {
                        self.editor
                            .explorer
                            .width
                            .max(230.0)
                            .min((ed_viewport.w - 160.0).max(80.0))
                    } else {
                        self.editor
                            .explorer
                            .width
                            .min(ed_viewport.w - 100.0)
                            .max(80.0)
                    }
                } else {
                    0.0
                };
                let code_x = ed_viewport.x + activity_w + sidebar_w;
                let abs_cx = (code_x + rel_cx).round().max(0.0) as u32;
                let abs_cy = (ed_viewport.y + ed_tab_h + crumb_h + rel_cy)
                    .round()
                    .max(0.0) as u32;
                active_ime_px = Some((abs_cx, abs_cy));
            }
        }

        // Keep IME candidate popup anchored cleanly to the active split pane's cursor
        if let Some((cur_px_x, cur_px_y)) = active_ime_px
            && (cur_px_x, cur_px_y) != self.last_ime_spot
        {
            self.last_ime_spot = (cur_px_x, cur_px_y);
            gl_state.window.set_ime_cursor_area(
                LogicalPosition::new(cur_px_x as f64 / sf, cur_px_y as f64 / sf),
                LogicalSize::new(
                    font_sys.metrics.cell_width as f64 / sf,
                    font_sys.metrics.cell_height as f64 / sf,
                ),
            );
        }

        let pane_scenes: Vec<PaneScene<'_>> = layouts
            .iter()
            .enumerate()
            .map(|(i, layout)| {
                let is_active = layout.pane_id == active_pane_id;
                PaneScene {
                    snapshot: &self.snapshot_bufs[i],
                    layout: *layout,
                    is_active,
                    ime_preedit: if is_active {
                        self.ime_preedit.as_deref()
                    } else {
                        None
                    },
                }
            })
            .collect();

        let tab_titles: Vec<TabTitle<'_>> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, t)| TabTitle {
                title: t.title(),
                active: idx == self.active_tab,
            })
            .collect();

        let blink_visible = if self.config.cursor_blink && self.focused {
            self.cursor_blink_on
        } else {
            true
        };

        let ed_ref = if self.editor.is_active() {
            Some(&mut self.editor)
        } else {
            None
        };

        let tabs_for_scene = if editor_exclusive {
            &tab_titles[..0]
        } else {
            &tab_titles[..]
        };

        let (atlas, instances) = self.render_engine.build_scene(
            &self.config,
            font_sys,
            &pane_scenes,
            &dividers,
            phys.width,
            phys.height,
            sf,
            tabs_for_scene,
            self.context_menu.as_ref(),
            self.help_dialog.as_ref(),
            self.available_update.as_ref(),
            self.status_hover.hovered_id,
            blink_visible,
            self.focused,
            ed_ref,
            ed_vp,
            split_div,
            self.ime_preedit.as_deref(),
            editor_exclusive,
            self.confirm_dialog.as_ref(),
        );

        gl_state.renderer.draw(
            &gl_state.gl,
            atlas,
            instances,
            phys.width,
            phys.height,
            self.config.colors.background,
            self.config.background_opacity,
        );

        let _ = gl_state.gl_surface.swap_buffers(&gl_state.gl_context);
    }
}

impl ApplicationHandler<AppEvent> for OceanApp {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause
            && self.config.cursor_blink
            && self.focused
            && !self.occluded
        {
            self.cursor_blink_on = !self.cursor_blink_on;
            self.last_blink_toggle = Instant::now();
            self.request_redraw();
        }

        // Lightweight mtime check for live config hot-reload (at most once every 2 seconds)
        if self.last_config_poll.elapsed() >= Duration::from_secs(2) {
            self.last_config_poll = Instant::now();
            if self.config.check_modified() {
                self.reload_configuration();
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.occluded = true;
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.occluded = false;
        if self.gl_state.is_none() {
            if let Err(err) = self.init_window_and_gl(event_loop) {
                eprintln!("ocean fatal error: {err}");
                event_loop.exit();
            }
        } else {
            self.reset_cursor_blink();
            self.request_redraw();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Wakeup => {
                self.wakeup_queued.store(false, Ordering::Release);
                if let Some(font_sys) = &mut self.font_sys {
                    if self.editor.poll_events(font_sys) {
                        self.request_redraw();
                    }
                }
                self.request_redraw();
            }
            AppEvent::TitleChanged(pane_id, maybe_title) => {
                let mut changed_active_tab = false;
                for (t_idx, tab) in self.tabs.iter_mut().enumerate() {
                    if let Some(pane) = tab.pane_by_id_mut(pane_id) {
                        pane.title = maybe_title.unwrap_or_else(|| "ocean".to_string());
                        if t_idx == self.active_tab && tab.active_pane_id == pane_id {
                            changed_active_tab = true;
                        }
                        break;
                    }
                }
                if changed_active_tab {
                    self.sync_window_title();
                }
                if self.tabs.len() > 1 {
                    self.request_redraw();
                }
            }
            AppEvent::ClipboardStore(text) => {
                if let Some(cb) = &mut self.clipboard {
                    let _ = cb.set_text(text);
                }
            }
            AppEvent::PtyReply(pane_id, reply) => {
                for tab in &self.tabs {
                    if let Some(pane) = tab.pane_by_id(pane_id) {
                        pane.write_bytes(reply.into_bytes());
                        break;
                    }
                }
            }
            AppEvent::ColorQuery(pane_id, index, fmt) => {
                let rgb = match index {
                    256 => self.config.colors.foreground,
                    257 => self.config.colors.background,
                    258 => self.config.colors.cursor_color,
                    i if i < 256 => self.config.colors.ansi[i],
                    _ => self.config.colors.background,
                };
                let reply = fmt(VteRgb {
                    r: rgb.r,
                    g: rgb.g,
                    b: rgb.b,
                });
                for tab in &self.tabs {
                    if let Some(pane) = tab.pane_by_id(pane_id) {
                        pane.write_bytes(reply.into_bytes());
                        break;
                    }
                }
            }
            AppEvent::IpcEditorRequest(request) => {
                self.handle_ipc_editor_request(request);
            }
            AppEvent::ChildExited(pane_id) => {
                self.close_pane_by_id(pane_id, event_loop);
                if self.config.check_modified() {
                    self.reload_configuration();
                }
            }
            AppEvent::UpdateAvailable(info) => {
                self.available_update = Some(info);
                self.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.request_exit(event_loop);
            }
            WindowEvent::Resized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
                    self.close_context_menu();
                    if let Some(gl_state) = &self.gl_state {
                        gl_state.gl_surface.resize(
                            &gl_state.gl_context,
                            NonZeroU32::new(new_size.width).unwrap(),
                            NonZeroU32::new(new_size.height).unwrap(),
                        );
                    }
                    self.resize_all_tabs();
                    self.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.close_context_menu();
                if let Some(font_sys) = &mut self.font_sys {
                    font_sys.update_size_or_config(&self.config, scale_factor, false);
                }
                self.render_engine.invalidate_caches();
                self.resize_all_tabs();
                self.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                if !focused {
                    self.close_context_menu();
                    self.dragging_divider = None;
                    self.mouse_left_down = false;
                }
                if let Some(gl_state) = &self.gl_state {
                    gl_state.window.set_ime_allowed(true);
                }
                if let Some(session) = self.active_session() {
                    let focus_mode = session.term.lock().mode().contains(TermMode::FOCUS_IN_OUT);
                    if focus_mode {
                        let seq = if focused { b"\x1b[I" } else { b"\x1b[O" };
                        session.write_bytes(seq.to_vec());
                    }
                }
                self.reset_cursor_blink();
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = new_mods.state();
            }
            WindowEvent::Ime(ime_event) => {
                if self.confirm_dialog.is_some() {
                    self.ime_preedit = None;
                    return;
                }
                match ime_event {
                    Ime::Enabled => {
                        self.ime_preedit = None;
                    }
                    Ime::Preedit(text, _) => {
                        self.ime_preedit = if text.is_empty() { None } else { Some(text) };
                        self.request_redraw();
                    }
                    Ime::Commit(text) => {
                        self.ime_preedit = None;
                        self.close_context_menu();
                        if self.help_dialog.is_some() {
                            return;
                        }
                        if !text.is_empty() {
                            if self.config.mouse_hide_while_typing {
                                self.set_mouse_hidden(true);
                            }

                            // The editor owns text input whenever it has focus.
                            if self.editor.is_active()
                                && (self.editor.editor_focused
                                    || self.editor.mode == crate::editor::WorkspaceMode::PureEditor)
                            {
                                if self.editor.accepts_text_input()
                                    && let Some(font_sys) = &mut self.font_sys
                                {
                                    self.editor.insert_ime_commit(&text, font_sys);
                                }
                                self.reset_cursor_blink();
                                self.request_redraw();
                                return;
                            }

                            if let Some(session) = self.active_session() {
                                session.clear_selection();
                                session.scroll(Scroll::Bottom);
                                session.write_bytes(text.into_bytes());
                            }
                            self.reset_cursor_blink();
                        }
                        self.request_redraw();
                    }
                    Ime::Disabled => {
                        self.ime_preedit = None;
                        self.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if self.confirm_dialog.is_some() {
                    let choice = self.confirm_dialog.as_mut().and_then(|dialog| {
                        dialog.handle_key(&event.logical_key, self.modifiers.control_key())
                    });
                    if let Some(choice) = choice {
                        self.apply_confirm_choice(choice, event_loop);
                    } else {
                        self.request_redraw();
                    }
                    return;
                }

                // 0. Keyboard navigation inside open Help & Documentation dialog
                if self.help_dialog.is_some() {
                    if let Some(action) =
                        match_keybind(&self.config.keybinds, &event.logical_key, self.modifiers)
                            .cloned()
                        && matches!(
                            action,
                            Action::ShowHelp
                                | Action::ToggleContextMenu
                                | Action::ToggleStatusBar
                                | Action::IncreaseFontSize(_)
                                | Action::DecreaseFontSize(_)
                                | Action::ResetFontSize
                                | Action::NextTheme
                                | Action::ToggleFullscreen
                                | Action::Quit
                        )
                    {
                        self.execute_action(action, event_loop);
                        return;
                    }

                    let outcome = if let (Some(gl_state), Some(font_sys), Some(help)) =
                        (&self.gl_state, &self.font_sys, &mut self.help_dialog)
                    {
                        let phys = gl_state.window.inner_size();
                        let sf = gl_state.window.scale_factor();
                        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);
                        let geom = help.compute_geometry(
                            font_sys.metrics.cell_width,
                            font_sys.metrics.cell_height,
                            phys.width,
                            phys.height,
                            sf,
                            status_h,
                        );
                        let total = help.wrapped_lines(geom.max_cols).len();
                        help.handle_key(
                            &event.logical_key,
                            self.modifiers.shift_key(),
                            total,
                            geom.visible_rows,
                        )
                    } else {
                        HelpKeyOutcome::Close
                    };

                    match outcome {
                        HelpKeyOutcome::Close => {
                            self.help_dialog = None;
                            self.request_redraw();
                        }
                        HelpKeyOutcome::Redraw => {
                            self.request_redraw();
                        }
                        HelpKeyOutcome::Consumed => {}
                        HelpKeyOutcome::EditConfig => {
                            self.help_dialog = None;
                            self.open_config_in_editor();
                            self.request_redraw();
                        }
                    }
                    return;
                }

                // 1. Keyboard navigation & dismissal when context menu is open
                if let Some(menu) = &mut self.context_menu {
                    let outcome = menu.handle_key(
                        &event.logical_key,
                        self.modifiers,
                        &self.config.theme_name,
                    );
                    self.sync_theme_preview();
                    match outcome {
                        MenuKeyOutcome::Close => {
                            self.close_context_menu();
                            self.request_redraw();
                            return;
                        }
                        MenuKeyOutcome::Redraw => {
                            self.request_redraw();
                            return;
                        }
                        MenuKeyOutcome::Execute(action) => {
                            if matches!(action, Action::SetTheme(_)) {
                                self.theme_before_preview = None;
                                self.context_menu = None;
                            } else {
                                self.close_context_menu();
                            }
                            self.execute_action(action, event_loop);
                            self.request_redraw();
                            return;
                        }
                        MenuKeyOutcome::NotHandled => {}
                    }
                }

                // Check global workspace toggle & window lifecycle keybinds first
                if let Some(action) =
                    match_keybind(&self.config.keybinds, &event.logical_key, self.modifiers)
                        .cloned()
                {
                    if matches!(
                        action,
                        Action::ToggleEditor
                            | Action::ToggleSplitMode
                            | Action::ToggleFullscreen
                            | Action::Quit
                            | Action::ReloadConfig
                    ) {
                        self.close_context_menu();
                        self.execute_action(action, event_loop);
                        return;
                    }
                }

                // If code editor is active and focused, route keys to the editor
                if self.editor.is_active()
                    && (self.editor.editor_focused
                        || self.editor.mode == crate::editor::WorkspaceMode::PureEditor)
                {
                    let ws_dir = self
                        .inherited_cwd(false)
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    if let Some(font_sys) = &mut self.font_sys {
                        if self.editor.handle_key(
                            &event,
                            self.modifiers,
                            font_sys,
                            &mut self.clipboard,
                            &ws_dir,
                            self.proxy.clone(),
                        ) {
                            if self.editor.take_close_confirm() {
                                self.open_close_document_confirm();
                            }
                            self.reset_cursor_blink();
                            self.sync_ime_policy();
                            self.request_redraw();
                            return;
                        }
                    }
                }

                // Check configured Ghostty-style keybinds
                if let Some(action) =
                    match_keybind(&self.config.keybinds, &event.logical_key, self.modifiers)
                        .cloned()
                {
                    if !matches!(action, Action::ToggleContextMenu) {
                        self.close_context_menu();
                    }
                    self.execute_action(action, event_loop);
                    return;
                }

                if self.context_menu.is_some() {
                    return;
                }

                // Do not emit raw key text while an IME preedit composition is actively open
                if self.ime_preedit.is_some() {
                    return;
                }

                // In Pure Editor mode the terminal pane is hidden; never leak keys into it.
                if self.editor.mode == crate::editor::WorkspaceMode::PureEditor {
                    return;
                }

                let maybe_bytes = self.active_session().and_then(|session| {
                    let mode = *session.term.lock().mode();
                    encode_key_event(&event, self.modifiers, mode)
                });
                if let Some(bytes) = maybe_bytes {
                    if self.config.mouse_hide_while_typing {
                        self.set_mouse_hidden(true);
                    }
                    if let Some(session) = self.active_session() {
                        session.clear_selection();
                        session.scroll(Scroll::Bottom);
                        session.write_bytes(bytes);
                    }
                    self.reset_cursor_blink();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = position;
                self.set_mouse_hidden(false);
                let px = position.x as f32;
                let py = position.y as f32;

                // Interactive hover on Neovim-style bottom shortcut bar
                if self.dragging_divider.is_none()
                    && !self.mouse_left_down
                    && (self.help_dialog.is_some()
                        || self.context_menu.is_some()
                        || self.borderless_resize_at(px, py).is_none())
                    && let Some(sb_geom) = self.current_status_bar_geometry()
                    && py >= sb_geom.y
                {
                    let (changed, pointer) = self.status_hover.update(&sb_geom, px, py);
                    self.set_cursor_icon(if pointer {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    });
                    if changed {
                        self.request_redraw();
                    }
                    return;
                }
                if self.status_hover.clear() {
                    self.request_redraw();
                }

                // 0. Hover tracking inside open Help & Documentation dialog
                if self.help_dialog.is_some() {
                    let hover_res = if let (Some(gl_state), Some(font_sys), Some(help)) =
                        (&self.gl_state, &self.font_sys, &mut self.help_dialog)
                    {
                        let phys = gl_state.window.inner_size();
                        let sf = gl_state.window.scale_factor();
                        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);
                        let geom = help.compute_geometry(
                            font_sys.metrics.cell_width,
                            font_sys.metrics.cell_height,
                            phys.width,
                            phys.height,
                            sf,
                            status_h,
                        );
                        Some(help.update_hover(&geom, px, py))
                    } else {
                        None
                    };
                    if let Some((changed, pointer)) = hover_res {
                        self.set_cursor_icon(if pointer {
                            CursorIcon::Pointer
                        } else {
                            CursorIcon::Default
                        });
                        if changed {
                            self.request_redraw();
                        }
                    }
                    return;
                }

                // 1. Hover tracking inside open context menu & Switch Theme submenu
                if self.context_menu.is_some() {
                    let hover_res = if let (Some(gl_state), Some(font_sys), Some(menu)) =
                        (&self.gl_state, &self.font_sys, &mut self.context_menu)
                    {
                        let phys = gl_state.window.inner_size();
                        let sf = gl_state.window.scale_factor();
                        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);
                        let geom = menu.compute_geometry(
                            font_sys.metrics.cell_width,
                            font_sys.metrics.cell_height,
                            phys.width,
                            phys.height,
                            sf,
                            status_h,
                        );
                        Some(menu.update_hover(&geom, px, py))
                    } else {
                        None
                    };
                    if let Some((changed, pointer)) = hover_res {
                        self.sync_theme_preview();
                        self.set_cursor_icon(if pointer {
                            CursorIcon::Pointer
                        } else {
                            CursorIcon::Default
                        });
                        if changed {
                            self.request_redraw();
                        }
                    }
                    return;
                }

                // 2. Interactive split divider dragging
                if let Some(drag) = &self.dragging_divider {
                    let coord = match drag.axis {
                        SplitAxis::Horizontal => px,
                        SplitAxis::Vertical => py,
                    };
                    let new_ratio =
                        ((coord - drag.container_start) / drag.container_span).clamp(0.12, 0.88);
                    let path = drag.path.clone();
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.root.set_ratio_at_path(&path, new_ratio);
                    }
                    self.resize_tab(self.active_tab);
                    self.request_redraw();
                    return;
                }

                // Interactive editor/terminal split divider dragging
                if self.editor.dragging_split_divider {
                    let sf = self
                        .gl_state
                        .as_ref()
                        .map(|g| g.window.scale_factor() as f32)
                        .unwrap_or(1.0);
                    let (avail_w, avail_h) = self
                        .gl_state
                        .as_ref()
                        .map(|g| {
                            let sz = g.window.inner_size();
                            (sz.width as f32, sz.height as f32)
                        })
                        .unwrap_or((800.0, 600.0));
                    let tab_h = self
                        .font_sys
                        .as_ref()
                        .map(|fs| RenderEngine::tab_bar_height(&self.config, fs, self.tabs.len()))
                        .unwrap_or(0.0);
                    let status_h = self
                        .font_sys
                        .as_ref()
                        .map(|fs| RenderEngine::status_bar_height(&self.config, fs, sf as f64))
                        .unwrap_or(0.0);

                    match self.editor.split_axis {
                        SplitAxis::Vertical => {
                            let total_h = (avail_h - tab_h - status_h - 4.0).max(10.0);
                            let rel_y = (py - tab_h).clamp(60.0, total_h - 60.0);
                            self.editor.split_ratio = (rel_y / total_h).clamp(0.15, 0.85);
                        }
                        SplitAxis::Horizontal => {
                            let total_w = (avail_w - 4.0).max(10.0);
                            let rel_x = px.clamp(60.0, total_w - 60.0);
                            self.editor.split_ratio = (rel_x / total_w).clamp(0.15, 0.85);
                        }
                    }
                    self.resize_all_tabs();
                    self.request_redraw();
                    return;
                }

                // Interactive file explorer sidebar divider dragging
                if self.editor.explorer.dragging_divider {
                    let (win_w, win_h) = self
                        .gl_state
                        .as_ref()
                        .map(|g| {
                            let sz = g.window.inner_size();
                            (sz.width as f32, sz.height as f32)
                        })
                        .unwrap_or((800.0, 600.0));
                    let tab_h = self
                        .font_sys
                        .as_ref()
                        .map(|fs| RenderEngine::tab_bar_height(&self.config, fs, self.tabs.len()))
                        .unwrap_or(0.0);
                    let (ed_vp, _, _) = self.editor.compute_layout(win_w, win_h, tab_h, 0.0);
                    let new_w = (px - ed_vp.x).clamp(80.0, (ed_vp.w - 100.0).max(80.0));
                    self.editor.explorer.width = new_w;
                    self.request_redraw();
                    return;
                }

                // Editor mouse selection drag or scrollbar drag / hover
                if self.editor.is_active() && self.editor.editor_focused {
                    let (win_w, win_h) = self
                        .gl_state
                        .as_ref()
                        .map(|g| {
                            let sz = g.window.inner_size();
                            (sz.width as f32, sz.height as f32)
                        })
                        .unwrap_or((800.0, 600.0));
                    let tab_h = self
                        .font_sys
                        .as_ref()
                        .map(|fs| RenderEngine::tab_bar_height(&self.config, fs, self.tabs.len()))
                        .unwrap_or(0.0);
                    let status_h = self
                        .font_sys
                        .as_ref()
                        .map(|fs| RenderEngine::status_bar_height(&self.config, fs, 1.0))
                        .unwrap_or(0.0);
                    let (ed_vp, _, _) = self.editor.compute_layout(win_w, win_h, tab_h, status_h);

                    if self.editor.dragging_scrollbar {
                        if let Some(font_sys) = &mut self.font_sys {
                            if self
                                .editor
                                .handle_scrollbar_drag(py, &ed_vp, tab_h, font_sys)
                            {
                                self.request_redraw();
                            }
                            return;
                        }
                    } else if self.mouse_left_down {
                        if let Some(font_sys) = &mut self.font_sys {
                            self.editor
                                .handle_mouse_drag(px, py, &ed_vp, tab_h, font_sys);
                            self.request_redraw();
                            return;
                        }
                    } else if let Some(font_sys) = &mut self.font_sys {
                        if self
                            .editor
                            .update_scrollbar_hover(px, py, &ed_vp, tab_h, font_sys)
                        {
                            self.request_redraw();
                        }
                    }
                }

                let (layouts, dividers) = self.active_tab_layout();

                // 3. Active mouse drag (selection or PTY mouse motion)
                if self.mouse_left_down
                    && let Some(sel_id) = self.selecting_pane_id
                    && let Some(layout) = layouts.iter().find(|l| l.pane_id == sel_id)
                    && let Some(font_sys) = &self.font_sys
                {
                    let clamped_x = px.clamp(layout.bounds_x, layout.bounds_x + layout.bounds_w);
                    let clamped_y = py.clamp(layout.bounds_y, layout.bounds_y + layout.bounds_h);
                    let (col, row, side) = layout.pixel_to_cell(
                        clamped_x,
                        clamped_y,
                        font_sys.metrics.cell_width,
                        font_sys.metrics.cell_height,
                    );
                    if let Some(pane) = self
                        .tabs
                        .get(self.active_tab)
                        .and_then(|t| t.pane_by_id(sel_id))
                    {
                        let mode = *pane.term.lock().mode();
                        if mode.intersects(TermMode::MOUSE_MODE) && !self.modifiers.shift_key() {
                            if let Some(seq) = encode_mouse_button(
                                MouseButton::Left,
                                true,
                                true,
                                col,
                                row,
                                self.modifiers,
                                mode,
                            ) {
                                pane.write_bytes(seq);
                            }
                        } else {
                            pane.update_selection(col, row, side);
                            self.request_redraw();
                        }
                    }
                    return;
                }

                // 4. Passive PTY mouse-motion reporting & cursor icon updates
                let Some((sf, win_w, win_h, tab_h, cw, ch)) = self
                    .gl_state
                    .as_ref()
                    .zip(self.font_sys.as_ref())
                    .map(|(gs, fs)| {
                        (
                            gs.window.scale_factor() as f32,
                            gs.window.inner_size().width as f32,
                            gs.window.inner_size().height as f32,
                            RenderEngine::tab_bar_height(&self.config, fs, self.tabs.len()),
                            fs.metrics.cell_width,
                            fs.metrics.cell_height,
                        )
                    })
                else {
                    return;
                };

                if let Some((_, resize_icon)) = self.borderless_resize_at(px, py) {
                    self.set_cursor_icon(resize_icon);
                    return;
                }
                if tab_h > 0.0 && py < tab_h {
                    let (tab_w, plus_x, plus_w) =
                        RenderEngine::tab_bar_metrics(win_w, tab_h, self.tabs.len());
                    let over_tabs_or_plus = px < self.tabs.len() as f32 * tab_w
                        || (px >= plus_x && px <= plus_x + plus_w);
                    self.set_cursor_icon(if over_tabs_or_plus {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    });
                    return;
                }
                let slop = (4.0 * sf).round().max(3.0);
                if let Some(div) = dividers.iter().find(|d| d.hit_test(px, py, slop)) {
                    self.set_cursor_icon(match div.axis {
                        SplitAxis::Horizontal => CursorIcon::ColResize,
                        SplitAxis::Vertical => CursorIcon::RowResize,
                    });
                    return;
                }

                if self.editor.is_active() {
                    let (_ed_vp, split_div, _) =
                        self.editor.compute_layout(win_w, win_h, tab_h, 0.0);
                    if let Some((sx, sy, sw, sh)) = split_div {
                        let slop = 4.0;
                        if px >= sx - slop
                            && px <= sx + sw + slop
                            && py >= sy - slop
                            && py <= sy + sh + slop
                        {
                            self.set_cursor_icon(match self.editor.split_axis {
                                SplitAxis::Vertical => CursorIcon::RowResize,
                                SplitAxis::Horizontal => CursorIcon::ColResize,
                            });
                            return;
                        }
                    }

                    if self.editor.explorer.visible && self.editor.explorer.is_open() {
                        let sw = self.editor.explorer.width.min(_ed_vp.w - 100.0).max(80.0);
                        let div_x = _ed_vp.x + sw;
                        if (px - div_x).abs() <= 4.0 && py >= _ed_vp.y && py <= _ed_vp.y + _ed_vp.h
                        {
                            self.set_cursor_icon(CursorIcon::ColResize);
                            return;
                        }
                        if px >= _ed_vp.x
                            && px < div_x
                            && py >= _ed_vp.y
                            && py <= _ed_vp.y + _ed_vp.h
                        {
                            self.set_cursor_icon(CursorIcon::Pointer);
                            if let Some(font_sys) = &self.font_sys {
                                let line_h = font_sys.metrics.cell_height;
                                let header_h = (line_h + 6.0).round();
                                let item_h = (line_h + 3.0).round();
                                self.editor
                                    .explorer
                                    .update_hover(px, py, _ed_vp.x, _ed_vp.y, header_h, item_h);
                                self.request_redraw();
                            }
                            return;
                        } else if self.editor.explorer.hovered_idx.is_some() {
                            self.editor.explorer.hovered_idx = None;
                            self.request_redraw();
                        }
                    }
                }

                self.set_cursor_icon(CursorIcon::Text);

                if let Some(layout) = pane_at_point(&layouts, px, py)
                    && let Some(pane) = self
                        .tabs
                        .get(self.active_tab)
                        .and_then(|t| t.pane_by_id(layout.pane_id))
                {
                    let mode = *pane.term.lock().mode();
                    if mode.contains(TermMode::MOUSE_MOTION) && !self.modifiers.shift_key() {
                        let (col, row, _) = layout.pixel_to_cell(px, py, cw, ch);
                        if let Some(seq) = encode_mouse_button(
                            MouseButton::Left,
                            false,
                            true,
                            col,
                            row,
                            self.modifiers,
                            mode,
                        ) {
                            pane.write_bytes(seq);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.set_mouse_hidden(false);
                let px = self.cursor_pos.x as f32;
                let py = self.cursor_pos.y as f32;

                if self.confirm_dialog.is_some() {
                    let choice = if state == ElementState::Pressed {
                        self.current_confirm_layout().and_then(|layout| {
                            self.confirm_dialog
                                .as_ref()
                                .and_then(|dialog| dialog.hit_test(&layout, px, py))
                        })
                    } else {
                        None
                    };
                    if let Some(choice) = choice {
                        self.apply_confirm_choice(choice, event_loop);
                    }
                    return;
                }

                if state == ElementState::Released && button == MouseButton::Left {
                    self.editor.dragging_split_divider = false;
                    self.editor.explorer.dragging_divider = false;
                    self.editor.dragging_scrollbar = false;
                    if self.dragging_divider.take().is_some() {
                        return;
                    }
                }

                // Clicks on the Neovim-style bottom shortcut & mode status bar
                if state == ElementState::Pressed
                    && (self.help_dialog.is_some()
                        || self.context_menu.is_some()
                        || self.borderless_resize_at(px, py).is_none())
                    && let Some(sb_geom) = self.current_status_bar_geometry()
                    && py >= sb_geom.y
                {
                    match button {
                        MouseButton::Left => {
                            if let Some(target) = sb_geom.click_at(px, py) {
                                match target {
                                    StatusClickTarget::Execute(Action::ToggleContextMenu) => {
                                        if self.context_menu.is_some() {
                                            self.close_context_menu();
                                            self.request_redraw();
                                        } else if px <= sb_geom.mode_pill.rect.w + 8.0 {
                                            self.open_context_menu(
                                                8.0,
                                                sb_geom.y + 8.0,
                                                true,
                                                false,
                                            );
                                        } else {
                                            self.execute_action(
                                                Action::ToggleContextMenu,
                                                event_loop,
                                            );
                                        }
                                    }
                                    StatusClickTarget::Execute(act) => {
                                        if matches!(act, Action::SetTheme(_)) {
                                            self.theme_before_preview = None;
                                            self.context_menu = None;
                                        } else {
                                            self.close_context_menu();
                                        }
                                        if !matches!(act, Action::ShowHelp) {
                                            self.help_dialog = None;
                                        }
                                        self.execute_action(act, event_loop);
                                    }
                                    StatusClickTarget::OpenThemeMenu => {
                                        let already_theme_mode =
                                            self.context_menu.as_ref().is_some_and(|m| {
                                                m.submenu_open && m.sub_hovered.is_some()
                                            });
                                        if already_theme_mode {
                                            self.close_context_menu();
                                            self.request_redraw();
                                        } else if let Some(menu) = &mut self.context_menu {
                                            menu.open_theme_submenu(&self.config.theme_name);
                                            self.sync_theme_preview();
                                            self.request_redraw();
                                        } else {
                                            self.open_context_menu(
                                                (sb_geom.w - 8.0).max(8.0),
                                                sb_geom.y + 8.0,
                                                true,
                                                true,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        MouseButton::Right => {
                            self.open_context_menu(px, sb_geom.y + 8.0, false, false);
                        }
                        _ => {}
                    }
                    return;
                }

                // 0. Handle clicks when Help & Documentation modal dialog is open
                if self.help_dialog.is_some() {
                    if state != ElementState::Pressed {
                        return;
                    }
                    if button == MouseButton::Right {
                        self.help_dialog = None;
                        self.open_context_menu(px, py, false, false);
                        return;
                    }
                    if button == MouseButton::Left {
                        let outcome = if let (Some(gl_state), Some(font_sys), Some(help)) =
                            (&self.gl_state, &self.font_sys, &mut self.help_dialog)
                        {
                            let phys = gl_state.window.inner_size();
                            let sf = gl_state.window.scale_factor();
                            let status_h =
                                RenderEngine::status_bar_height(&self.config, font_sys, sf);
                            let geom = help.compute_geometry(
                                font_sys.metrics.cell_width,
                                font_sys.metrics.cell_height,
                                phys.width,
                                phys.height,
                                sf,
                                status_h,
                            );
                            help.handle_click(&geom, px, py)
                        } else {
                            HelpClickOutcome::Close
                        };
                        match outcome {
                            HelpClickOutcome::Close => {
                                self.help_dialog = None;
                                self.request_redraw();
                            }
                            HelpClickOutcome::Redraw => {
                                self.request_redraw();
                            }
                            HelpClickOutcome::Consumed => {}
                            HelpClickOutcome::EditConfig => {
                                self.help_dialog = None;
                                self.open_config_in_editor();
                                self.request_redraw();
                            }
                        }
                    }
                    return;
                }

                // 1. Handle clicks when context menu (or Switch Theme submenu) is open
                if self.context_menu.is_some() {
                    if state != ElementState::Pressed {
                        return;
                    }
                    let outcome = if let (Some(gl_state), Some(font_sys), Some(menu)) =
                        (&self.gl_state, &self.font_sys, &mut self.context_menu)
                    {
                        let phys = gl_state.window.inner_size();
                        let sf = gl_state.window.scale_factor();
                        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);
                        let geom = menu.compute_geometry(
                            font_sys.metrics.cell_width,
                            font_sys.metrics.cell_height,
                            phys.width,
                            phys.height,
                            sf,
                            status_h,
                        );
                        menu.handle_click(&geom, px, py)
                    } else {
                        MenuClickOutcome::Outside
                    };

                    match outcome {
                        MenuClickOutcome::Execute(action) => {
                            if matches!(button, MouseButton::Left | MouseButton::Right) {
                                if matches!(action, Action::SetTheme(_)) {
                                    self.theme_before_preview = None;
                                    self.context_menu = None;
                                } else {
                                    self.close_context_menu();
                                }
                                self.execute_action(action, event_loop);
                                self.request_redraw();
                            }
                            return;
                        }
                        MenuClickOutcome::ConsumedKeepOpen => {
                            self.sync_theme_preview();
                            self.request_redraw();
                            return;
                        }
                        MenuClickOutcome::Outside => {
                            self.close_context_menu();
                            self.request_redraw();
                            if button != MouseButton::Right {
                                return;
                            }
                            // For Right-Click outside, fall through to open context menu at the new spot
                        }
                    }
                }

                // 2. Borderless window edge drag-resize when system title bar is hidden
                if state == ElementState::Pressed
                    && button == MouseButton::Left
                    && let Some((dir, _)) = self.borderless_resize_at(px, py)
                {
                    if let Some(gl_state) = &self.gl_state {
                        let _ = gl_state.window.drag_resize_window(dir);
                    }
                    return;
                }

                // 3. Super + Left Drag moves window cleanly even when titlebar & tabbar are hidden
                if state == ElementState::Pressed
                    && button == MouseButton::Left
                    && self.modifiers.super_key()
                    && !self.config.effective_decorations()
                {
                    if let Some(gl_state) = &self.gl_state {
                        let _ = gl_state.window.drag_window();
                    }
                    return;
                }

                // 4. Tab bar interaction (switch tab, middle-click close, `+` button, drag empty bar, right-click menu)
                let tab_h = self
                    .font_sys
                    .as_ref()
                    .map(|fs| RenderEngine::tab_bar_height(&self.config, fs, self.tabs.len()))
                    .unwrap_or(0.0);

                if state == ElementState::Pressed
                    && tab_h > 0.0
                    && py < tab_h
                    && !self.tabs.is_empty()
                {
                    let win_w = self
                        .gl_state
                        .as_ref()
                        .map(|g| g.window.inner_size().width as f32)
                        .unwrap_or(800.0);
                    let (tab_w, plus_x, plus_w) =
                        RenderEngine::tab_bar_metrics(win_w, tab_h, self.tabs.len());
                    let clicked_tab = (px / tab_w).floor() as usize;

                    match button {
                        MouseButton::Left => {
                            if clicked_tab < self.tabs.len() && px < self.tabs.len() as f32 * tab_w
                            {
                                self.active_tab = clicked_tab;
                                self.sync_window_title();
                                self.request_redraw();
                            } else if px >= plus_x && px <= plus_x + plus_w {
                                let _ = self.spawn_new_tab(false);
                            } else if let Some(gl_state) = &self.gl_state {
                                let now = Instant::now();
                                if now.duration_since(self.last_tabbar_empty_click)
                                    <= Duration::from_millis(380)
                                {
                                    gl_state
                                        .window
                                        .set_maximized(!gl_state.window.is_maximized());
                                } else {
                                    let _ = gl_state.window.drag_window();
                                }
                                self.last_tabbar_empty_click = now;
                            }
                        }
                        MouseButton::Middle => {
                            if clicked_tab < self.tabs.len() && px < self.tabs.len() as f32 * tab_w
                            {
                                self.close_tab_at(clicked_tab, event_loop);
                            }
                        }
                        MouseButton::Right => {
                            if clicked_tab < self.tabs.len() && px < self.tabs.len() as f32 * tab_w
                            {
                                self.active_tab = clicked_tab;
                                self.sync_window_title();
                            }
                            self.open_context_menu(px, py, false, false);
                        }
                        _ => {}
                    }
                    return;
                }

                // Editor viewport & split divider mouse interactions
                if self.editor.is_active() {
                    let win_w = self
                        .gl_state
                        .as_ref()
                        .map(|g| g.window.inner_size().width as f32)
                        .unwrap_or(800.0);
                    let win_h = self
                        .gl_state
                        .as_ref()
                        .map(|g| g.window.inner_size().height as f32)
                        .unwrap_or(600.0);
                    let sf = self
                        .gl_state
                        .as_ref()
                        .map(|g| g.window.scale_factor() as f32)
                        .unwrap_or(1.0);
                    let status_h = self
                        .font_sys
                        .as_ref()
                        .map(|fs| RenderEngine::status_bar_height(&self.config, fs, sf as f64))
                        .unwrap_or(0.0);
                    let (ed_vp, split_div, _) =
                        self.editor.compute_layout(win_w, win_h, tab_h, status_h);

                    // Check click on editor/terminal split divider in Split mode
                    if let Some((sx, sy, sw, sh)) = split_div {
                        let slop = 4.0;
                        if state == ElementState::Pressed
                            && button == MouseButton::Left
                            && px >= sx - slop
                            && px <= sx + sw + slop
                            && py >= sy - slop
                            && py <= sy + sh + slop
                        {
                            self.editor.dragging_split_divider = true;
                            return;
                        }
                    }

                    // Check click inside editor viewport
                    if px >= ed_vp.x
                        && px <= ed_vp.x + ed_vp.w
                        && py >= ed_vp.y
                        && py <= ed_vp.y + ed_vp.h
                    {
                        self.editor.editor_focused = true;
                        self.sync_ime_policy();
                        if state == ElementState::Pressed {
                            if button == MouseButton::Right {
                                self.open_context_menu(px, py, false, false);
                                return;
                            }
                            if let Some(font_sys) = &mut self.font_sys {
                                self.editor.handle_mouse_click(
                                    button,
                                    px,
                                    py,
                                    &ed_vp,
                                    tab_h,
                                    font_sys,
                                    self.modifiers,
                                    self.proxy.clone(),
                                );
                            }
                            if self.editor.take_close_confirm() {
                                self.open_close_document_confirm();
                            }
                            self.reset_cursor_blink();
                            self.request_redraw();
                            return;
                        }
                    } else if state == ElementState::Pressed {
                        self.editor.editor_focused = false;
                        self.sync_ime_policy();
                    }
                }

                // 5. Split divider drag-to-resize
                let (layouts, dividers) = self.active_tab_layout();
                if state == ElementState::Pressed
                    && button == MouseButton::Left
                    && let Some(gl_state) = &self.gl_state
                {
                    let slop = (4.0 * gl_state.window.scale_factor() as f32)
                        .round()
                        .max(3.0);
                    if let Some(div) = dividers.iter().find(|d| d.hit_test(px, py, slop)) {
                        self.dragging_divider = Some(DividerDragState {
                            path: div.path.clone(),
                            axis: div.axis,
                            container_start: div.container_start,
                            container_span: div.container_span,
                        });
                        return;
                    }
                }

                // 6. Locate target split pane under cursor (and focus it on press)
                let target_layout =
                    if state == ElementState::Released && button == MouseButton::Left {
                        self.selecting_pane_id
                            .and_then(|id| layouts.iter().find(|l| l.pane_id == id))
                            .or_else(|| pane_at_point(&layouts, px, py))
                    } else {
                        pane_at_point(&layouts, px, py)
                    };

                let Some(layout) = target_layout.copied() else {
                    return;
                };

                if state == ElementState::Pressed
                    && let Some(tab) = self.tabs.get_mut(self.active_tab)
                    && tab.active_pane_id != layout.pane_id
                {
                    tab.active_pane_id = layout.pane_id;
                    self.sync_window_title();
                    self.reset_cursor_blink();
                    self.request_redraw();
                }

                let (cw, ch) = self
                    .font_sys
                    .as_ref()
                    .map(|f| (f.metrics.cell_width, f.metrics.cell_height))
                    .unwrap_or((9.0, 20.0));
                let (col, row, side) = layout.pixel_to_cell(px, py, cw, ch);

                // 7. Forward to PTY mouse reporting unless Shift is held to override
                let pty_mouse_mode = self
                    .tabs
                    .get(self.active_tab)
                    .and_then(|t| t.pane_by_id(layout.pane_id))
                    .map(|s| *s.term.lock().mode())
                    .filter(|m| m.intersects(TermMode::MOUSE_MODE) && !self.modifiers.shift_key());

                if let Some(mode) = pty_mouse_mode {
                    if button == MouseButton::Left {
                        self.mouse_left_down = state == ElementState::Pressed;
                        self.selecting_pane_id = if self.mouse_left_down {
                            Some(layout.pane_id)
                        } else {
                            None
                        };
                    }
                    if let Some(seq) = encode_mouse_button(
                        button,
                        state == ElementState::Pressed,
                        false,
                        col,
                        row,
                        self.modifiers,
                        mode,
                    ) && let Some(pane) = self
                        .tabs
                        .get(self.active_tab)
                        .and_then(|t| t.pane_by_id(layout.pane_id))
                    {
                        pane.write_bytes(seq);
                    }
                    return;
                }

                match (button, state) {
                    (MouseButton::Right, ElementState::Pressed) => {
                        self.open_context_menu(px, py, false, false);
                    }
                    (MouseButton::Left, ElementState::Pressed) => {
                        // Ctrl + Left Click -> Open URL in default browser
                        if self.config.link_url
                            && self.modifiers.control_key()
                            && let Some(pane) = self
                                .tabs
                                .get(self.active_tab)
                                .and_then(|t| t.pane_by_id(layout.pane_id))
                            && let Some(url) = pane.extract_url_at(col, row)
                        {
                            let _ = Command::new("xdg-open").arg(url).spawn();
                            return;
                        }

                        self.mouse_left_down = true;
                        self.selecting_pane_id = Some(layout.pane_id);
                        let now = Instant::now();
                        if now.duration_since(self.last_click_time) <= Duration::from_millis(400)
                            && self.last_click_cell == (layout.pane_id, col, row)
                        {
                            self.click_streak = (self.click_streak % 3) + 1;
                        } else {
                            self.click_streak = 1;
                        }
                        self.last_click_time = now;
                        self.last_click_cell = (layout.pane_id, col, row);

                        let sel_ty = match self.click_streak {
                            2 => SelectionType::Semantic,
                            3 => SelectionType::Lines,
                            _ if self.modifiers.alt_key() => SelectionType::Block,
                            _ => SelectionType::Simple,
                        };
                        if let Some(pane) = self
                            .tabs
                            .get(self.active_tab)
                            .and_then(|t| t.pane_by_id(layout.pane_id))
                        {
                            pane.start_selection(col, row, side, sel_ty);
                            self.request_redraw();
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        self.mouse_left_down = false;
                        self.selecting_pane_id = None;
                        if self.config.copy_on_select {
                            self.execute_action(Action::CopyToClipboard, event_loop);
                        }
                    }
                    (MouseButton::Middle, ElementState::Pressed) => {
                        self.execute_action(Action::PasteFromClipboard, event_loop);
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.context_menu.take().is_some() {
                    self.request_redraw();
                }

                let px = self.cursor_pos.x as f32;
                let py = self.cursor_pos.y as f32;

                // Priority 1: Code Editor (when active and cursor is over editor viewport)
                if self.editor.is_active() {
                    let win_w = self
                        .gl_state
                        .as_ref()
                        .map(|g| g.window.inner_size().width as f32)
                        .unwrap_or(800.0);
                    let win_h = self
                        .gl_state
                        .as_ref()
                        .map(|g| g.window.inner_size().height as f32)
                        .unwrap_or(600.0);
                    let sf = self
                        .gl_state
                        .as_ref()
                        .map(|g| g.window.scale_factor() as f32)
                        .unwrap_or(1.0);
                    let tab_h = self
                        .font_sys
                        .as_ref()
                        .map(|fs| RenderEngine::tab_bar_height(&self.config, fs, self.tabs.len()))
                        .unwrap_or(0.0);
                    let status_h = self
                        .font_sys
                        .as_ref()
                        .map(|fs| RenderEngine::status_bar_height(&self.config, fs, sf as f64))
                        .unwrap_or(0.0);
                    let (ed_vp, _, _) = self.editor.compute_layout(win_w, win_h, tab_h, status_h);
                    if px >= ed_vp.x
                        && px <= ed_vp.x + ed_vp.w
                        && py >= ed_vp.y
                        && py <= ed_vp.y + ed_vp.h
                    {
                        if let Some(font_sys) = &mut self.font_sys {
                            let multiplier = self.config.mouse_scroll_multiplier;
                            if self
                                .editor
                                .handle_scroll(delta, font_sys, px, py, &ed_vp, multiplier)
                            {
                                self.request_redraw();
                            }
                            return;
                        }
                    }
                }

                let cell_h = self
                    .font_sys
                    .as_ref()
                    .map(|f| f.metrics.cell_height as f64)
                    .unwrap_or(20.0);

                let lines: i32 = match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        (y * self.config.mouse_scroll_multiplier).round() as i32
                    }
                    MouseScrollDelta::PixelDelta(pos) => {
                        self.pixel_scroll_accum +=
                            pos.y * self.config.mouse_scroll_multiplier as f64;
                        let steps = (self.pixel_scroll_accum / cell_h).trunc() as i32;
                        self.pixel_scroll_accum -= steps as f64 * cell_h;
                        steps
                    }
                };

                if lines == 0 {
                    return;
                }

                // Scroll inside Help & Documentation dialog when open
                if self.help_dialog.is_some() {
                    let scrolled = if let (Some(gl_state), Some(font_sys), Some(help)) =
                        (&self.gl_state, &self.font_sys, &mut self.help_dialog)
                    {
                        let phys = gl_state.window.inner_size();
                        let sf = gl_state.window.scale_factor();
                        let status_h = RenderEngine::status_bar_height(&self.config, font_sys, sf);
                        let geom = help.compute_geometry(
                            font_sys.metrics.cell_width,
                            font_sys.metrics.cell_height,
                            phys.width,
                            phys.height,
                            sf,
                            status_h,
                        );
                        let total = help.wrapped_lines(geom.max_cols).len();
                        help.scroll_lines(-lines, total, geom.visible_rows)
                    } else {
                        false
                    };
                    if scrolled {
                        self.request_redraw();
                    }
                    return;
                }

                let (layouts, _) = self.active_tab_layout();
                let Some(layout) = pane_at_point(&layouts, px, py).copied() else {
                    return;
                };
                let (cw, ch) = self
                    .font_sys
                    .as_ref()
                    .map(|f| (f.metrics.cell_width, f.metrics.cell_height))
                    .unwrap_or((9.0, 20.0));
                let (col, row, _) = layout.pixel_to_cell(px, py, cw, ch);

                if let Some(pane) = self
                    .tabs
                    .get(self.active_tab)
                    .and_then(|t| t.pane_by_id(layout.pane_id))
                {
                    let mode = *pane.term.lock().mode();
                    if !self.modifiers.shift_key()
                        && let Some(seq) =
                            encode_mouse_wheel(lines > 0, col, row, self.modifiers, mode)
                    {
                        for _ in 0..lines.unsigned_abs().min(10) {
                            pane.write_bytes(seq.clone());
                        }
                        return;
                    }
                    pane.scroll(Scroll::Delta(lines));
                    self.request_redraw();
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    self.reset_cursor_blink();
                    self.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_frame();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.config.cursor_blink && self.focused && !self.occluded {
            let next = self.last_blink_toggle + Duration::from_millis(530);
            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

#[allow(dead_code)]
fn _assert_cstr(_: CString) {}

fn editor_program() -> String {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let p = Path::new(dir).join("nano");
            if p.is_file() {
                return "nano".to_string();
            }
        }
    }
    for standard in &["/usr/bin/nano", "/bin/nano", "/usr/local/bin/nano"] {
        if Path::new(standard).is_file() {
            return standard.to_string();
        }
    }
    std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string())
}

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icons/64x64/ocean.png");

fn load_window_icon() -> Option<Icon> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(APP_ICON_PNG));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for px in bytes.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        _ => return None,
    };
    Icon::from_rgba(rgba, info.width, info.height).ok()
}
