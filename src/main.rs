use std::{
    cmp::min,
    fs,
    io::{self, Stdout, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, TableState, Tabs, Wrap},
};
use sysinfo::{Disks, Pid, ProcessStatus, ProcessesToUpdate, System};

const VIEWS: [View; 9] = [
    View::Dashboard,
    View::Disk,
    View::Network,
    View::Process,
    View::Cpu,
    View::Memory,
    View::Services,
    View::Logs,
    View::System,
];
const FAST_SELECTION_STEP: isize = 5;

fn main() -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = App::new().run(&mut terminal);
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum View {
    Dashboard,
    Disk,
    Network,
    Process,
    Cpu,
    Memory,
    Services,
    Logs,
    System,
}

impl View {
    fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Disk => "Disk",
            Self::Network => "Network",
            Self::Process => "Process",
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Services => "Services",
            Self::Logs => "Logs",
            Self::System => "System",
        }
    }

    fn previous(self) -> Self {
        let index = self.index();
        VIEWS[(index + VIEWS.len() - 1) % VIEWS.len()]
    }

    fn next(self) -> Self {
        VIEWS[(self.index() + 1) % VIEWS.len()]
    }

    fn index(self) -> usize {
        VIEWS.iter().position(|view| *view == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Focus {
    Tabs,
    Table,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuAction {
    CopyDetails,
    CopyPath,
    CopyPort,
    CopyPid,
    Enter,
    NewFolder,
    Rename,
    CopyTo,
    MoveTo,
    Delete,
    KillProcess,
}

impl MenuAction {
    fn label(self) -> &'static str {
        match self {
            Self::CopyDetails => "Copy details",
            Self::CopyPath => "Copy path",
            Self::CopyPort => "Copy port",
            Self::CopyPid => "Copy PID",
            Self::Enter => "Enter",
            Self::NewFolder => "New folder here",
            Self::Rename => "Rename",
            Self::CopyTo => "Copy to path",
            Self::MoveTo => "Move to path",
            Self::Delete => "Delete",
            Self::KillProcess => "Kill process",
        }
    }
}

#[derive(Clone, Debug)]
enum Modal {
    ActionMenu { actions: Vec<MenuAction> },
    ConfirmDelete(PathBuf),
    ConfirmKill(Pid),
    Prompt(Prompt),
}

#[derive(Clone, Debug)]
enum Prompt {
    Search,
    NewFolder(PathBuf),
    Rename(PathBuf),
    CopyTo(PathBuf),
    MoveTo(PathBuf),
}

impl Prompt {
    fn title(&self) -> &'static str {
        match self {
            Self::Search => "Search keyword",
            Self::NewFolder(_) => "New folder name",
            Self::Rename(_) => "Rename to",
            Self::CopyTo(_) => "Copy to target path",
            Self::MoveTo(_) => "Move to target path",
        }
    }
}

#[derive(Clone, Debug)]
struct FileEntry {
    name: String,
    path: PathBuf,
    kind: FileKind,
    size: Option<u64>,
    modified: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl FileKind {
    fn label(self) -> &'static str {
        match self {
            Self::Directory => "dir",
            Self::File => "file",
            Self::Symlink => "link",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Debug)]
struct NetworkRow {
    proto: String,
    local: String,
    remote: String,
    state: String,
    program: String,
    pid: Option<Pid>,
}

#[derive(Clone, Debug)]
struct ProcessRow {
    pid: Pid,
    cpu: f32,
    memory: u64,
    status: ProcessStatus,
    name: String,
}

#[derive(Clone, Debug)]
struct ServiceRow {
    name: String,
    state: String,
    detail: String,
    pid: Option<Pid>,
}

#[derive(Clone, Debug)]
struct LogRow {
    line: String,
}

#[derive(Clone, Debug)]
struct InfoRow {
    key: String,
    value: String,
}

struct App {
    system: System,
    disks: Disks,
    network: Vec<NetworkRow>,
    processes: Vec<ProcessRow>,
    services: Vec<ServiceRow>,
    logs: Vec<LogRow>,
    files: Vec<FileEntry>,
    view: View,
    focus: Focus,
    disk_path: Option<PathBuf>,
    table_state: TableState,
    menu_state: TableState,
    modal: Option<Modal>,
    prompt_text: String,
    search: String,
    last_refresh: Instant,
    status: String,
    clipboard: String,
}

impl App {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let disks = Disks::new_with_refreshed_list();

        let mut app = Self {
            system,
            disks,
            network: Vec::new(),
            processes: Vec::new(),
            services: Vec::new(),
            logs: Vec::new(),
            files: Vec::new(),
            view: View::Dashboard,
            focus: Focus::Tabs,
            disk_path: None,
            table_state: TableState::default().with_selected(Some(0)),
            menu_state: TableState::default().with_selected(Some(0)),
            modal: None,
            prompt_text: String::new(),
            search: String::new(),
            last_refresh: Instant::now(),
            status: String::new(),
            clipboard: String::new(),
        };
        app.refresh();
        app
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        loop {
            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key) {
                        break;
                    }
                }
            }

            if self.modal.is_none() && self.last_refresh.elapsed() >= Duration::from_secs(3) {
                self.refresh_live_data();
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return true;
        }

        if let Some(Modal::Prompt(_)) = self.modal {
            return self.handle_prompt_key(key);
        }
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('/') | KeyCode::Char('s') => self.open_prompt(Prompt::Search, ""),
            KeyCode::Esc => self.back_or_quit_hint(),
            KeyCode::Left => self.handle_left(),
            KeyCode::Right => self.handle_right(),
            KeyCode::Enter => self.handle_enter(),
            KeyCode::Tab => self.change_view(self.view.next()),
            KeyCode::BackTab => self.change_view(self.view.previous()),
            KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.handle_up_with_step(FAST_SELECTION_STEP)
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.handle_down_with_step(FAST_SELECTION_STEP)
            }
            KeyCode::Up => self.handle_up_with_step(1),
            KeyCode::Down => self.handle_down_with_step(1),
            KeyCode::PageUp => self.move_selection(-self.page_step()),
            KeyCode::PageDown => self.move_selection(self.page_step()),
            KeyCode::Home => self.select_first(),
            KeyCode::End => self.select_last(),
            KeyCode::Char('1') => self.change_view(View::Dashboard),
            KeyCode::Char('2') => self.change_view(View::Disk),
            KeyCode::Char('3') => self.change_view(View::Network),
            KeyCode::Char('4') => self.change_view(View::Process),
            KeyCode::Char('5') => self.change_view(View::Cpu),
            KeyCode::Char('6') => self.change_view(View::Memory),
            KeyCode::Char('7') => self.change_view(View::Services),
            KeyCode::Char('8') => self.change_view(View::Logs),
            KeyCode::Char('9') => self.change_view(View::System),
            KeyCode::Char('r') | KeyCode::Char('R') => self.refresh(),
            _ => {}
        }
        false
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                self.prompt_text.clear();
            }
            KeyCode::Enter => self.submit_prompt(),
            KeyCode::Backspace => {
                self.prompt_text.pop();
            }
            KeyCode::Char(ch) => self.prompt_text.push(ch),
            _ => {}
        }
        false
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') if matches!(self.modal, Some(Modal::ActionMenu { .. })) => {
                self.modal = None
            }
            KeyCode::Esc | KeyCode::Left => self.modal = None,
            KeyCode::Up => self.move_menu_selection(-1),
            KeyCode::Down => self.move_menu_selection(1),
            KeyCode::Enter | KeyCode::Right => self.activate_modal(),
            _ => {}
        }
        false
    }

    fn handle_up_with_step(&mut self, step: isize) {
        if self.focus == Focus::Table && self.table_state.selected().unwrap_or(0) == 0 {
            self.focus = Focus::Tabs;
        } else {
            self.focus = Focus::Table;
            self.move_selection(-step);
        }
    }

    fn handle_down_with_step(&mut self, step: isize) {
        if self.focus == Focus::Tabs {
            self.focus = Focus::Table;
            self.select_first();
        } else {
            self.move_selection(step);
        }
    }

    fn handle_left(&mut self) {
        if self.focus == Focus::Tabs {
            self.change_view_with_focus(self.view.previous(), Focus::Tabs);
        } else {
            self.go_left();
        }
    }

    fn handle_right(&mut self) {
        if self.focus == Focus::Tabs {
            self.change_view_with_focus(self.view.next(), Focus::Tabs);
        } else {
            self.open_action_menu();
        }
    }

    fn handle_enter(&mut self) {
        if self.focus == Focus::Tabs {
            self.focus = Focus::Table;
            self.select_first();
        } else {
            self.enter_selected();
        }
    }

    fn open_prompt(&mut self, prompt: Prompt, initial: &str) {
        self.prompt_text = initial.to_string();
        self.modal = Some(Modal::Prompt(prompt));
    }

    fn submit_prompt(&mut self) {
        let Some(Modal::Prompt(prompt)) = self.modal.clone() else {
            return;
        };
        let value = self.prompt_text.trim().to_string();
        self.modal = None;
        self.prompt_text.clear();

        match prompt {
            Prompt::Search => {
                self.search = value;
                self.select_first();
                self.status = if self.search.is_empty() {
                    "Search cleared".to_string()
                } else {
                    format!("Filtering by keyword: {}", self.search)
                };
            }
            Prompt::NewFolder(base) => {
                if value.is_empty() {
                    self.status = "Folder name is empty".to_string();
                } else {
                    self.create_folder(base.join(value));
                }
            }
            Prompt::Rename(path) => {
                if value.is_empty() {
                    self.status = "New name is empty".to_string();
                } else {
                    let target = path.parent().unwrap_or_else(|| Path::new(".")).join(value);
                    self.rename_path(path, target);
                }
            }
            Prompt::CopyTo(path) => {
                if value.is_empty() {
                    self.status = "Target path is empty".to_string();
                } else {
                    self.copy_path(path, PathBuf::from(value));
                }
            }
            Prompt::MoveTo(path) => {
                if value.is_empty() {
                    self.status = "Target path is empty".to_string();
                } else {
                    self.rename_path(path, PathBuf::from(value));
                }
            }
        }
    }

    fn activate_modal(&mut self) {
        match self.modal.clone() {
            Some(Modal::ActionMenu { actions }) => {
                let index = self.menu_state.selected().unwrap_or(0);
                if let Some(action) = actions.get(index).copied() {
                    self.modal = None;
                    self.run_action(action);
                }
            }
            Some(Modal::ConfirmDelete(path)) => {
                self.modal = None;
                self.delete_path(path);
            }
            Some(Modal::ConfirmKill(pid)) => {
                self.modal = None;
                self.kill_process(pid);
            }
            Some(Modal::Prompt(_)) | None => {}
        }
    }

    fn run_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::CopyDetails => self.copy_selected_details(),
            MenuAction::CopyPath => {
                if let Some(path) = self.selected_path() {
                    self.copy_to_clipboard(path.display().to_string());
                }
            }
            MenuAction::CopyPort => {
                if let Some(port) = self.selected_network_port() {
                    self.copy_to_clipboard(port);
                }
            }
            MenuAction::CopyPid => {
                if let Some(pid) = self.selected_pid() {
                    self.copy_to_clipboard(pid.to_string());
                }
            }
            MenuAction::Enter => self.enter_selected(),
            MenuAction::NewFolder => {
                let base = self.disk_path.clone().unwrap_or_else(|| PathBuf::from("."));
                self.open_prompt(Prompt::NewFolder(base), "");
            }
            MenuAction::Rename => {
                if let Some(path) = self.selected_path() {
                    let initial = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.open_prompt(Prompt::Rename(path), &initial);
                }
            }
            MenuAction::CopyTo => {
                if let Some(path) = self.selected_path() {
                    self.open_prompt(Prompt::CopyTo(path), "");
                }
            }
            MenuAction::MoveTo => {
                if let Some(path) = self.selected_path() {
                    self.open_prompt(Prompt::MoveTo(path), "");
                }
            }
            MenuAction::Delete => {
                if let Some(path) = self.selected_path() {
                    self.modal = Some(Modal::ConfirmDelete(path));
                }
            }
            MenuAction::KillProcess => {
                if let Some(pid) = self.selected_pid() {
                    self.modal = Some(Modal::ConfirmKill(pid));
                }
            }
        }
    }

    fn refresh(&mut self) {
        self.disks.refresh(true);
        if self.disk_path.is_some() {
            self.refresh_files();
        }
        self.services = read_services();
        self.logs = read_logs();
        self.refresh_live_data();
    }

    fn refresh_live_data(&mut self) {
        self.system.refresh_cpu_all();
        self.system.refresh_memory();
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        self.network = read_network_connections();
        self.processes = read_processes(&self.system);
        self.last_refresh = Instant::now();
        self.status = format!(
            "{} disks | {} network | {} processes | search: {}",
            self.disks.len(),
            self.network.len(),
            self.processes.len(),
            if self.search.is_empty() {
                "off"
            } else {
                self.search.as_str()
            }
        );
        self.clamp_selection();
    }

    fn refresh_files(&mut self) {
        self.files = self
            .disk_path
            .as_ref()
            .map(|path| read_directory(path).unwrap_or_else(|err| self.error_entries(err)))
            .unwrap_or_default();
        self.clamp_selection();
    }

    fn error_entries(&self, err: io::Error) -> Vec<FileEntry> {
        vec![FileEntry {
            name: format!("Unable to read directory: {err}"),
            path: self.disk_path.clone().unwrap_or_default(),
            kind: FileKind::Other,
            size: None,
            modified: None,
        }]
    }

    fn change_view(&mut self, view: View) {
        self.change_view_with_focus(view, Focus::Table);
    }

    fn change_view_with_focus(&mut self, view: View, focus: Focus) {
        self.view = view;
        self.modal = None;
        self.focus = focus;
        self.select_first();
    }

    fn row_count(&self) -> usize {
        match self.view {
            View::Dashboard => 0,
            View::Disk if self.disk_path.is_none() => self.filtered_disk_indices().len(),
            View::Disk => self.filtered_file_indices().len(),
            View::Network => self.filtered_network_indices().len(),
            View::Process => self.filtered_process_indices().len(),
            View::Cpu => self.cpu_rows().len(),
            View::Memory => self.memory_rows().len(),
            View::Services => self.filtered_service_indices().len(),
            View::Logs => self.filtered_log_indices().len(),
            View::System => self.system_rows().len(),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let row_count = self.row_count();
        move_table_selection(&mut self.table_state, row_count, delta);
    }

    fn page_step(&self) -> isize {
        (self.row_count().min(12).max(1)) as isize
    }

    fn move_menu_selection(&mut self, delta: isize) {
        let len = match &self.modal {
            Some(Modal::ActionMenu { actions }) => actions.len(),
            Some(Modal::ConfirmDelete(_)) | Some(Modal::ConfirmKill(_)) => 1,
            Some(Modal::Prompt(_)) | None => 0,
        };
        move_table_selection(&mut self.menu_state, len, delta);
    }

    fn select_first(&mut self) {
        self.table_state
            .select(if self.row_count() == 0 { None } else { Some(0) });
    }

    fn select_last(&mut self) {
        self.table_state.select(self.row_count().checked_sub(1));
    }

    fn clamp_selection(&mut self) {
        let row_count = self.row_count();
        let selected = self.table_state.selected().unwrap_or(0);
        self.table_state.select(if row_count == 0 {
            None
        } else {
            Some(min(selected, row_count - 1))
        });
    }

    fn go_left(&mut self) {
        if self.view != View::Disk {
            self.change_view(self.view.previous());
            return;
        }
        self.go_up_directory();
    }

    fn back_or_quit_hint(&mut self) {
        if self.focus == Focus::Tabs {
            self.focus = Focus::Table;
        } else if self.view == View::Disk && self.disk_path.is_some() {
            self.go_up_directory();
        } else if !self.search.is_empty() {
            self.search.clear();
            self.select_first();
            self.status = "Search cleared".to_string();
        } else {
            self.status = "Press q to quit, / to search, arrows and Enter to explore".to_string();
        }
    }

    fn go_up_directory(&mut self) {
        let Some(path) = self.disk_path.clone() else {
            return;
        };

        let mounted_roots = self
            .disks
            .iter()
            .map(|disk| disk.mount_point().to_path_buf())
            .collect::<Vec<_>>();
        if mounted_roots.iter().any(|root| root == &path) || path.parent().is_none() {
            self.disk_path = None;
        } else {
            self.disk_path = path.parent().map(PathBuf::from);
        }
        self.refresh_files();
        self.select_first();
    }

    fn enter_selected(&mut self) {
        match self.view {
            View::Disk if self.disk_path.is_none() => {
                if let Some(disk) = self.selected_disk_mount() {
                    self.disk_path = Some(disk);
                    self.refresh_files();
                    self.select_first();
                }
            }
            View::Disk => {
                if let Some(entry) = self.selected_file() {
                    if entry.kind == FileKind::Directory {
                        self.disk_path = Some(entry.path.clone());
                        self.refresh_files();
                        self.select_first();
                    } else {
                        self.open_action_menu();
                    }
                }
            }
            _ => self.open_action_menu(),
        }
    }

    fn open_action_menu(&mut self) {
        let actions = self.available_actions();
        if actions.is_empty() {
            return;
        }
        self.menu_state.select(Some(0));
        self.modal = Some(Modal::ActionMenu { actions });
    }

    fn available_actions(&self) -> Vec<MenuAction> {
        match self.view {
            View::Disk if self.disk_path.is_none() => {
                vec![
                    MenuAction::Enter,
                    MenuAction::CopyDetails,
                    MenuAction::CopyPath,
                ]
            }
            View::Disk => {
                let mut actions = vec![
                    MenuAction::NewFolder,
                    MenuAction::CopyDetails,
                    MenuAction::CopyPath,
                    MenuAction::Rename,
                    MenuAction::CopyTo,
                    MenuAction::MoveTo,
                ];
                if self
                    .selected_file()
                    .is_some_and(|entry| entry.kind == FileKind::Directory)
                {
                    actions.insert(0, MenuAction::Enter);
                }
                actions.push(MenuAction::Delete);
                actions
            }
            View::Network if self.selected_network_pid().is_some() => {
                vec![
                    MenuAction::CopyDetails,
                    MenuAction::CopyPort,
                    MenuAction::KillProcess,
                ]
            }
            View::Network => vec![MenuAction::CopyDetails, MenuAction::CopyPort],
            View::Process => vec![MenuAction::CopyDetails, MenuAction::KillProcess],
            View::Services if self.selected_service_pid().is_some() => {
                vec![
                    MenuAction::CopyDetails,
                    MenuAction::CopyPid,
                    MenuAction::KillProcess,
                ]
            }
            _ => vec![MenuAction::CopyDetails],
        }
    }

    fn selected_disk_mount(&self) -> Option<PathBuf> {
        let index = self.table_state.selected()?;
        let disk_index = *self.filtered_disk_indices().get(index)?;
        self.disks
            .iter()
            .nth(disk_index)
            .map(|disk| disk.mount_point().to_path_buf())
    }

    fn selected_file(&self) -> Option<&FileEntry> {
        let index = self.table_state.selected()?;
        let file_index = *self.filtered_file_indices().get(index)?;
        self.files.get(file_index)
    }

    fn selected_path(&self) -> Option<PathBuf> {
        match self.view {
            View::Dashboard => None,
            View::Disk if self.disk_path.is_none() => self.selected_disk_mount(),
            View::Disk => self.selected_file().map(|entry| entry.path.clone()),
            _ => None,
        }
    }

    fn selected_pid(&self) -> Option<Pid> {
        match self.view {
            View::Network => self.selected_network_pid(),
            View::Services => self.selected_service_pid(),
            View::Process => {
                let index = self.table_state.selected()?;
                let process_index = *self.filtered_process_indices().get(index)?;
                self.processes.get(process_index).map(|process| process.pid)
            }
            _ => None,
        }
    }

    fn selected_network_pid(&self) -> Option<Pid> {
        self.selected_network_row().and_then(|row| row.pid)
    }

    fn selected_network_row(&self) -> Option<&NetworkRow> {
        if self.view != View::Network {
            return None;
        }
        let index = self.table_state.selected()?;
        let row_index = *self.filtered_network_indices().get(index)?;
        self.network.get(row_index)
    }

    fn selected_network_port(&self) -> Option<String> {
        let row = self.selected_network_row()?;
        endpoint_port(&row.local)
            .or_else(|| endpoint_port(&row.remote))
            .map(|port| port.to_string())
    }

    fn selected_service_pid(&self) -> Option<Pid> {
        if self.view != View::Services {
            return None;
        }
        let index = self.table_state.selected()?;
        let row_index = *self.filtered_service_indices().get(index)?;
        self.services.get(row_index).and_then(|row| row.pid)
    }

    fn selected_details(&self) -> Option<String> {
        if self.view == View::Dashboard {
            return Some(self.dashboard_summary());
        }
        let index = self.table_state.selected()?;
        match self.view {
            View::Dashboard => Some(self.dashboard_summary()),
            View::Disk if self.disk_path.is_none() => {
                let disk_index = *self.filtered_disk_indices().get(index)?;
                let disk = self.disks.iter().nth(disk_index)?;
                let total = disk.total_space();
                let used = total.saturating_sub(disk.available_space());
                Some(format!(
                    "disk={} mount={} fs={} used={} total={} use={:.1}%",
                    disk.name().to_string_lossy(),
                    disk.mount_point().display(),
                    disk.file_system().to_string_lossy(),
                    bytes(used),
                    bytes(total),
                    percent(used, total)
                ))
            }
            View::Disk => {
                let entry = self.selected_file()?;
                Some(format!(
                    "{} {} {}",
                    entry.kind.label(),
                    entry.path.display(),
                    entry.size.map(bytes).unwrap_or_else(|| "-".to_string())
                ))
            }
            View::Network => {
                let row_index = *self.filtered_network_indices().get(index)?;
                let conn = self.network.get(row_index)?;
                let local_port = endpoint_port(&conn.local).unwrap_or("-");
                let remote_port = endpoint_port(&conn.remote).unwrap_or("-");
                Some(format!(
                    "{} {} -> {} {} {} pid={} local_port={} remote_port={}",
                    conn.proto,
                    conn.local,
                    conn.remote,
                    conn.state,
                    conn.program,
                    conn.pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    local_port,
                    remote_port
                ))
            }
            View::Process => {
                let row_index = *self.filtered_process_indices().get(index)?;
                let process = self.processes.get(row_index)?;
                Some(format!(
                    "pid={} cpu={:.1}% memory={} status={} command={}",
                    process.pid,
                    process.cpu,
                    bytes(process.memory),
                    process.status,
                    process.name
                ))
            }
            View::Cpu => self.cpu_rows().get(index).map(|row| row.value.clone()),
            View::Memory => self
                .memory_rows()
                .get(index)
                .map(|row| format!("{}={}", row.key, row.value)),
            View::Services => {
                let row_index = *self.filtered_service_indices().get(index)?;
                let service = self.services.get(row_index)?;
                Some(format!(
                    "{} {} {} pid={}",
                    service.state,
                    service.name,
                    service.detail,
                    service
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ))
            }
            View::Logs => {
                let row_index = *self.filtered_log_indices().get(index)?;
                self.logs.get(row_index).map(|log| log.line.clone())
            }
            View::System => self
                .system_rows()
                .get(index)
                .map(|row| format!("{}={}", row.key, row.value)),
        }
    }

    fn copy_selected_details(&mut self) {
        if let Some(details) = self.selected_details() {
            self.copy_to_clipboard(details);
        }
    }

    fn copy_to_clipboard(&mut self, text: String) {
        self.clipboard = text.clone();
        match write_clipboard(&text) {
            Ok(()) => self.status = "Copied to system clipboard".to_string(),
            Err(_) => self.status = "Copied to internal clipboard buffer".to_string(),
        }
    }

    fn create_folder(&mut self, path: PathBuf) {
        match fs::create_dir_all(&path) {
            Ok(()) => {
                self.status = format!("Created {}", path.display());
                self.refresh_files();
            }
            Err(err) => self.status = format!("Create folder failed: {err}"),
        }
    }

    fn rename_path(&mut self, path: PathBuf, target: PathBuf) {
        match fs::rename(&path, &target) {
            Ok(()) => {
                self.status = format!("Moved {} -> {}", path.display(), target.display());
                self.refresh_files();
            }
            Err(err) => self.status = format!("Move/rename failed: {err}"),
        }
    }

    fn copy_path(&mut self, path: PathBuf, target: PathBuf) {
        let result = if path.is_dir() {
            copy_dir_all(&path, &target)
        } else {
            fs::copy(&path, &target).map(|_| ())
        };
        match result {
            Ok(()) => {
                self.status = format!("Copied {} -> {}", path.display(), target.display());
                self.refresh_files();
            }
            Err(err) => self.status = format!("Copy failed: {err}"),
        }
    }

    fn delete_path(&mut self, path: PathBuf) {
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };

        match result {
            Ok(()) => {
                self.status = format!("Deleted {}", path.display());
                self.refresh_files();
            }
            Err(err) => self.status = format!("Delete failed: {err}"),
        }
    }

    fn kill_process(&mut self, pid: Pid) {
        let Some(process) = self.system.process(pid) else {
            self.status = format!("Process {pid} no longer exists");
            self.refresh_live_data();
            return;
        };

        if process.kill() {
            self.status = format!("Sent kill signal to {pid}");
        } else {
            self.status = format!("Unable to kill process {pid}");
        }
        self.refresh_live_data();
    }

    fn filtered_disk_indices(&self) -> Vec<usize> {
        self.disks
            .iter()
            .enumerate()
            .filter_map(|(index, disk)| {
                let text = format!(
                    "{} {} {}",
                    disk.name().to_string_lossy(),
                    disk.mount_point().display(),
                    disk.file_system().to_string_lossy()
                );
                matches_search(&text, &self.search).then_some(index)
            })
            .collect()
    }

    fn filtered_file_indices(&self) -> Vec<usize> {
        self.files
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let text = format!(
                    "{} {} {}",
                    entry.name,
                    entry.kind.label(),
                    entry.path.display()
                );
                matches_search(&text, &self.search).then_some(index)
            })
            .collect()
    }

    fn filtered_network_indices(&self) -> Vec<usize> {
        self.network
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let text = format!(
                    "{} {} {} {} {} {}",
                    row.proto,
                    row.local,
                    row.remote,
                    row.state,
                    row.program,
                    row.pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string())
                );
                matches_search(&text, &self.search).then_some(index)
            })
            .collect()
    }

    fn filtered_process_indices(&self) -> Vec<usize> {
        self.processes
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let text = format!(
                    "{} {:.1} {} {} {}",
                    row.pid, row.cpu, row.memory, row.status, row.name
                );
                matches_search(&text, &self.search).then_some(index)
            })
            .collect()
    }

    fn filtered_service_indices(&self) -> Vec<usize> {
        self.services
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let text = format!(
                    "{} {} {} {}",
                    row.name,
                    row.state,
                    row.detail,
                    row.pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string())
                );
                matches_search(&text, &self.search).then_some(index)
            })
            .collect()
    }

    fn filtered_log_indices(&self) -> Vec<usize> {
        self.logs
            .iter()
            .enumerate()
            .filter_map(|(index, row)| matches_search(&row.line, &self.search).then_some(index))
            .collect()
    }

    fn cpu_rows(&self) -> Vec<InfoRow> {
        let mut rows = vec![InfoRow {
            key: "Global CPU".to_string(),
            value: format!("{:.1}%", self.system.global_cpu_usage()),
        }];
        for (index, cpu) in self.system.cpus().iter().enumerate() {
            rows.push(InfoRow {
                key: format!("CPU {index}"),
                value: format!("{:.1}% {}", cpu.cpu_usage(), cpu.brand()),
            });
        }
        rows.into_iter()
            .filter(|row| matches_search(&format!("{} {}", row.key, row.value), &self.search))
            .collect()
    }

    fn memory_rows(&self) -> Vec<InfoRow> {
        let rows = vec![
            InfoRow {
                key: "Memory used".to_string(),
                value: format!(
                    "{} / {} ({:.1}%)",
                    bytes(self.system.used_memory()),
                    bytes(self.system.total_memory()),
                    percent(self.system.used_memory(), self.system.total_memory())
                ),
            },
            InfoRow {
                key: "Memory available".to_string(),
                value: bytes(self.system.available_memory()),
            },
            InfoRow {
                key: "Swap used".to_string(),
                value: format!(
                    "{} / {} ({:.1}%)",
                    bytes(self.system.used_swap()),
                    bytes(self.system.total_swap()),
                    percent(self.system.used_swap(), self.system.total_swap())
                ),
            },
        ];
        rows.into_iter()
            .filter(|row| matches_search(&format!("{} {}", row.key, row.value), &self.search))
            .collect()
    }

    fn system_rows(&self) -> Vec<InfoRow> {
        let rows = vec![
            InfoRow {
                key: "Name".to_string(),
                value: System::name().unwrap_or_else(|| "-".to_string()),
            },
            InfoRow {
                key: "Kernel".to_string(),
                value: System::kernel_version().unwrap_or_else(|| "-".to_string()),
            },
            InfoRow {
                key: "OS version".to_string(),
                value: System::os_version().unwrap_or_else(|| "-".to_string()),
            },
            InfoRow {
                key: "Host".to_string(),
                value: System::host_name().unwrap_or_else(|| "-".to_string()),
            },
            InfoRow {
                key: "Uptime".to_string(),
                value: format_duration(Duration::from_secs(System::uptime())),
            },
            InfoRow {
                key: "Boot time".to_string(),
                value: System::boot_time().to_string(),
            },
        ];
        rows.into_iter()
            .filter(|row| matches_search(&format!("{} {}", row.key, row.value), &self.search))
            .collect()
    }

    fn dashboard_summary(&self) -> String {
        format!(
            "cpu={:.1}% memory={} / {} network={} processes={} services={} logs={} disks={}",
            self.system.global_cpu_usage(),
            bytes(self.system.used_memory()),
            bytes(self.system.total_memory()),
            self.network.len(),
            self.processes.len(),
            self.services.len(),
            self.logs.len(),
            self.disks.len()
        )
    }

    fn render(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(5),
            ])
            .split(frame.area());

        frame.render_widget(Clear, frame.area());
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(Color::Black)),
            frame.area(),
        );
        self.render_tabs(frame, chunks[0]);
        frame.render_widget(Clear, chunks[1]);
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(Color::Black)),
            chunks[1],
        );
        match self.view {
            View::Dashboard => self.render_dashboard(frame, chunks[1]),
            View::Disk => self.render_disks(frame, chunks[1]),
            View::Network => self.render_network(frame, chunks[1]),
            View::Process => self.render_processes(frame, chunks[1]),
            View::Cpu => {
                self.render_info_table(frame, chunks[1], "CPU", self.cpu_rows(), Color::LightRed)
            }
            View::Memory => self.render_info_table(
                frame,
                chunks[1],
                "Memory",
                self.memory_rows(),
                Color::LightYellow,
            ),
            View::Services => self.render_services(frame, chunks[1]),
            View::Logs => self.render_logs(frame, chunks[1]),
            View::System => self.render_info_table(
                frame,
                chunks[1],
                "System",
                self.system_rows(),
                Color::LightGreen,
            ),
        }
        self.render_help(frame, chunks[2]);
        self.render_modal(frame);
    }

    fn render_tabs(&self, frame: &mut Frame, area: Rect) {
        let titles = VIEWS.map(|view| Line::from(view.label()));
        let border = if self.focus == Focus::Tabs {
            Color::LightYellow
        } else {
            Color::Magenta
        };
        let tabs = Tabs::new(titles)
            .select(self.view.index())
            .block(
                Block::default()
                    .title("OST Explorer")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border)),
            )
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(if self.focus == Focus::Tabs {
                        Color::LightYellow
                    } else {
                        Color::LightCyan
                    })
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(tabs, area);
    }

    fn render_dashboard(&self, frame: &mut Frame, area: Rect) {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),
                Constraint::Length(11),
                Constraint::Min(8),
            ])
            .split(area);

        let gauges = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
            ])
            .split(vertical[0]);

        let cpu = self.system.global_cpu_usage().clamp(0.0, 100.0);
        let mem =
            percent(self.system.used_memory(), self.system.total_memory()).clamp(0.0, 100.0) as f32;
        let swap =
            percent(self.system.used_swap(), self.system.total_swap()).clamp(0.0, 100.0) as f32;

        frame.render_widget(
            dashboard_gauge("CPU", cpu, Color::LightCyan, format!("{cpu:.1}%")),
            gauges[0],
        );
        frame.render_widget(
            dashboard_gauge("Memory", mem, Color::LightGreen, format!("{mem:.1}%")),
            gauges[1],
        );
        frame.render_widget(
            dashboard_gauge("Swap", swap, Color::LightBlue, format!("{swap:.1}%")),
            gauges[2],
        );

        let middle = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(vertical[1]);

        let disk_data = self
            .disks
            .iter()
            .take(8)
            .map(|disk| {
                let total = disk.total_space();
                let used = total.saturating_sub(disk.available_space());
                (
                    short_label(&disk.name().to_string_lossy(), 9),
                    percent(used, total).round() as u64,
                )
            })
            .collect::<Vec<_>>();
        render_gauge_list(frame, middle[0], "Disk usage", &disk_data, Color::LightBlue);

        let stats = vec![
            Line::from(vec![
                Span::styled("Network rows  ", Color::LightCyan),
                Span::raw(self.network.len().to_string()),
            ]),
            Line::from(vec![
                Span::styled("Processes     ", Color::Cyan),
                Span::raw(self.processes.len().to_string()),
            ]),
            Line::from(vec![
                Span::styled("Services      ", Color::LightGreen),
                Span::raw(self.services.len().to_string()),
            ]),
            Line::from(vec![
                Span::styled("Recent logs   ", Color::LightBlue),
                Span::raw(self.logs.len().to_string()),
            ]),
            Line::from(vec![
                Span::styled("Uptime        ", Color::Green),
                Span::raw(format_duration(Duration::from_secs(System::uptime()))),
            ]),
            Line::from(vec![
                Span::styled("Memory        ", Color::LightGreen),
                Span::raw(format!(
                    "{} / {}",
                    bytes(self.system.used_memory()),
                    bytes(self.system.total_memory())
                )),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(stats)
                .block(
                    themed_block("System at a glance")
                        .border_style(Style::default().fg(Color::LightGreen)),
                )
                .wrap(Wrap { trim: true }),
            middle[1],
        );

        let bottom = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(vertical[2]);

        let cpu_data = self
            .system
            .cpus()
            .iter()
            .enumerate()
            .take(16)
            .map(|(index, cpu)| {
                (
                    index.to_string(),
                    cpu.cpu_usage().clamp(0.0, 100.0).round() as u64,
                )
            })
            .collect::<Vec<_>>();
        render_gauge_list(
            frame,
            bottom[0],
            "Per-core CPU",
            &cpu_data,
            Color::LightCyan,
        );

        let top_processes = self
            .processes
            .iter()
            .take(5)
            .map(|process| {
                Line::from(vec![
                    Span::styled(format!("{:>5.1}% ", process.cpu), Color::LightCyan),
                    Span::raw(short_label(&process.name, 34)),
                ])
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(top_processes)
                .block(
                    themed_block("Top CPU processes")
                        .border_style(Style::default().fg(Color::Cyan)),
                )
                .wrap(Wrap { trim: true }),
            bottom[1],
        );
    }

    fn render_disks(&mut self, frame: &mut Frame, area: Rect) {
        if self.disk_path.is_some() {
            self.render_files(frame, area);
            return;
        }

        let disk_indices = self.filtered_disk_indices();
        let rows = disk_indices.into_iter().filter_map(|index| {
            let disk = self.disks.iter().nth(index)?;
            let total = disk.total_space();
            let available = disk.available_space();
            let used = total.saturating_sub(available);
            Some(Row::new([
                Cell::from(Line::from(vec![
                    Span::styled("● ", Style::default().fg(Color::LightGreen)),
                    Span::raw(disk.name().to_string_lossy().to_string()),
                ])),
                Cell::from(disk.mount_point().display().to_string()),
                Cell::from(disk.file_system().to_string_lossy().to_string()),
                Cell::from(bytes(used)),
                Cell::from(bytes(total)),
                Cell::from(format!("{:.1}%", percent(used, total))),
            ]))
        });

        let table = Table::new(
            rows,
            [
                Constraint::Length(20),
                Constraint::Percentage(34),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(8),
            ],
        )
        .header(header(["Volume", "Mount", "FS", "Used", "Total", "Use%"]))
        .block(
            themed_block("Disk volumes")
                .title_bottom("Enter: open  Right: actions  /: search")
                .border_style(Style::default().fg(Color::LightGreen)),
        )
        .row_highlight_style(highlight_style())
        .highlight_symbol("▸ ");

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_files(&mut self, frame: &mut Frame, area: Rect) {
        let title = self
            .disk_path
            .as_ref()
            .map(|path| format!("Files: {}", path.display()))
            .unwrap_or_else(|| "Files".to_string());
        let file_indices = self.filtered_file_indices();
        let rows = file_indices.into_iter().filter_map(|index| {
            let entry = self.files.get(index)?;
            let icon = match entry.kind {
                FileKind::Directory => Span::styled("▣ ", Color::LightBlue),
                FileKind::File => Span::styled("• ", Color::White),
                FileKind::Symlink => Span::styled("↪ ", Color::LightMagenta),
                FileKind::Other => Span::styled("? ", Color::LightRed),
            };
            Some(Row::new([
                Cell::from(Line::from(vec![icon, Span::raw(entry.name.clone())])),
                Cell::from(entry.kind.label()),
                Cell::from(entry.size.map(bytes).unwrap_or_else(|| "-".to_string())),
                Cell::from(
                    entry
                        .modified
                        .and_then(|time| time.elapsed().ok())
                        .map(age)
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(entry.path.display().to_string()),
            ]))
        });

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(30),
                Constraint::Length(8),
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Percentage(40),
            ],
        )
        .header(header(["Name", "Kind", "Size", "Modified", "Path"]))
        .block(
            themed_block(title)
                .title_bottom("Enter: open/menu  Left/Esc: back  Right: actions  /: search")
                .border_style(Style::default().fg(Color::LightBlue)),
        )
        .row_highlight_style(highlight_style())
        .highlight_symbol("▸ ");

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_network(&mut self, frame: &mut Frame, area: Rect) {
        let row_indices = self.filtered_network_indices();
        let rows = row_indices.into_iter().filter_map(|index| {
            let conn = self.network.get(index)?;
            let proto_color = if conn.proto.starts_with("tcp") {
                Color::LightCyan
            } else {
                Color::LightYellow
            };
            let state_color = match conn.state.as_str() {
                "LISTEN" => Color::LightGreen,
                "ESTABLISHED" => Color::LightCyan,
                "CLOSE_WAIT" | "TIME_WAIT" => Color::Yellow,
                "-" => Color::Gray,
                _ => Color::LightRed,
            };
            Some(Row::new([
                Cell::from(Span::styled(conn.proto.clone(), proto_color)),
                Cell::from(conn.local.clone()),
                Cell::from(conn.remote.clone()),
                Cell::from(Span::styled(conn.state.clone(), state_color)),
                Cell::from(
                    conn.pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(conn.program.clone()),
            ]))
        });

        let table = Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Percentage(28),
                Constraint::Percentage(28),
                Constraint::Length(14),
                Constraint::Length(8),
                Constraint::Percentage(18),
            ],
        )
        .header(header([
            "Proto", "Local", "Remote", "State", "PID", "Program",
        ]))
        .block(
            themed_block("Network connections")
                .title_bottom("Enter/Right: actions  /: search")
                .border_style(Style::default().fg(Color::LightCyan)),
        )
        .row_highlight_style(highlight_style())
        .highlight_symbol("▸ ");

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_processes(&mut self, frame: &mut Frame, area: Rect) {
        let row_indices = self.filtered_process_indices();
        let rows = row_indices.into_iter().filter_map(|index| {
            let process = self.processes.get(index)?;
            let cpu_color = if process.cpu >= 20.0 {
                Color::LightRed
            } else if process.cpu >= 5.0 {
                Color::Yellow
            } else {
                Color::LightGreen
            };
            Some(Row::new([
                Cell::from(process.pid.to_string()),
                Cell::from(Span::styled(format!("{:.1}", process.cpu), cpu_color)),
                Cell::from(bytes(process.memory)),
                Cell::from(process.status.to_string()),
                Cell::from(process.name.clone()),
            ]))
        });

        let table = Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Percentage(55),
            ],
        )
        .header(header(["PID", "CPU%", "Memory", "Status", "Command"]))
        .block(
            themed_block("Processes by CPU")
                .title_bottom("Enter/Right: actions  /: search")
                .border_style(Style::default().fg(Color::LightMagenta)),
        )
        .row_highlight_style(highlight_style())
        .highlight_symbol("▸ ");

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_info_table(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        rows: Vec<InfoRow>,
        color: Color,
    ) {
        let rows = rows.into_iter().map(|row| {
            Row::new([
                Cell::from(Span::styled(row.key, Color::LightYellow)),
                Cell::from(row.value),
            ])
        });
        let table = Table::new(rows, [Constraint::Length(24), Constraint::Percentage(76)])
            .header(header(["Metric", "Value"]))
            .block(
                themed_block(title)
                    .title_bottom("Enter/Right: copy details  /: search")
                    .border_style(Style::default().fg(color)),
            )
            .row_highlight_style(highlight_style())
            .highlight_symbol("▸ ");
        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_services(&mut self, frame: &mut Frame, area: Rect) {
        let row_indices = self.filtered_service_indices();
        let rows = row_indices.into_iter().filter_map(|index| {
            let service = self.services.get(index)?;
            let color = if service.state.contains("running") || service.state.contains("0") {
                Color::LightGreen
            } else {
                Color::Yellow
            };
            Some(Row::new([
                Cell::from(service.name.clone()),
                Cell::from(Span::styled(service.state.clone(), color)),
                Cell::from(
                    service
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(service.detail.clone()),
            ]))
        });
        let table = Table::new(
            rows,
            [
                Constraint::Percentage(35),
                Constraint::Length(16),
                Constraint::Length(8),
                Constraint::Percentage(37),
            ],
        )
        .header(header(["Service", "State", "PID", "Detail"]))
        .block(
            themed_block("Services")
                .title_bottom("Enter/Right: copy details  /: search")
                .border_style(Style::default().fg(Color::LightGreen)),
        )
        .row_highlight_style(highlight_style())
        .highlight_symbol("▸ ");
        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_logs(&mut self, frame: &mut Frame, area: Rect) {
        let row_indices = self.filtered_log_indices();
        let rows = row_indices.into_iter().filter_map(|index| {
            let log = self.logs.get(index)?;
            Some(Row::new([Cell::from(log.line.clone())]))
        });
        let table = Table::new(rows, [Constraint::Percentage(100)])
            .header(header(["Recent logs"]))
            .block(
                themed_block("Logs")
                    .title_bottom("Enter/Right: copy details  /: search")
                    .border_style(Style::default().fg(Color::LightRed)),
            )
            .row_highlight_style(highlight_style())
            .highlight_symbol("▸ ");
        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let focus = match self.focus {
            Focus::Tabs => "tabs",
            Focus::Table => "table",
        };
        let help = vec![
            Line::from(vec![
                Span::styled("↑↓←→", Style::default().fg(Color::LightGreen).bold()),
                Span::raw(" navigate   "),
                Span::styled("Enter", Style::default().fg(Color::LightCyan).bold()),
                Span::raw(" open/action   "),
                Span::styled("/", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" search   "),
                Span::styled("Esc", Style::default().fg(Color::LightMagenta).bold()),
                Span::raw(" back/clear   "),
                Span::styled("q", Style::default().fg(Color::LightRed).bold()),
                Span::raw(" quit"),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("Focus: {focus}  "),
                    Style::default().fg(Color::LightCyan),
                ),
                Span::styled("Search: ", Style::default().fg(Color::Yellow)),
                Span::raw(if self.search.is_empty() {
                    "<none>"
                } else {
                    self.search.as_str()
                }),
                Span::raw("  "),
                Span::styled("Status: ", Style::default().fg(Color::Yellow)),
                Span::raw(self.status.as_str()),
            ]),
        ];

        let paragraph = Paragraph::new(help)
            .block(
                Block::default()
                    .title("Controls")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }

    fn render_modal(&mut self, frame: &mut Frame) {
        let Some(modal) = self.modal.clone() else {
            return;
        };
        let area = centered_rect(62, 34, frame.area());
        frame.render_widget(Clear, area);

        match modal {
            Modal::ActionMenu { actions } => {
                let rows = actions.iter().map(|action| Row::new([action.label()]));
                let table = Table::new(rows, [Constraint::Percentage(100)])
                    .block(
                        themed_block("Actions")
                            .title_bottom("↑↓ choose  Enter: run  Esc/←: close")
                            .border_style(Style::default().fg(Color::LightMagenta)),
                    )
                    .row_highlight_style(highlight_style())
                    .highlight_symbol("▸ ");
                frame.render_stateful_widget(table, area, &mut self.menu_state);
            }
            Modal::ConfirmDelete(path) => {
                let text = vec![
                    Line::from(Span::styled("Delete this item?", Color::LightRed).bold()),
                    Line::from(""),
                    Line::from(path.display().to_string()),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Enter", Color::LightRed).bold(),
                        Span::raw(" delete permanently    "),
                        Span::styled("Esc/←", Color::LightGreen).bold(),
                        Span::raw(" cancel"),
                    ]),
                ];
                frame.render_widget(confirm_block(text, "Confirm delete"), area);
            }
            Modal::ConfirmKill(pid) => {
                let text = vec![
                    Line::from(Span::styled("Kill this process?", Color::LightRed).bold()),
                    Line::from(""),
                    Line::from(format!("PID {pid}")),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Enter", Color::LightRed).bold(),
                        Span::raw(" kill    "),
                        Span::styled("Esc/←", Color::LightGreen).bold(),
                        Span::raw(" cancel"),
                    ]),
                ];
                frame.render_widget(confirm_block(text, "Confirm kill"), area);
            }
            Modal::Prompt(prompt) => {
                let input = if self.prompt_text.is_empty() {
                    " ".to_string()
                } else {
                    self.prompt_text.clone()
                };
                let text = vec![
                    Line::from(prompt.title()),
                    Line::from(""),
                    Line::from(Span::styled(
                        input,
                        Style::default().fg(Color::Black).bg(Color::LightYellow),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Enter", Color::LightGreen).bold(),
                        Span::raw(" apply    "),
                        Span::styled("Esc", Color::LightRed).bold(),
                        Span::raw(" cancel"),
                    ]),
                ];
                frame.render_widget(confirm_block(text, prompt.title()), area);
            }
        }
    }
}

