use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

use crate::sysex::{
    build_aced_bulk_dump, build_aced_dump_request, build_effect_dump_request, build_param_change,
    build_pced_bulk_dump, build_pced_dump_request, build_prog_table_dump_request,
    build_remote_switch, build_system_dump_request, build_vced_bulk_dump, build_vced_dump_request,
    format_hex, parse_sysex_dump, ParsedDump, Subgroup,
};

const SYSEX_PACING: Duration = Duration::from_millis(45);
const MAX_LOG_LINES: usize = 80;

#[derive(Debug, Clone)]
pub enum WorkerCmd {
    ConnectOut(String),
    ConnectIn(String),
    SetChannel(u8),
    QueueParam(Subgroup, u8, u8),
    PushVoiceBulk { vced: [u8; 93], aced: [u8; 23] },
    PushPerfBulk([u8; 110]),
    SendRemoteSwitch(u8),
    SendRawSysex(Vec<Vec<u8>>),
    SendMidi3([u8; 3], u8),
    SelectProgram(u8),
    RequestVoiceDump,
    UnsolicitedVcedSeen,
    RequestPerfDump,
    RequestSystemDump,
    Shutdown,
}

fn monotonic_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[derive(Clone)]
pub struct SharedMidiState {
    pub selected_out: Arc<Mutex<String>>,
    pub selected_in: Arc<Mutex<String>>,
    pub status: Arc<Mutex<String>>,
    pub logs: Arc<Mutex<VecDeque<String>>>,
    pub incoming_dumps: Arc<Mutex<VecDeque<ParsedDump>>>,
    pub captured_system_dumps: Arc<Mutex<Vec<Vec<u8>>>>,
    pub suppress_tx_until_ms: Arc<AtomicU64>,
    pub current_program: Arc<Mutex<Option<u8>>>,
}

impl Default for SharedMidiState {
    fn default() -> Self {
        Self {
            selected_out: Arc::new(Mutex::new(String::new())),
            selected_in: Arc::new(Mutex::new(String::new())),
            status: Arc::new(Mutex::new("Disconnected".to_string())),
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES))),
            incoming_dumps: Arc::new(Mutex::new(VecDeque::new())),
            captured_system_dumps: Arc::new(Mutex::new(Vec::new())),
            suppress_tx_until_ms: Arc::new(AtomicU64::new(0)),
            current_program: Arc::new(Mutex::new(None)),
        }
    }
}

impl SharedMidiState {
    pub fn suppress_tx_for(&self, millis: u64) {
        self.suppress_tx_until_ms
            .store(monotonic_ms().saturating_add(millis), Ordering::Relaxed);
    }

    pub fn is_tx_suppressed(&self) -> bool {
        monotonic_ms() < self.suppress_tx_until_ms.load(Ordering::Relaxed)
    }
    pub fn push_log(&self, line: String) {
        if let Ok(mut logs) = self.logs.lock() {
            if logs.len() >= MAX_LOG_LINES {
                logs.pop_front();
            }
            logs.push_back(line);
        }
    }

    pub fn set_status(&self, msg: impl Into<String>) {
        if let Ok(mut s) = self.status.lock() {
            *s = msg.into();
        }
    }
}

pub fn list_midi_ports() -> (Vec<String>, Vec<String>) {
    let outs = MidiOutput::new("tx81z-scan-out")
        .map(|m| {
            m.ports()
                .iter()
                .filter_map(|p| m.port_name(p).ok())
                .collect()
        })
        .unwrap_or_default();

    let ins = MidiInput::new("tx81z-scan-in")
        .map(|m| {
            m.ports()
                .iter()
                .filter_map(|p| m.port_name(p).ok())
                .collect()
        })
        .unwrap_or_default();

    (outs, ins)
}

pub struct MidiWorker {
    pub tx: Sender<WorkerCmd>,
    pub state: SharedMidiState,
    handle: Option<JoinHandle<()>>,
}

impl MidiWorker {
    pub fn spawn() -> Self {
        let (tx, rx) = crossbeam_channel::bounded(2048);
        let state = SharedMidiState::default();
        let worker_state = state.clone();
        let worker_tx = tx.clone();

        let handle = thread::Builder::new()
            .name("tx81z-sysex-worker".to_string())
            .spawn(move || run_worker(rx, worker_tx, worker_state))
            .expect("failed to spawn tx81z sysex worker");

        Self {
            tx,
            state,
            handle: Some(handle),
        }
    }

