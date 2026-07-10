//! Portable supervision for line-oriented child processes.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RestartPolicy {
    /// Number of restarts allowed during `window` (the initial start is free).
    pub max_restarts: usize,
    pub window: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            window: Duration::from_secs(60),
            initial_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SidecarConfig {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub restart: RestartPolicy,
    pub health_interval: Duration,
    pub shutdown_timeout: Duration,
    /// Frequency of exit and shutdown checks. Kept configurable for bounded tests.
    pub poll_interval: Duration,
}

impl SidecarConfig {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: HashMap::new(),
            restart: RestartPolicy::default(),
            health_interval: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(2),
            poll_interval: Duration::from_millis(20),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarStatus {
    Starting,
    Healthy,
    Restarting,
    Stopped,
    Failed,
}

#[derive(Debug)]
pub enum SidecarError {
    Spawn(std::io::Error),
    Io(std::io::Error),
    Disconnected,
    AlreadyStopped,
}

impl fmt::Display for SidecarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(e) => write!(f, "could not start sidecar: {e}"),
            Self::Io(e) => write!(f, "sidecar I/O failed: {e}"),
            Self::Disconnected => write!(f, "sidecar output disconnected"),
            Self::AlreadyStopped => write!(f, "sidecar supervisor is already stopped"),
        }
    }
}

impl std::error::Error for SidecarError {}

#[derive(Clone)]
pub struct LineWriter(Arc<Mutex<Option<BufWriter<ChildStdin>>>>);

impl LineWriter {
    pub fn write_line(&self, line: &str) -> Result<(), SidecarError> {
        let mut guard = self.0.lock().unwrap();
        let writer = guard.as_mut().ok_or(SidecarError::Disconnected)?;
        writer
            .write_all(line.as_bytes())
            .map_err(SidecarError::Io)?;
        writer.write_all(b"\n").map_err(SidecarError::Io)?;
        writer.flush().map_err(SidecarError::Io)
    }
}

#[derive(Clone)]
pub struct LineReader(Arc<Mutex<mpsc::Receiver<String>>>);

impl LineReader {
    pub fn read_line(&self) -> Result<String, SidecarError> {
        self.0
            .lock()
            .unwrap()
            .recv()
            .map_err(|_| SidecarError::Disconnected)
    }

    pub fn read_line_timeout(&self, timeout: Duration) -> Result<Option<String>, SidecarError> {
        match self.0.lock().unwrap().recv_timeout(timeout) {
            Ok(line) => Ok(Some(line)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(SidecarError::Disconnected),
        }
    }
}

#[derive(Clone)]
pub struct SidecarIo {
    pub stdin: LineWriter,
    pub stdout: LineReader,
    pub stderr: LineReader,
}

type HealthProbe = dyn Fn(&SidecarIo) -> Result<(), String> + Send + Sync + 'static;

pub struct SidecarSupervisor {
    status: Arc<Mutex<SidecarStatus>>,
    io: SidecarIo,
    command: mpsc::Sender<SupervisorCommand>,
    worker: Option<JoinHandle<()>>,
}

enum SupervisorCommand {
    Shutdown(mpsc::Sender<()>),
}

impl SidecarSupervisor {
    pub fn spawn(
        config: SidecarConfig,
        health_probe: impl Fn(&SidecarIo) -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<Self, SidecarError> {
        if config.program.is_empty() {
            return Err(SidecarError::Spawn(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "program is empty",
            )));
        }
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();
        let stdin = LineWriter(Arc::new(Mutex::new(None)));
        let io = SidecarIo {
            stdin: stdin.clone(),
            stdout: LineReader(Arc::new(Mutex::new(stdout_rx))),
            stderr: LineReader(Arc::new(Mutex::new(stderr_rx))),
        };
        let status = Arc::new(Mutex::new(SidecarStatus::Starting));
        let (command, commands) = mpsc::channel();
        let worker_status = status.clone();
        let worker_io = io.clone();
        let probe: Arc<HealthProbe> = Arc::new(health_probe);
        let worker = thread::spawn(move || {
            supervise(
                config,
                worker_status,
                worker_io,
                stdout_tx,
                stderr_tx,
                commands,
                probe,
            )
        });
        Ok(Self {
            status,
            io,
            command,
            worker: Some(worker),
        })
    }

    pub fn status(&self) -> SidecarStatus {
        *self.status.lock().unwrap()
    }

    pub fn io(&self) -> SidecarIo {
        self.io.clone()
    }