fn move_table_selection(state: &mut TableState, row_count: usize, delta: isize) {
    if row_count == 0 {
        state.select(None);
        return;
    }

    let selected = state.selected().unwrap_or(0) as isize;
    let next = (selected + delta).clamp(0, row_count as isize - 1) as usize;
    state.select(Some(next));
}

fn read_directory(path: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_dir() {
            FileKind::Directory
        } else if file_type.is_file() {
            FileKind::File
        } else if file_type.is_symlink() {
            FileKind::Symlink
        } else {
            FileKind::Other
        };
        entries.push(FileEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            path,
            kind,
            size: (kind == FileKind::File).then_some(metadata.len()),
            modified: metadata.modified().ok(),
        });
    }

    entries.sort_by(|a, b| {
        (a.kind != FileKind::Directory, a.name.to_lowercase())
            .cmp(&(b.kind != FileKind::Directory, b.name.to_lowercase()))
    });
    Ok(entries)
}

fn read_processes(system: &System) -> Vec<ProcessRow> {
    let mut processes = system
        .processes()
        .values()
        .map(|process| ProcessRow {
            pid: process.pid(),
            cpu: process.cpu_usage(),
            memory: process.memory(),
            status: process.status(),
            name: process.name().to_string_lossy().to_string(),
        })
        .collect::<Vec<_>>();
    processes.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    processes
}

