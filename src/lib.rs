use std::num::NonZeroU32;
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

mod editor;
mod midi_worker;
mod params;
mod sysex;
mod ui_widgets;

use midi_worker::{MidiWorker, WorkerCmd};
use params::{Tx81zParams, OP_ACED_BASE, OP_VCED_BASE};
use sysex::Subgroup;

pub struct Tx81zPlugin {
    params: Arc<Tx81zParams>,
    editor_state: Arc<EguiState>,
    midi_worker: MidiWorker,
    last_vced: [u8; 94],
    last_aced: [u8; 23],
    last_channel: u8,
    shadows_initialized: bool,
}

impl Default for Tx81zPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(Tx81zParams::default()),
            editor_state: EguiState::from_size(1080, 740),
            midi_worker: MidiWorker::spawn(),
            last_vced: [0xFF; 94],
            last_aced: [0xFF; 23],
            last_channel: 0xFF,
            shadows_initialized: false,
        }
    }
}

impl Tx81zPlugin {
    #[inline]
    fn check_and_queue(&mut self, subgroup: Subgroup, param: u8, val: u8) {
        let shadow = match subgroup {
            Subgroup::Vced => &mut self.last_vced[param as usize],
            Subgroup::Aced => &mut self.last_aced[param as usize],
            Subgroup::SystemOrRemote => return,
        };
        if *shadow != val {
            *shadow = val;
            if self.shadows_initialized && !self.midi_worker.state.is_tx_suppressed() {
                self.midi_worker
                    .try_send(WorkerCmd::QueueParam(subgroup, param, val));
            }
        }
    }

    fn sync_changed_params(&mut self) {
        let ch = (self.params.midi_channel.value() - 1).clamp(0, 15) as u8;
        if self.last_channel != ch {
            self.last_channel = ch;
            self.midi_worker.try_send(WorkerCmd::SetChannel(ch));
        }

        // Copy values first so we don't hold immutable borrows on self.params while mutating self
        let mut op_vals = [[(Subgroup::Vced, 0u8, 0u8); 18]; 4];
        for (i, op) in self.params.operators().iter().enumerate() {
            op_vals[i] = op.sysex_values(i);
        }
        for group in &op_vals {
            for &(sub, p, v) in group {
                self.check_and_queue(sub, p, v);
            }
        }

        let globals: [(Subgroup, u8, u8); 26] = [
            (Subgroup::Vced, 52, self.params.algorithm.value() as u8),
            (Subgroup::Vced, 53, self.params.feedback.value() as u8),
            (Subgroup::Vced, 54, self.params.lfo_speed.value() as u8),
            (Subgroup::Vced, 55, self.params.lfo_delay.value() as u8),
            (Subgroup::Vced, 56, self.params.pmd.value() as u8),
            (Subgroup::Vced, 57, self.params.amd.value() as u8),
            (Subgroup::Vced, 58, self.params.lfo_sync.value() as u8),
            (Subgroup::Vced, 59, self.params.lfo_wave.value() as u8),
            (Subgroup::Vced, 60, self.params.p_mod_sens.value() as u8),
            (Subgroup::Vced, 61, self.params.a_mod_sens.value() as u8),
            (Subgroup::Vced, 62, self.params.transpose.value() as u8),
            (Subgroup::Vced, 63, self.params.mono_mode.value() as u8),
            (Subgroup::Vced, 64, self.params.pitch_bend_range.value() as u8),
            (Subgroup::Vced, 65, self.params.portamento_mode.value() as u8),
            (Subgroup::Vced, 66, self.params.portamento_time.value() as u8),
            (Subgroup::Vced, 67, self.params.fc_volume.value() as u8),
            (Subgroup::Vced, 71, self.params.mw_pitch.value() as u8),
            (Subgroup::Vced, 72, self.params.mw_amp.value() as u8),
            (Subgroup::Vced, 73, self.params.bc_pitch.value() as u8),
            (Subgroup::Vced, 74, self.params.bc_amp.value() as u8),
            (Subgroup::Vced, 75, self.params.bc_pitch_bias.value() as u8),
            (Subgroup::Vced, 76, self.params.bc_eg_bias.value() as u8),
            (Subgroup::Vced, 93, self.params.op_enable_mask()),
            (Subgroup::Aced, 20, self.params.reverb_rate.value() as u8),
            (Subgroup::Aced, 21, self.params.fc_pitch.value() as u8),
            (Subgroup::Aced, 22, self.params.fc_amp.value() as u8),
        ];
        for (sub, p, v) in globals {
            self.check_and_queue(sub, p, v);
        }

        // Suppress unused import warnings while keeping OP_*_BASE public for editor
        let _ = (OP_VCED_BASE, OP_ACED_BASE);
        self.shadows_initialized = true;
    }
}