    pub fn shutdown(&mut self) -> Result<(), SidecarError> {
        let Some(worker) = self.worker.take() else {
            return Err(SidecarError::AlreadyStopped);
        };
        let (done_tx, done_rx) = mpsc::channel();
        self.command
            .send(SupervisorCommand::Shutdown(done_tx))
            .map_err(|_| SidecarError::AlreadyStopped)?;
        let _ = done_rx.recv();
        let _ = worker.join();
        Ok(())
    }
}

impl Drop for SidecarSupervisor {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn supervise(
    config: SidecarConfig,
    status: Arc<Mutex<SidecarStatus>>,
    io: SidecarIo,
    stdout_tx: mpsc::Sender<String>,
    stderr_tx: mpsc::Sender<String>,
    commands: mpsc::Receiver<SupervisorCommand>,
    probe: Arc<HealthProbe>,
) {
    let mut restarts = VecDeque::new();
    let mut consecutive_failures = 0u32;
    loop {
        set_status(
            &status,
            if consecutive_failures == 0 {
                SidecarStatus::Starting
            } else {
                SidecarStatus::Restarting
            },
        );
        if consecutive_failures > 0 {
            let shift = consecutive_failures.saturating_sub(1).min(31);
            let delay = config
                .restart
                .initial_backoff
                .saturating_mul(1u32 << shift)
                .min(config.restart.max_backoff);
            if wait_or_shutdown(delay, &commands, &io, None, &config, &status) {
                return;
            }
        }
        let mut child = match spawn_child(&config, &io, stdout_tx.clone(), stderr_tx.clone()) {
            Ok(child) => child,
            Err(_) => {
                if !allow_restart(&config.restart, &mut restarts) {
                    set_status(&status, SidecarStatus::Failed);
                    return;
                }
                consecutive_failures = consecutive_failures.saturating_add(1);
                continue;
            }
        };
        set_status(&status, SidecarStatus::Healthy);
        let mut next_probe = Instant::now() + config.health_interval;
        let restart = loop {
            match commands.recv_timeout(config.poll_interval) {
                Ok(SupervisorCommand::Shutdown(done)) => {
                    stop_child(
                        &mut child,
                        &io,
                        config.shutdown_timeout,
                        config.poll_interval,
                    );
                    set_status(&status, SidecarStatus::Stopped);
                    let _ = done.send(());
                    return;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    stop_child(
                        &mut child,
                        &io,
                        config.shutdown_timeout,
                        config.poll_interval,
                    );
                    set_status(&status, SidecarStatus::Stopped);
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break true,
                Ok(None) => {}
            }
            if Instant::now() >= next_probe {
                next_probe = Instant::now() + config.health_interval;
                if probe(&io).is_err() {
                    stop_child(
                        &mut child,
                        &io,
                        config.shutdown_timeout,
                        config.poll_interval,
                    );
                    break true;
                }
                consecutive_failures = 0;
            }
        };
        if restart {
            *io.stdin.0.lock().unwrap() = None;
            if !allow_restart(&config.restart, &mut restarts) {
                set_status(&status, SidecarStatus::Failed);
                return;
            }
            consecutive_failures = consecutive_failures.saturating_add(1);
        }
    }
}

fn spawn_child(
    config: &SidecarConfig,
    io: &SidecarIo,
    out: mpsc::Sender<String>,
    err: mpsc::Sender<String>,
) -> Result<Child, std::io::Error> {
    let mut child = Command::new(&config.program)
        .args(&config.args)
        .envs(&config.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    *io.stdin.0.lock().unwrap() = child.stdin.take().map(BufWriter::new);
    pipe_lines(child.stdout.take().unwrap(), out);
    pipe_error_lines(child.stderr.take().unwrap(), err);
    Ok(child)
}

fn pipe_lines(pipe: ChildStdout, tx: mpsc::Sender<String>) {
    thread::spawn(move || forward_lines(pipe, tx));
}
fn pipe_error_lines(pipe: ChildStderr, tx: mpsc::Sender<String>) {
    thread::spawn(move || forward_lines(pipe, tx));
}
fn forward_lines(pipe: impl std::io::Read, tx: mpsc::Sender<String>) {
    for line in BufReader::new(pipe).lines() {
        match line {
            Ok(line) => {
                if tx.send(line).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn allow_restart(policy: &RestartPolicy, history: &mut VecDeque<Instant>) -> bool {
    let now = Instant::now();
    while history
        .front()
        .is_some_and(|at| now.duration_since(*at) > policy.window)
    {
        history.pop_front();
    }
    if history.len() >= policy.max_restarts {
        return false;
    }
    history.push_back(now);
    true
}

fn stop_child(child: &mut Child, io: &SidecarIo, deadline: Duration, poll: Duration) {
    *io.stdin.0.lock().unwrap() = None;
    let until = Instant::now() + deadline;
    while Instant::now() < until {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(poll.min(until.saturating_duration_since(Instant::now())));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_or_shutdown(
    delay: Duration,
    commands: &mpsc::Receiver<SupervisorCommand>,
    io: &SidecarIo,
    child: Option<&mut Child>,
    config: &SidecarConfig,
    status: &Arc<Mutex<SidecarStatus>>,
) -> bool {
    match commands.recv_timeout(delay) {
        Ok(SupervisorCommand::Shutdown(done)) => {
            if let Some(child) = child {
                stop_child(child, io, config.shutdown_timeout, config.poll_interval);
            }
            set_status(status, SidecarStatus::Stopped);
            let _ = done.send(());
            true
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            set_status(status, SidecarStatus::Stopped);
            true
        }
        Err(mpsc::RecvTimeoutError::Timeout) => false,
    }
}

fn set_status(status: &Arc<Mutex<SidecarStatus>>, value: SidecarStatus) {
    *status.lock().unwrap() = value;
}