fn read_network_connections() -> Vec<NetworkRow> {
    let lsof_rows = read_lsof_network_connections();
    if !lsof_rows.is_empty() {
        return lsof_rows;
    }

    let output = Command::new("netstat").args(["-anv"]).output();
    let Ok(output) = output else {
        return Vec::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_netstat_line)
        .take(300)
        .collect()
}

fn read_lsof_network_connections() -> Vec<NetworkRow> {
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", "-iUDP"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(parse_lsof_line)
        .take(500)
        .collect()
}

fn parse_lsof_line(line: &str) -> Option<NetworkRow> {
    let columns = line.split_whitespace().collect::<Vec<_>>();
    let command = *columns.first()?;
    let pid = columns.get(1)?.parse::<u32>().ok().map(Pid::from_u32);
    let proto_index = columns
        .iter()
        .position(|column| *column == "TCP" || *column == "UDP")?;
    let proto = columns.get(proto_index)?;
    let name = columns.get(proto_index + 1..)?.join(" ");
    let (endpoint, state) = split_lsof_state(&name);
    let (local, remote) = endpoint
        .split_once("->")
        .map(|(local, remote)| (local.to_string(), remote.to_string()))
        .unwrap_or_else(|| (endpoint.to_string(), "*".to_string()));

    Some(NetworkRow {
        proto: (*proto).to_string(),
        local,
        remote,
        state,
        program: clean_text(command),
        pid,
    })
}