    pub fn try_send(&self, cmd: WorkerCmd) {
        let _ = self.tx.try_send(cmd);
    }
}

impl Drop for MidiWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerCmd::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn connect_output(port_name: &str, state: &SharedMidiState) -> Option<MidiOutputConnection> {
    let midi_out = MidiOutput::new("TX81Z-CLAP-Out").ok()?;
    let port = midi_out
        .ports()
        .into_iter()
        .find(|p| midi_out.port_name(p).ok().as_deref() == Some(port_name))?;

    match midi_out.connect(&port, "tx81z-sysex-out") {
        Ok(conn) => {
            if let Ok(mut sel) = state.selected_out.lock() {
                *sel = port_name.to_string();
            }
            state.set_status(format!("Out: {}", port_name));
            state.push_log(format!("[INFO] Connected MIDI Out: {}", port_name));
            Some(conn)
        }
        Err(e) => {
            state.set_status(format!("Out Err: {}", e));
            state.push_log(format!("[ERR] MIDI Out failed: {}", e));
            None
        }
    }
}

fn connect_input(
    port_name: &str,
    state: &SharedMidiState,
    cmd_tx: &Sender<WorkerCmd>,
) -> Option<MidiInputConnection<()>> {
    let mut midi_in = MidiInput::new("TX81Z-CLAP-In").ok()?;
    midi_in.ignore(Ignore::None);
    let port = midi_in
        .ports()
        .into_iter()
        .find(|p| midi_in.port_name(p).ok().as_deref() == Some(port_name))?;

    let cb_state = state.clone();
    let cb_tx = cmd_tx.clone();
    match midi_in.connect(
        &port,
        "tx81z-sysex-in",
        move |_stamp, message, _| {
            if message.first() == Some(&0xF0) {
                let preview = if message.len() > 24 {
                    format!("{} ... ({} bytes)", format_hex(&message[..24]), message.len())
                } else {
                    format_hex(message)
                };
                cb_state.push_log(format!("RX: {}", preview));
                if let Ok(mut sys) = cb_state.captured_system_dumps.lock() {
                    sys.push(message.to_vec());
                }
                if let Some(dump) = parse_sysex_dump(message) {
                    if matches!(dump, ParsedDump::Vced(_)) {
                        let _ = cb_tx.try_send(WorkerCmd::UnsolicitedVcedSeen);
                    }
                    if let Ok(mut q) = cb_state.incoming_dumps.lock() {
                        q.push_back(dump);
                    }
                }
            } else if message.first() != Some(&0xFE) && message.first() != Some(&0xF8) {
                if let (Some(&status), Some(&prog)) = (message.first(), message.get(1)) {
                    if (status & 0xF0) == 0xC0 {
                        if let Ok(mut cp) = cb_state.current_program.lock() {
                            *cp = Some(prog & 0x7F);
                        }
                    }
                }
                cb_state.push_log(format!("RX MIDI: {}", format_hex(message)));
            }
        },
        (),
    ) {
        Ok(conn) => {
            if let Ok(mut sel) = state.selected_in.lock() {
                *sel = port_name.to_string();
            }
            state.push_log(format!("[INFO] Connected MIDI In: {}", port_name));
            Some(conn)
        }
        Err(e) => {
            state.set_status(format!("In Err: {}", e));
            state.push_log(format!("[ERR] MIDI In failed: {}", e));
            None
        }
    }
}

