use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const MAX_LOG_LINES: usize = 4000;
const RESOURCE_SAMPLE_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub services: Vec<ServiceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub build: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Idle,
    Building,
    Running,
    Stopped,
    Exited(i32),
    Failed,
}

impl ServiceStatus {
    pub fn label(&self) -> String {
        match self {
            ServiceStatus::Idle => "idle".into(),
            ServiceStatus::Building => "building".into(),
            ServiceStatus::Running => "running".into(),
            ServiceStatus::Stopped => "stopped".into(),
            ServiceStatus::Exited(code) => format!("exit {}", code),
            ServiceStatus::Failed => "failed".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Stdout,
    Stderr,
    System,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub service: String,
    pub text: String,
    pub kind: LogKind,
}

pub struct ServiceProcess {
    pub config: ServiceConfig,
    pub status: ServiceStatus,
    pub child: Option<Child>,
    pub pid: Option<u32>,
    pub started_at: Option<Instant>,
    pub cpu_pct: f32,
    pub ram_bytes: u64,
    pub disk_read: u64,
    pub disk_write: u64,
}

impl ServiceProcess {
    fn new(config: ServiceConfig) -> Self {
        Self {
            config,
            status: ServiceStatus::Idle,
            child: None,
            pid: None,
            started_at: None,
            cpu_pct: 0.0,
            ram_bytes: 0,
            disk_read: 0,
            disk_write: 0,
        }
    }

    fn clear_resources(&mut self) {
        self.cpu_pct = 0.0;
        self.ram_bytes = 0;
        self.disk_read = 0;
        self.disk_write = 0;
    }
}

pub struct Runtime {
    pub project_root: PathBuf,
    pub config: RuntimeConfig,
    pub services: Vec<ServiceProcess>,
    pub log: Arc<Mutex<VecDeque<LogLine>>>,
    pub selected: usize,
    pub filter: Option<String>,
    pub last_load_error: Option<String>,
    pub last_loaded_at: Option<Instant>,
    sys: sysinfo::System,
    last_sample_at: Option<Instant>,
}

impl Runtime {
    pub fn new(project_root: PathBuf, initial_config: RuntimeConfig) -> Self {
        let mut rt = Self {
            project_root,
            config: RuntimeConfig::default(),
            services: Vec::new(),
            log: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES))),
            selected: 0,
            filter: None,
            last_load_error: None,
            last_loaded_at: None,
            sys: sysinfo::System::new(),
            last_sample_at: None,
        };
        rt.apply_config_external(initial_config);
        rt
    }

    pub fn apply_config_external(&mut self, config: RuntimeConfig) {
        match validate_config(&config) {
            Ok(()) => {
                self.last_load_error = None;
                self.last_loaded_at = Some(Instant::now());
                self.apply_config(config);
            }
            Err(e) => {
                self.last_load_error = Some(format!("{:#}", e));
                self.last_loaded_at = Some(Instant::now());
            }
        }
    }

    fn apply_config(&mut self, new_config: RuntimeConfig) {
        let mut new_services: Vec<ServiceProcess> = Vec::with_capacity(new_config.services.len());
        for cfg in &new_config.services {
            let existing = self
                .services
                .iter()
                .position(|s| s.config.name == cfg.name);
            if let Some(idx) = existing {
                let mut moved = self.services.remove(idx);
                moved.config = cfg.clone();
                new_services.push(moved);
            } else {
                new_services.push(ServiceProcess::new(cfg.clone()));
            }
        }
        for stale in self.services.drain(..) {
            if let Some(child) = stale.child {
                let _ = drop_child(child);
            }
        }
        self.services = new_services;
        self.config = new_config;
        if self.selected >= self.services.len() {
            self.selected = self.services.len().saturating_sub(1);
        }
    }

    pub fn config_exists(&self) -> bool {
        !self.config.services.is_empty()
    }

    pub fn move_selection(&mut self, delta: i32) {
        let len = self.services.len();
        if len == 0 {
            return;
        }
        let cur = self.selected as i32;
        let next = (cur + delta).rem_euclid(len as i32);
        self.selected = next as usize;
    }

    pub fn select_index(&mut self, idx: usize) {
        if idx < self.services.len() {
            self.selected = idx;
        }
    }

    pub fn selected_service_name(&self) -> Option<String> {
        self.services
            .get(self.selected)
            .map(|s| s.config.name.clone())
    }

    pub fn toggle_filter_selected(&mut self) {
        let Some(name) = self.selected_service_name() else { return };
        match &self.filter {
            Some(current) if *current == name => self.filter = None,
            _ => self.filter = Some(name),
        }
    }

    pub fn clear_filter(&mut self) {
        self.filter = None;
    }

    pub fn clear_log(&mut self) {
        if let Ok(mut log) = self.log.lock() {
            log.clear();
        }
    }

    pub fn run(&mut self, target: Option<&str>) {
        let order = match self.resolve_targets(target) {
            Ok(o) => o,
            Err(e) => {
                self.system_log(target.unwrap_or("all"), &e);
                return;
            }
        };
        for name in order {
            self.run_one(&name);
        }
    }

    pub fn stop(&mut self, target: Option<&str>) {
        let names = self.affected_services(target);
        for name in names.iter().rev() {
            self.stop_one(name);
        }
    }

    pub fn reload(&mut self, target: Option<&str>) {
        let names = self.affected_services(target);
        for name in &names {
            self.stop_one(name);
        }
        for name in &names {
            self.run_one(name);
        }
    }

    pub fn build(&mut self, target: Option<&str>) {
        let order = match self.resolve_targets(target) {
            Ok(o) => o,
            Err(e) => {
                self.system_log(target.unwrap_or("all"), &e);
                return;
            }
        };
        for name in order {
            self.build_one(&name);
        }
    }

    fn run_one(&mut self, name: &str) {
        let Some(idx) = self.services.iter().position(|s| s.config.name == name) else {
            self.system_log(name, &format!("Unknown service: {}", name));
            return;
        };
        if matches!(self.services[idx].status, ServiceStatus::Running) {
            self.system_log(name, "Already running");
            return;
        }
        let cfg = self.services[idx].config.clone();
        let cwd = self.resolve_cwd(&cfg);
        let log = self.log.clone();
        let argv = match parse_command(&cfg.command) {
            Some(a) if !a.is_empty() => a,
            _ => {
                self.services[idx].status = ServiceStatus::Failed;
                self.system_log(name, "Empty or invalid `command`");
                return;
            }
        };
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &cfg.env {
            cmd.env(k, v);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.services[idx].status = ServiceStatus::Failed;
                self.system_log(name, &format!("spawn failed: {}", e));
                return;
            }
        };
        let pid = child.id();
        if let Some(out) = child.stdout.take() {
            spawn_reader(name.to_string(), out, LogKind::Stdout, log.clone());
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(name.to_string(), err, LogKind::Stderr, log.clone());
        }
        self.services[idx].child = Some(child);
        self.services[idx].pid = Some(pid);
        self.services[idx].started_at = Some(Instant::now());
        self.services[idx].status = ServiceStatus::Running;
        self.services[idx].clear_resources();
        self.system_log(name, &format!("started (pid {})", pid));
    }

    fn stop_one(&mut self, name: &str) {
        let Some(idx) = self.services.iter().position(|s| s.config.name == name) else {
            return;
        };
        let Some(mut child) = self.services[idx].child.take() else {
            if matches!(self.services[idx].status, ServiceStatus::Running) {
                self.services[idx].status = ServiceStatus::Stopped;
            }
            return;
        };
        let _ = drop_child_inplace(&mut child);
        self.services[idx].pid = None;
        self.services[idx].status = ServiceStatus::Stopped;
        self.services[idx].started_at = None;
        self.services[idx].clear_resources();
        self.system_log(name, "stopped");
    }

    fn build_one(&mut self, name: &str) {
        let Some(idx) = self.services.iter().position(|s| s.config.name == name) else {
            self.system_log(name, &format!("Unknown service: {}", name));
            return;
        };
        let Some(build_cmd) = self.services[idx].config.build.clone() else {
            self.system_log(name, "No build command configured");
            return;
        };
        let argv = match parse_command(&build_cmd) {
            Some(a) if !a.is_empty() => a,
            _ => {
                self.system_log(name, "Empty or invalid `build`");
                return;
            }
        };
        let cwd = self.resolve_cwd(&self.services[idx].config);
        let env = self.services[idx].config.env.clone();
        let log = self.log.clone();
        let was_running = matches!(self.services[idx].status, ServiceStatus::Running);
        if was_running {
            self.system_log(name, "stopping for build");
            self.stop_one(name);
        }
        self.services[idx].status = ServiceStatus::Building;
        self.system_log(name, &format!("build: {}", build_cmd));
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &env {
            cmd.env(k, v);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.services[idx].status = ServiceStatus::Failed;
                self.system_log(name, &format!("build spawn failed: {}", e));
                return;
            }
        };
        if let Some(out) = child.stdout.take() {
            spawn_reader(name.to_string(), out, LogKind::Stdout, log.clone());
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(name.to_string(), err, LogKind::Stderr, log.clone());
        }
        let status = child.wait();
        match status {
            Ok(st) if st.success() => {
                self.services[idx].status = ServiceStatus::Idle;
                self.system_log(name, "build ok");
                if was_running {
                    self.run_one(name);
                }
            }
            Ok(st) => {
                let code = st.code().unwrap_or(-1);
                self.services[idx].status = ServiceStatus::Failed;
                self.system_log(name, &format!("build failed (exit {})", code));
            }
            Err(e) => {
                self.services[idx].status = ServiceStatus::Failed;
                self.system_log(name, &format!("build wait failed: {}", e));
            }
        }
    }

    pub fn tick(&mut self) {
        self.collect_exits();
        self.sample_resources();
    }

    fn collect_exits(&mut self) {
        for service in self.services.iter_mut() {
            let exited = match service.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => Some(status.code().unwrap_or(-1)),
                    Ok(None) => None,
                    Err(_) => Some(-1),
                },
                None => None,
            };
            if let Some(code) = exited {
                service.child = None;
                service.pid = None;
                service.started_at = None;
                service.clear_resources();
                service.status = if code == 0 {
                    ServiceStatus::Exited(0)
                } else {
                    ServiceStatus::Exited(code)
                };
                push_log(
                    &self.log,
                    LogLine {
                        service: service.config.name.clone(),
                        text: format!("[runtime] process exited with code {}", code),
                        kind: LogKind::System,
                    },
                );
            }
        }
    }

    fn sample_resources(&mut self) {
        let now = Instant::now();
        let due = self
            .last_sample_at
            .map(|t| now.duration_since(t) >= RESOURCE_SAMPLE_INTERVAL)
            .unwrap_or(true);
        if !due {
            return;
        }
        self.last_sample_at = Some(now);
        let pids: Vec<sysinfo::Pid> = self
            .services
            .iter()
            .filter_map(|s| s.pid.map(|p| sysinfo::Pid::from_u32(p)))
            .collect();
        if pids.is_empty() {
            return;
        }
        let refresh_kind = sysinfo::ProcessRefreshKind::everything();
        let to_refresh = sysinfo::ProcessesToUpdate::Some(&pids);
        self.sys.refresh_processes_specifics(to_refresh, true, refresh_kind);
        for service in self.services.iter_mut() {
            let Some(raw_pid) = service.pid else { continue };
            let pid = sysinfo::Pid::from_u32(raw_pid);
            let Some(proc) = self.sys.process(pid) else {
                service.clear_resources();
                continue;
            };
            service.cpu_pct = proc.cpu_usage();
            service.ram_bytes = proc.memory();
            let du = proc.disk_usage();
            service.disk_read = du.total_read_bytes;
            service.disk_write = du.total_written_bytes;
        }
    }

    fn affected_services(&self, target: Option<&str>) -> Vec<String> {
        match target {
            Some(name) => self
                .services
                .iter()
                .find(|s| s.config.name == name)
                .map(|s| vec![s.config.name.clone()])
                .unwrap_or_default(),
            None => self.services.iter().map(|s| s.config.name.clone()).collect(),
        }
    }

    fn resolve_targets(&self, target: Option<&str>) -> Result<Vec<String>, String> {
        let names: Vec<&str> = match target {
            Some(name) => {
                if !self.services.iter().any(|s| s.config.name == name) {
                    return Err(format!("Unknown service: {}", name));
                }
                vec![name]
            }
            None => self
                .services
                .iter()
                .map(|s| s.config.name.as_str())
                .collect(),
        };
        let target_set: HashSet<&str> = names.iter().copied().collect();
        let mut visited: HashSet<String> = HashSet::new();
        let mut stack: HashSet<String> = HashSet::new();
        let mut order: Vec<String> = Vec::new();
        for name in names {
            self.topo_visit(name, &target_set, &mut visited, &mut stack, &mut order)?;
        }
        Ok(order)
    }

    fn topo_visit(
        &self,
        name: &str,
        target_set: &HashSet<&str>,
        visited: &mut HashSet<String>,
        stack: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), String> {
        if visited.contains(name) {
            return Ok(());
        }
        if !stack.insert(name.to_string()) {
            return Err(format!("Dependency cycle involving `{}`", name));
        }
        let svc = self
            .services
            .iter()
            .find(|s| s.config.name == name)
            .ok_or_else(|| format!("Unknown dependency: {}", name))?;
        for dep in &svc.config.depends_on {
            if !target_set.contains(dep.as_str())
                && !self.services.iter().any(|s| s.config.name == *dep)
            {
                return Err(format!("Service `{}` depends on unknown `{}`", name, dep));
            }
            self.topo_visit(dep, target_set, visited, stack, order)?;
        }
        stack.remove(name);
        visited.insert(name.to_string());
        order.push(name.to_string());
        Ok(())
    }

    fn resolve_cwd(&self, cfg: &ServiceConfig) -> PathBuf {
        match cfg.cwd.as_ref() {
            Some(rel) => {
                let p = PathBuf::from(rel);
                if p.is_absolute() {
                    p
                } else {
                    self.project_root.join(p)
                }
            }
            None => self.project_root.clone(),
        }
    }

    fn system_log(&self, service: &str, message: &str) {
        push_log(
            &self.log,
            LogLine {
                service: service.to_string(),
                text: format!("[runtime] {}", message),
                kind: LogKind::System,
            },
        );
    }

    pub fn shutdown(&mut self) {
        for service in self.services.iter_mut() {
            if let Some(mut child) = service.child.take() {
                let _ = drop_child_inplace(&mut child);
            }
            service.pid = None;
            service.started_at = None;
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn push_log(log: &Arc<Mutex<VecDeque<LogLine>>>, line: LogLine) {
    let Ok(mut log) = log.lock() else { return };
    if log.len() >= MAX_LOG_LINES {
        let drop = log.len() - MAX_LOG_LINES + 1;
        for _ in 0..drop {
            log.pop_front();
        }
    }
    log.push_back(line);
}

fn spawn_reader<R: Read + Send + 'static>(
    service: String,
    reader: R,
    kind: LogKind,
    log: Arc<Mutex<VecDeque<LogLine>>>,
) {
    thread::spawn(move || {
        let mut buf = BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match buf.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let text = line.trim_end_matches(['\r', '\n']).to_string();
                    push_log(
                        &log,
                        LogLine {
                            service: service.clone(),
                            text,
                            kind,
                            },
                    );
                }
                Err(_) => break,
            }
        }
    });
}

pub fn validate_config(config: &RuntimeConfig) -> Result<()> {
    let mut seen: HashSet<String> = HashSet::new();
    for svc in &config.services {
        if svc.name.trim().is_empty() {
            anyhow::bail!("Service with empty name");
        }
        if !seen.insert(svc.name.clone()) {
            anyhow::bail!("Duplicate service name `{}`", svc.name);
        }
    }
    Ok(())
}

fn parse_command(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for c in trimmed.chars() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ws if ws.is_whitespace() && !in_single && !in_double => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
            }
            other => buf.push(other),
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    Some(out)
}

fn drop_child(mut child: Child) -> Result<(), std::io::Error> {
    drop_child_inplace(&mut child)
}

fn drop_child_inplace(child: &mut Child) -> Result<(), std::io::Error> {
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

pub fn human_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if n >= GB {
        format!("{:.1}G", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.1}M", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.0}K", n as f64 / KB as f64)
    } else {
        format!("{}B", n)
    }
}