fn split_lsof_state(name: &str) -> (String, String) {
    let Some(start) = name.rfind(" (") else {
        return (clean_text(name), "-".to_string());
    };
    let Some(end) = name[start..].find(')') else {
        return (clean_text(name), "-".to_string());
    };
    let state = &name[start + 2..start + end];
    (clean_text(name[..start].trim()), clean_text(state))
}

fn endpoint_port(endpoint: &str) -> Option<&str> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() || endpoint == "*" || endpoint == "-" {
        return None;
    }

    let port = endpoint.rsplit_once(':')?.1;
    let port = port
        .trim()
        .trim_end_matches(')')
        .trim_end_matches(',')
        .trim_end_matches(']');
    if port.is_empty() || port == "*" {
        None
    } else {
        Some(port)
    }
}

fn parse_netstat_line(line: &str) -> Option<NetworkRow> {
    let columns = line.split_whitespace().collect::<Vec<_>>();
    let proto = *columns.first()?;
    if !proto.starts_with("tcp") && !proto.starts_with("udp") {
        return None;
    }

    let local = columns.get(3)?;
    let remote = columns.get(4).copied().unwrap_or("*");
    let state = columns
        .iter()
        .copied()
        .find(|value| {
            matches!(
                *value,
                "LISTEN"
                    | "ESTABLISHED"
                    | "CLOSE_WAIT"
                    | "TIME_WAIT"
                    | "SYN_SENT"
                    | "SYN_RECEIVED"
                    | "CLOSED"
                    | "LAST_ACK"
                    | "FIN_WAIT_1"
                    | "FIN_WAIT_2"
            )
        })
        .unwrap_or("-");
    let program = columns.last().copied().unwrap_or("-");
    let pid = columns
        .iter()
        .position(|column| *column == state)
        .and_then(|index| extract_pid(&columns[index + 1..]));

    Some(NetworkRow {
        proto: proto.to_string(),
        local: (*local).to_string(),
        remote: remote.to_string(),
        state: state.to_string(),
        program: program.to_string(),
        pid,
    })
}