fn run_worker(rx: Receiver<WorkerCmd>, cmd_tx: Sender<WorkerCmd>, state: SharedMidiState) {
    let mut out_conn: Option<MidiOutputConnection> = None;
    let mut _in_conn: Option<MidiInputConnection<()>> = None;
    let mut channel: u8 = 0;
    let mut pending: BTreeMap<(u8, u8), u8> = BTreeMap::new();
    let mut last_send = Instant::now() - SYSEX_PACING;
    let mut last_req_at = Instant::now() - Duration::from_secs(10);
    let mut pending_auto_fetch_at: Option<Instant> = None;

    loop {
        let mut timeout = if pending.is_empty() {
            Duration::from_millis(50)
        } else {
            SYSEX_PACING.saturating_sub(last_send.elapsed())
        };
        if let Some(t) = pending_auto_fetch_at {
            timeout = timeout.min(t.saturating_duration_since(Instant::now()));
        }

        match rx.recv_timeout(timeout) {
            Ok(cmd) => {
                if !handle_cmd(
                    cmd,
                    &mut out_conn,
                    &mut _in_conn,
                    &mut channel,
                    &mut pending,
                    &mut last_req_at,
                    &mut pending_auto_fetch_at,
                    &cmd_tx,
                    &state,
                ) {
                    break;
                }
                while let Ok(more) = rx.try_recv() {
                    if !handle_cmd(
                        more,
                        &mut out_conn,
                        &mut _in_conn,
                        &mut channel,
                        &mut pending,
                        &mut last_req_at,
                        &mut pending_auto_fetch_at,
                        &cmd_tx,
                        &state,
                    ) {
                        return;
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }

        if let Some(t) = pending_auto_fetch_at {
            if Instant::now() >= t {
                pending_auto_fetch_at = None;
                last_req_at = Instant::now();
                request_voice_dump(&mut out_conn, channel, &state);
            }
        }

        if !pending.is_empty() && last_send.elapsed() >= SYSEX_PACING {
            if let Some((&(sub, param), &val)) = pending.iter().next() {
                pending.remove(&(sub, param));
                let subgroup = match sub {
                    0x12 => Subgroup::Vced,
                    0x13 => Subgroup::Aced,
                    _ => Subgroup::SystemOrRemote,
                };
                let msg = build_param_change(channel, subgroup, param, val);
                if let Some(conn) = out_conn.as_mut() {
                    if conn.send(&msg).is_ok() {
                        state.push_log(format!("TX: {}", format_hex(&msg)));
                    }
                }
                last_send = Instant::now();
            }
        }
    }
}

fn request_voice_dump(
    out_conn: &mut Option<MidiOutputConnection>,
    channel: u8,
    state: &SharedMidiState,
) {
    if let Some(conn) = out_conn.as_mut() {
        let vced_req = build_vced_dump_request(channel);
        let _ = conn.send(&vced_req);
        state.push_log(format!("TX: {}", format_hex(&vced_req)));

        thread::sleep(Duration::from_millis(250));
        let aced_req = build_aced_dump_request(channel);
        let _ = conn.send(&aced_req);
        state.push_log(format!("TX: {}", format_hex(&aced_req)));
    } else {
        state.push_log("[WARN] Connect MIDI Out before requesting dump".to_string());
    }
}

fn handle_cmd(
    cmd: WorkerCmd,
    out_conn: &mut Option<MidiOutputConnection>,
    in_conn: &mut Option<MidiInputConnection<()>>,
    channel: &mut u8,
    pending: &mut BTreeMap<(u8, u8), u8>,
    last_req_at: &mut Instant,
    pending_auto_fetch_at: &mut Option<Instant>,
    cmd_tx: &Sender<WorkerCmd>,
    state: &SharedMidiState,
) -> bool {
    match cmd {
        WorkerCmd::ConnectOut(name) => {
            *out_conn = None;
            *out_conn = connect_output(&name, state);
        }
        WorkerCmd::ConnectIn(name) => {
            *in_conn = None;
            *in_conn = connect_input(&name, state, cmd_tx);
        }
        WorkerCmd::SetChannel(ch) => {
            *channel = ch & 0x0F;
        }
        WorkerCmd::QueueParam(subgroup, param, value) => {
            pending.insert((subgroup as u8, param), value);
        }
        WorkerCmd::PushVoiceBulk { vced, aced } => {
            pending.clear();
            if let Some(conn) = out_conn.as_mut() {
                let vced_msg = build_vced_bulk_dump(*channel, &vced);
                let _ = conn.send(&vced_msg);
                state.push_log(format!("TX: VCED Bulk ({} bytes)", vced_msg.len()));

                thread::sleep(Duration::from_millis(80));
                let aced_msg = build_aced_bulk_dump(*channel, &aced);
                let _ = conn.send(&aced_msg);
                state.push_log(format!("TX: ACED Bulk ({} bytes)", aced_msg.len()));
            } else {
                state.push_log("[WARN] Connect MIDI Out before pushing voice".to_string());
            }
        }
        WorkerCmd::PushPerfBulk(pced) => {
            if let Some(conn) = out_conn.as_mut() {
                let msg = build_pced_bulk_dump(*channel, &pced);
                let _ = conn.send(&msg);
                state.push_log(format!("TX: PCED Bulk ({} bytes)", msg.len()));
            } else {
                state.push_log("[WARN] Connect MIDI Out before pushing perf".to_string());
            }
        }
        WorkerCmd::SendRemoteSwitch(sw) => {
            if let Some(conn) = out_conn.as_mut() {
                let on_msg = build_remote_switch(*channel, sw, true);
                let _ = conn.send(&on_msg);
                state.push_log(format!("TX: Remote SW {} ON", sw));
                thread::sleep(Duration::from_millis(45));
                let off_msg = build_remote_switch(*channel, sw, false);
                let _ = conn.send(&off_msg);
            }
        }
        WorkerCmd::SendRawSysex(msgs) => {
            if let Some(conn) = out_conn.as_mut() {
                for (i, msg) in msgs.iter().enumerate() {
                    if i > 0 {
                        thread::sleep(Duration::from_millis(150));
                    }
                    let _ = conn.send(msg);
                    state.push_log(format!("TX: Raw SysEx ({} bytes)", msg.len()));
                }
            } else {
                state.push_log("[WARN] Connect MIDI Out before sending .syx".to_string());
            }
        }
        WorkerCmd::SendMidi3(bytes, len) => {
            if (bytes[0] & 0xF0) == 0xC0 {
                if let Ok(mut cp) = state.current_program.lock() {
                    *cp = Some(bytes[1] & 0x7F);
                }
            }
            if let Some(conn) = out_conn.as_mut() {
                let n = (len as usize).min(3);
                let _ = conn.send(&bytes[..n]);
            }
        }
        WorkerCmd::SelectProgram(prog) => {
            let p = prog & 0x7F;
            if let Ok(mut cp) = state.current_program.lock() {
                *cp = Some(p);
            }
            if let Some(conn) = out_conn.as_mut() {
                *last_req_at = Instant::now();
                let pc = [0xC0 | (*channel & 0x0F), p];
                let _ = conn.send(&pc);
                state.push_log(format!("TX: Program Change {:02X}", p));

                thread::sleep(Duration::from_millis(150));
                request_voice_dump(out_conn, *channel, state);
            } else {
                state.push_log("[WARN] Connect MIDI Out before selecting program".to_string());
            }
        }
        WorkerCmd::RequestVoiceDump => {
            *last_req_at = Instant::now();
            request_voice_dump(out_conn, *channel, state);
        }
        WorkerCmd::UnsolicitedVcedSeen => {
            if last_req_at.elapsed() > Duration::from_millis(900)
                && pending_auto_fetch_at.is_none()
            {
                *pending_auto_fetch_at = Some(Instant::now() + Duration::from_millis(200));
            }
        }
        WorkerCmd::RequestPerfDump => {
            if let Some(conn) = out_conn.as_mut() {
                let req = build_pced_dump_request(*channel);
                let _ = conn.send(&req);
                state.push_log(format!("TX: {}", format_hex(&req)));
            } else {
                state.push_log("[WARN] Connect MIDI Out before requesting perf".to_string());
            }
        }
        WorkerCmd::RequestSystemDump => {
            if let Some(conn) = out_conn.as_mut() {
                if let Ok(mut sys) = state.captured_system_dumps.lock() {
                    sys.clear();
                }
                for req in [
                    build_system_dump_request(*channel),
                    build_prog_table_dump_request(*channel),
                    build_effect_dump_request(*channel),
                ] {
                    let _ = conn.send(&req);
                    state.push_log(format!("TX: {}", format_hex(&req)));
                    thread::sleep(Duration::from_millis(200));
                }
            } else {
                state.push_log("[WARN] Connect MIDI Out before requesting system".to_string());
            }
        }
        WorkerCmd::Shutdown => return false,
    }
    true
}