impl Plugin for Tx81zPlugin {
    const NAME: &'static str = "Yamaha TX81Z Editor";
    const VENDOR: &'static str = "TX81Z-CLAP";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(
            self.params.clone(),
            self.editor_state.clone(),
            self.midi_worker.state.clone(),
            self.midi_worker.tx.clone(),
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        if let Ok(out) = self.params.saved_midi_out.lock() {
            if !out.is_empty() {
                self.midi_worker.try_send(WorkerCmd::ConnectOut(out.clone()));
            }
        }
        if let Ok(inp) = self.params.saved_midi_in.lock() {
            if !inp.is_empty() {
                self.midi_worker.try_send(WorkerCmd::ConnectIn(inp.clone()));
            }
        }
        self.sync_changed_params();
        true
    }

    fn process(
        &mut self,
        _buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::NoteOn {
                    channel,
                    note,
                    velocity,
                    ..
                } => {
                    let vel = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
                    self.midi_worker.try_send(WorkerCmd::SendMidi3(
                        [0x90 | (channel & 0x0F), note & 0x7F, vel],
                        3,
                    ));
                }
                NoteEvent::NoteOff {
                    channel,
                    note,
                    velocity,
                    ..
                } => {
                    let vel = (velocity * 127.0).round().clamp(0.0, 127.0) as u8;
                    self.midi_worker.try_send(WorkerCmd::SendMidi3(
                        [0x80 | (channel & 0x0F), note & 0x7F, vel],
                        3,
                    ));
                }
                NoteEvent::MidiCC {
                    channel, cc, value, ..
                } => {
                    let val = (value * 127.0).round().clamp(0.0, 127.0) as u8;
                    self.midi_worker.try_send(WorkerCmd::SendMidi3(
                        [0xB0 | (channel & 0x0F), cc & 0x7F, val],
                        3,
                    ));
                }
                NoteEvent::MidiPitchBend { channel, value, .. } => {
                    let bend = (value * 16383.0).round().clamp(0.0, 16383.0) as u16;
                    let lsb = (bend & 0x7F) as u8;
                    let msb = ((bend >> 7) & 0x7F) as u8;
                    self.midi_worker.try_send(WorkerCmd::SendMidi3(
                        [0xE0 | (channel & 0x0F), lsb, msb],
                        3,
                    ));
                }
                NoteEvent::MidiProgramChange {
                    channel, program, ..
                } => {
                    self.midi_worker.try_send(WorkerCmd::SendMidi3(
                        [0xC0 | (channel & 0x0F), program & 0x7F, 0],
                        2,
                    ));
                }
                NoteEvent::MidiChannelPressure {
                    channel, pressure, ..
                } => {
                    let val = (pressure * 127.0).round().clamp(0.0, 127.0) as u8;
                    self.midi_worker.try_send(WorkerCmd::SendMidi3(
                        [0xD0 | (channel & 0x0F), val, 0],
                        2,
                    ));
                }
                _ => {}
            }
            context.send_event(event);
        }
        self.sync_changed_params();
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Tx81zPlugin {
    const CLAP_ID: &'static str = "org.tx81z.editor";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Full MIDI SysEx hardware controller for the Yamaha TX81Z");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

nih_export_clap!(Tx81zPlugin);