fn extract_pid(columns: &[&str]) -> Option<Pid> {
    columns
        .iter()
        .rev()
        .find_map(|column| extract_u32(column).map(Pid::from_u32))
}

fn extract_u32(text: &str) -> Option<u32> {
    if let Ok(pid) = text.parse::<u32>() {
        return (pid > 0).then_some(pid);
    }

    text.split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .find(|pid| *pid > 0)
}

fn read_services() -> Vec<ServiceRow> {
    let output = if cfg!(target_os = "macos") {
        Command::new("launchctl").arg("list").output()
    } else {
        Command::new("systemctl")
            .args(["list-units", "--type=service", "--no-pager", "--plain"])
            .output()
    };

    let Ok(output) = output else {
        return vec![ServiceRow {
            name: "service command unavailable".to_string(),
            state: "-".to_string(),
            detail: "-".to_string(),
            pid: None,
        }];
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .take(300)
        .filter_map(|line| {
            let columns = line.split_whitespace().collect::<Vec<_>>();
            if columns.is_empty() {
                return None;
            }
            if cfg!(target_os = "macos") {
                let pid = columns
                    .first()
                    .and_then(|value| value.parse::<u32>().ok())
                    .map(Pid::from_u32);
                Some(ServiceRow {
                    state: clean_text(columns.first().unwrap_or(&"-")),
                    name: clean_text(columns.get(2).unwrap_or(columns.first().unwrap_or(&"-"))),
                    detail: clean_text(line),
                    pid,
                })
            } else {
                Some(ServiceRow {
                    name: clean_text(columns.first()?),
                    state: clean_text(columns.get(3).unwrap_or(&"-")),
                    detail: clean_text(line),
                    pid: extract_pid(&columns),
                })
            }
        })
        .collect()
}

fn read_logs() -> Vec<LogRow> {
    let output = if cfg!(target_os = "macos") {
        Command::new("log")
            .args(["show", "--last", "5m", "--style", "compact"])
            .output()
    } else {
        Command::new("journalctl")
            .args(["-n", "200", "--no-pager"])
            .output()
    };

    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .rev()
            .take(200)
            .map(|line| LogRow {
                line: clean_text(line),
            })
            .collect(),
        _ => vec![LogRow {
            line: "No log output available without additional permissions".to_string(),
        }],
    }
}

fn copy_dir_all(source: &Path, target: &Path) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target_path = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target_path)?;
        } else {
            fs::copy(entry.path(), target_path)?;
        }
    }
    Ok(())
}

fn write_clipboard(text: &str) -> io::Result<()> {
    let command = if cfg!(target_os = "macos") {
        "pbcopy"
    } else if Command::new("wl-copy").arg("--version").output().is_ok() {
        "wl-copy"
    } else {
        "xclip"
    };
    let mut child = Command::new(command).stdin(Stdio::piped()).spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait()?;
    Ok(())
}

fn matches_search(text: &str, search: &str) -> bool {
    if search.trim().is_empty() {
        return true;
    }
    let haystack = text.to_lowercase();
    search
        .split_whitespace()
        .all(|keyword| haystack.contains(&keyword.to_lowercase()))
}

fn clean_text(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch == '\t' {
                ' '
            } else if ch.is_control() {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
}

fn header<const N: usize>(labels: [&'static str; N]) -> Row<'static> {
    Row::new(labels.map(Cell::from)).style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

fn themed_block<T: Into<String>>(title: T) -> Block<'static> {
    Block::default()
        .title(title.into())
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White))
}

fn highlight_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::LightCyan)
        .add_modifier(Modifier::BOLD)
}

fn dashboard_gauge<T: Into<String>>(
    title: T,
    percent_value: f32,
    color: Color,
    label: String,
) -> Gauge<'static> {
    Gauge::default()
        .block(themed_block(title).border_style(Style::default().fg(color)))
        .gauge_style(
            Style::default()
                .fg(color)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .percent(percent_value.round().clamp(0.0, 100.0) as u16)
        .label(label)
}

fn render_gauge_list(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    items: &[(String, u64)],
    color: Color,
) {
    let block = themed_block(title)
        .title_bottom("horizontal bars, value centered")
        .border_style(Style::default().fg(color));
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    frame.render_widget(block, area);

    let visible = items.len().min(inner.height as usize);
    if visible == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(1); visible])
        .split(inner);

    for ((label, value), row) in items.iter().take(visible).zip(rows.iter()) {
        let percent_value = (*value).min(100) as u16;
        let gauge = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(color)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .label(format!("{}  {}%", short_label(label, 10), percent_value))
            .percent(percent_value);
        frame.render_widget(gauge, *row);
    }
}

fn short_label(text: &str, max_chars: usize) -> String {
    let mut label = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars && max_chars > 1 {
        label.pop();
        label.push('…');
    }
    label
}

fn confirm_block(lines: Vec<Line<'static>>, title: &'static str) -> Paragraph<'static> {
    Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightRed)),
        )
        .wrap(Wrap { trim: true })
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = value as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64 * 100.0
    }
}

fn age(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let minutes = (seconds % 3600) / 60;
    format!("{days}d {hours}h {minutes}m")
}
