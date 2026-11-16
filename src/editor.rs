use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Color32, RichText};
use nih_plug_egui::{create_egui_editor, EguiState};

use crate::midi_worker::{list_midi_ports, SharedMidiState, WorkerCmd};
use crate::params::{OperatorParams, Tx81zParams, OP_ACED_BASE, OP_VCED_BASE};
use crate::sysex::{
    build_aced_bulk_dump, build_pced_bulk_dump, build_vced_bulk_dump, split_syx_messages,
    ParsedDump,
};
use crate::ui_widgets::{
    draw_algorithm_diagram, draw_eg_graph, draw_lfo_preview, draw_waveform_preview, is_carrier,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Operators,
    VoiceLfo,
    Performance,
    PatchesUtility,
    SysexLog,
}

const ALG_DESCS: [&str; 8] = [
    "1: 4->3->2->1 (Serial)",
    "2: (4+3)->2->1",
    "3: (4->3+2)->1",
    "4: (4->3)->2+1",
    "5: 4->3 + 2->1 (Dual)",
    "6: 4->(1+2+3)",
    "7: 4->3 + 2 + 1",
    "8: 1+2+3+4 (Organ)",
];

struct EditorUiState {
    tab: Tab,
    out_ports: Vec<String>,
    in_ports: Vec<String>,
    voice_name: String,
    sel_bank: u8,
    sel_slot: u8,
    pced: [u8; 110],
    perf_name: String,
    patch_filename: String,
    patch_files: Vec<String>,
    patch_status: String,
}

impl Default for EditorUiState {
    fn default() -> Self {
        let (out_ports, in_ports) = list_midi_ports();
        let mut pced = [0u8; 110];
        for i in 0..8 {
            let b = i * 12;
            pced[b] = if i == 0 { 8 } else { 0 }; // Max notes
            pced[b + 3] = i as u8; // Receive channel
            pced[b + 5] = 127; // High note limit
            pced[b + 6] = 7; // Detune center
            pced[b + 7] = 24; // Note shift center
            pced[b + 8] = 90; // Volume
            pced[b + 9] = 3; // Out I+II
        }
        pced[100..110].copy_from_slice(b"TX81Z PERF");

        let mut st = Self {
            tab: Tab::Operators,
            out_ports,
            in_ports,
            voice_name: "TX81Z CLAP".to_string(),
            sel_bank: 0,
            sel_slot: 1,
            pced,
            perf_name: "TX81Z PERF".to_string(),
            patch_filename: "my_patch.syx".to_string(),
            patch_files: Vec::new(),
            patch_status: String::new(),
        };
        st.refresh_patch_list();
        st
    }
}

impl EditorUiState {
    fn patch_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join("TX81Z_Patches")
    }

    fn refresh_patch_list(&mut self) {
        let dir = Self::patch_dir();
        let _ = fs::create_dir_all(&dir);
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.to_lowercase().ends_with(".syx") {
                        files.push(name.to_string());
                    }
                }
            }
        }
        files.sort();
        self.patch_files = files;
    }
}

pub fn create(
    params: Arc<Tx81zParams>,
    editor_state: Arc<EguiState>,
    midi_state: SharedMidiState,
    cmd_tx: crossbeam_channel::Sender<WorkerCmd>,
) -> Option<Box<dyn Editor>> {
    let ui_state = Arc::new(Mutex::new(EditorUiState::default()));

    create_egui_editor(
        editor_state,
        (),
        |_, _| {},
        move |egui_ctx, setter, _state| {
            egui_ctx.request_repaint_after(std::time::Duration::from_millis(200));
            let mut ui_st = ui_state.lock().unwrap();
            apply_incoming_dumps(&params, &midi_state, &mut ui_st, setter);

            let status = midi_state.status.lock().unwrap().clone();
            let status_color = if status.contains("Err") || status == "Disconnected" {
                Color32::from_rgb(245, 130, 110)
            } else {
                Color32::from_rgb(130, 225, 155)
            };

            egui::TopBottomPanel::bottom("tx81z_footer").show(egui_ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("MIDI: {}", status))
                            .small()
                            .color(status_color),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "v{} ({})",
                                env!("CARGO_PKG_VERSION"),
                                env!("TX81Z_BUILD_TIME")
                            ))
                            .small()
                            .color(Color32::GRAY),
                        );
                    });
                });
            });

            egui::CentralPanel::default().show(egui_ctx, |ui| {
                draw_top_bar(ui, &params, &midi_state, &cmd_tx, &mut ui_st, setter);
                ui.separator();

                ui.horizontal(|ui| {
                    ui.selectable_value(&mut ui_st.tab, Tab::Operators, "Operators (1-4)");
                    ui.selectable_value(&mut ui_st.tab, Tab::VoiceLfo, "Voice / LFO");
                    ui.selectable_value(&mut ui_st.tab, Tab::Performance, "Performance (1-8)");
                    ui.selectable_value(
                        &mut ui_st.tab,
                        Tab::PatchesUtility,
                        "Patches & Utility (.syx)",
                    );
                    ui.selectable_value(&mut ui_st.tab, Tab::SysexLog, "SysEx Log");
                });
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| match ui_st.tab {
                    Tab::Operators => draw_operators_tab(ui, &params, &mut ui_st, setter),
                    Tab::VoiceLfo => draw_voice_tab(ui, &params, setter),
                    Tab::Performance => draw_performance_tab(ui, &cmd_tx, &mut ui_st),
                    Tab::PatchesUtility => {
                        draw_patches_tab(ui, &params, &midi_state, &cmd_tx, &mut ui_st, setter)
                    }
                    Tab::SysexLog => draw_log_tab(ui, &midi_state),
                });
            });
        },
    )
}

fn draw_top_bar(
    ui: &mut egui::Ui,
    params: &Tx81zParams,
    midi_state: &SharedMidiState,
    cmd_tx: &crossbeam_channel::Sender<WorkerCmd>,
    ui_st: &mut EditorUiState,
    setter: &ParamSetter,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("YAMAHA TX81Z")
                .strong()
                .color(Color32::from_rgb(100, 210, 255)),
        );

        let sel_out = midi_state.selected_out.lock().unwrap().clone();
        egui::ComboBox::from_id_salt("midi_out")
            .selected_text(if sel_out.is_empty() {
                "MIDI Out..."
            } else {
                &sel_out
            })
            .width(165.0)
            .show_ui(ui, |ui| {
                for port in &ui_st.out_ports {
                    if ui.selectable_label(sel_out == *port, port).clicked() {
                        if let Ok(mut s) = params.saved_midi_out.lock() {
                            *s = port.clone();
                        }
                        let _ = cmd_tx.send(WorkerCmd::ConnectOut(port.clone()));
                    }
                }
            });

        let sel_in = midi_state.selected_in.lock().unwrap().clone();
        egui::ComboBox::from_id_salt("midi_in")
            .selected_text(if sel_in.is_empty() {
                "MIDI In..."
            } else {
                &sel_in
            })
            .width(165.0)
            .show_ui(ui, |ui| {
                for port in &ui_st.in_ports {
                    if ui.selectable_label(sel_in == *port, port).clicked() {
                        if let Ok(mut s) = params.saved_midi_in.lock() {
                            *s = port.clone();
                        }
                        let _ = cmd_tx.send(WorkerCmd::ConnectIn(port.clone()));
                    }
                }
            });

        if ui.button("Refresh").clicked() {
            let (outs, ins) = list_midi_ports();
            ui_st.out_ports = outs;
            ui_st.in_ports = ins;
        }

        ui.label("Ch:");
        let mut ch = params.midi_channel.value();
        if ui.add(egui::DragValue::new(&mut ch).range(1..=16)).changed() {
            setter.begin_set_parameter(&params.midi_channel);
            setter.set_parameter(&params.midi_channel, ch);
            setter.end_set_parameter(&params.midi_channel);
        }

        if ui.button("Fetch Voice").clicked() {
            let _ = cmd_tx.send(WorkerCmd::RequestVoiceDump);
        }

        if ui.button("Push All").clicked() {
            let _ = cmd_tx.send(WorkerCmd::PushVoiceBulk {
                vced: params.to_vced_bytes(&ui_st.voice_name),
                aced: params.to_aced_bytes(),
            });
        }
    });

    let cur_prog = *midi_state.current_program.lock().unwrap();
    if let Some(p) = cur_prog {
        ui_st.sel_bank = (p / 32).min(3);
        ui_st.sel_slot = (p % 32) + 1;
    }
    let prog_label = match cur_prog {
        Some(p) => {
            let bank = match p / 32 {
                0 => "I",
                1 => "A",
                2 => "B",
                _ => "C",
            };
            format!("{}{:02}", bank, (p % 32) + 1)
        }
        None => "--".to_string(),
    };
    ui.horizontal(|ui| {
        ui.label("Patch:");
        for (b_idx, b_name) in [(0u8, "I"), (1, "A"), (2, "B"), (3, "C")] {
            if ui
                .selectable_label(ui_st.sel_bank == b_idx, b_name)
                .clicked()
            {
                ui_st.sel_bank = b_idx;
                let p = ui_st.sel_bank * 32 + (ui_st.sel_slot - 1);
                let _ = cmd_tx.send(WorkerCmd::SelectProgram(p));
            }
        }
        if ui.button("<").clicked() {
            let cur = ui_st.sel_bank * 32 + (ui_st.sel_slot - 1);
            let next = cur.saturating_sub(1);
            ui_st.sel_bank = next / 32;
            ui_st.sel_slot = (next % 32) + 1;
            let _ = cmd_tx.send(WorkerCmd::SelectProgram(next));
        }
        let mut slot = ui_st.sel_slot;
        if ui.add(egui::DragValue::new(&mut slot).range(1..=32)).changed() {
            ui_st.sel_slot = slot;
            let p = ui_st.sel_bank * 32 + (ui_st.sel_slot - 1);
            let _ = cmd_tx.send(WorkerCmd::SelectProgram(p));
        }
        if ui.button(">").clicked() {
            let cur = ui_st.sel_bank * 32 + (ui_st.sel_slot - 1);
            let next = (cur + 1).min(127);
            ui_st.sel_bank = next / 32;
            ui_st.sel_slot = (next % 32) + 1;
            let _ = cmd_tx.send(WorkerCmd::SelectProgram(next));
        }
        ui.separator();
        ui.label(
            RichText::new(format!("Editing: [{}] {}", prog_label, ui_st.voice_name))
                .strong()
                .color(Color32::from_rgb(255, 220, 130)),
        );
    });
}

fn draw_operators_tab(
    ui: &mut egui::Ui,
    params: &Tx81zParams,
    ui_st: &mut EditorUiState,
    setter: &ParamSetter,
) {
    let alg = (params.algorithm.value() as usize).min(7);
    let op_ons = [&params.op1_on, &params.op2_on, &params.op3_on, &params.op4_on];
    let op_on_vals = [
        params.op1_on.value(),
        params.op2_on.value(),
        params.op3_on.value(),
        params.op4_on.value(),
    ];

    ui.horizontal(|ui| {
        draw_algorithm_diagram(ui, alg, params.feedback.value(), op_on_vals);

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Voice Name:");
                let resp =
                    ui.add(egui::TextEdit::singleline(&mut ui_st.voice_name).desired_width(110.0));
                if resp.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape))
                {
                    resp.surrender_focus();
                }
                ui.label(RichText::new(format!("Alg {}", ALG_DESCS[alg])).strong());
            });

            ui.horizontal(|ui| {
                ui.label("Algorithm:");
                for a in 0..8 {
                    if ui
                        .selectable_label(alg == a, format!("{}", a + 1))
                        .clicked()
                    {
                        setter.begin_set_parameter(&params.algorithm);
                        setter.set_parameter(&params.algorithm, a as i32);
                        setter.end_set_parameter(&params.algorithm);
                    }
                }
            });

            int_row(ui, "OP4 Feedback", &params.feedback, 0..=7, setter);
        });
    });
    ui.separator();

    ui.columns(4, |cols| {
        for (i, op) in params.operators().iter().enumerate() {
            let ui = &mut cols[i];
            let carrier = is_carrier(alg, i + 1);
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.heading(format!("OP {}", i + 1));
                    let badge = if carrier {
                        RichText::new("CARRIER").small().strong().color(Color32::from_rgb(90, 215, 250))
                    } else {
                        RichText::new("MOD").small().strong().color(Color32::from_rgb(245, 175, 85))
                    };
                    ui.label(badge);
                    bool_row(ui, "On", op_ons[i], setter);
                });

                draw_eg_graph(
                    ui,
                    op.ar.value(),
                    op.d1r.value(),
                    op.d1l.value(),
                    op.d2r.value(),
                    op.rr.value(),
                    carrier,
                );

                ui.horizontal(|ui| {
                    draw_waveform_preview(ui, op.wave.value());
                    int_row(ui, "Wave", &op.wave, 0..=7, setter);
                });

                int_row(ui, "Out Level", &op.out, 0..=99, setter);
                bool_row(ui, "Fixed Freq", &op.fix, setter);
                if op.fix.value() {
                    int_row(ui, "Fix Range", &op.fix_rng, 0..=7, setter);
                } else {
                    int_row(ui, "Coarse", &op.crs, 0..=63, setter);
                }
                int_row(ui, "Fine", &op.fine, 0..=15, setter);
                int_row(ui, "Detune", &op.det, 0..=6, setter);
                ui.separator();
                int_row(ui, "AR", &op.ar, 0..=31, setter);
                int_row(ui, "D1R", &op.d1r, 0..=31, setter);
                int_row(ui, "D2R", &op.d2r, 0..=31, setter);
                int_row(ui, "RR", &op.rr, 0..=15, setter);
                int_row(ui, "D1L", &op.d1l, 0..=15, setter);
                int_row(ui, "EG Shift", &op.eg_shift, 0..=3, setter);
                ui.separator();
                int_row(ui, "Level Scale", &op.ls, 0..=99, setter);
                int_row(ui, "Rate Scale", &op.rs, 0..=3, setter);
                int_row(ui, "EG Bias", &op.ebs, 0..=7, setter);
                int_row(ui, "Vel Sens", &op.kvs, 0..=7, setter);
                bool_row(ui, "AM Enable", &op.ame, setter);
            });
        }
    });
}

fn transpose_note_name(val: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let midi = (val + 36).clamp(0, 127) as usize; // 24 -> 60 (C3)
    format!("{}{}", NAMES[midi % 12], (midi / 12) as i32 - 2)
}

fn draw_voice_tab(ui: &mut egui::Ui, params: &Tx81zParams, setter: &ParamSetter) {
    ui.columns(3, |cols| {
        cols[0].group(|ui| {
            ui.heading("Voice & Pitch");
            let trans = params.transpose.value();
            ui.horizontal(|ui| {
                int_row(ui, "Transpose", &params.transpose, 0..=48, setter);
                if ui
                    .button(transpose_note_name(trans))
                    .on_hover_text("Click to reset to C3 (24)")
                    .clicked()
                {
                    setter.begin_set_parameter(&params.transpose);
                    setter.set_parameter(&params.transpose, 24);
                    setter.end_set_parameter(&params.transpose);
                }
            });
            int_row(ui, "Pitch Bend", &params.pitch_bend_range, 0..=12, setter);

            ui.horizontal(|ui| {
                ui.label("Key Mode:");
                let mono = params.mono_mode.value();
                if ui.selectable_label(!mono, "Poly").clicked() {
                    setter.begin_set_parameter(&params.mono_mode);
                    setter.set_parameter(&params.mono_mode, false);
                    setter.end_set_parameter(&params.mono_mode);
                }
                if ui.selectable_label(mono, "Mono").clicked() {
                    setter.begin_set_parameter(&params.mono_mode);
                    setter.set_parameter(&params.mono_mode, true);
                    setter.end_set_parameter(&params.mono_mode);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Portamento:");
                let fing = params.portamento_mode.value();
                if ui.selectable_label(!fing, "Full Time").clicked() {
                    setter.begin_set_parameter(&params.portamento_mode);
                    setter.set_parameter(&params.portamento_mode, false);
                    setter.end_set_parameter(&params.portamento_mode);
                }
                if ui.selectable_label(fing, "Fingered").clicked() {
                    setter.begin_set_parameter(&params.portamento_mode);
                    setter.set_parameter(&params.portamento_mode, true);
                    setter.end_set_parameter(&params.portamento_mode);
                }
            });

            int_row(ui, "Porta Time", &params.portamento_time, 0..=99, setter);
            int_row(ui, "Reverb Rate", &params.reverb_rate, 0..=7, setter);
        });

        cols[1].group(|ui| {
            ui.heading("LFO");
            draw_lfo_preview(
                ui,
                params.lfo_wave.value(),
                params.lfo_speed.value(),
                params.lfo_delay.value(),
                params.pmd.value(),
                params.amd.value(),
            );

            ui.horizontal(|ui| {
                ui.label("Wave:");
                let cur_w = params.lfo_wave.value();
                for (idx, name) in [(0, "Saw"), (1, "Sqr"), (2, "Tri"), (3, "S&H")] {
                    if ui.selectable_label(cur_w == idx, name).clicked() {
                        setter.begin_set_parameter(&params.lfo_wave);
                        setter.set_parameter(&params.lfo_wave, idx);
                        setter.end_set_parameter(&params.lfo_wave);
                    }
                }
            });

            int_row(ui, "Speed", &params.lfo_speed, 0..=99, setter);
            int_row(ui, "Delay", &params.lfo_delay, 0..=99, setter);
            int_row(ui, "Pitch Depth (PMD)", &params.pmd, 0..=99, setter);
            int_row(ui, "Amp Depth (AMD)", &params.amd, 0..=99, setter);
            bool_row(ui, "LFO Key Sync", &params.lfo_sync, setter);
            ui.separator();
            int_row(ui, "Pitch Mod Sens", &params.p_mod_sens, 0..=7, setter);
            int_row(ui, "Amp Mod Sens", &params.a_mod_sens, 0..=3, setter);
        });

        cols[2].group(|ui| {
            ui.heading("Controllers");
            ui.label(RichText::new("Mod Wheel").strong());
            int_row(ui, "MW Pitch", &params.mw_pitch, 0..=99, setter);
            int_row(ui, "MW Amp", &params.mw_amp, 0..=99, setter);
            ui.separator();

            ui.label(RichText::new("Foot Controller").strong());
            int_row(ui, "FC Volume", &params.fc_volume, 0..=99, setter);
            int_row(ui, "FC Pitch", &params.fc_pitch, 0..=99, setter);
            int_row(ui, "FC Amp", &params.fc_amp, 0..=99, setter);
            ui.separator();

            ui.label(RichText::new("Breath Controller").strong());
            int_row(ui, "BC Pitch", &params.bc_pitch, 0..=99, setter);
            int_row(ui, "BC Amp", &params.bc_amp, 0..=99, setter);
            int_row(ui, "BC Pitch Bias", &params.bc_pitch_bias, 0..=99, setter);
            int_row(ui, "BC EG Bias", &params.bc_eg_bias, 0..=99, setter);
        });
    });
}

fn draw_performance_tab(
    ui: &mut egui::Ui,
    cmd_tx: &crossbeam_channel::Sender<WorkerCmd>,
    ui_st: &mut EditorUiState,
) {
    ui.horizontal(|ui| {
        if ui.button("Fetch Perf").clicked() {
            let _ = cmd_tx.send(WorkerCmd::RequestPerfDump);
        }
        if ui.button("Push Perf").clicked() {
            let mut name_bytes = [b' '; 10];
            for (i, b) in ui_st.perf_name.bytes().take(10).enumerate() {
                name_bytes[i] = if (32..=126).contains(&b) { b } else { b' ' };
            }
            ui_st.pced[100..110].copy_from_slice(&name_bytes);
            let _ = cmd_tx.send(WorkerCmd::PushPerfBulk(ui_st.pced));
        }
        ui.label("Perf Name:");
        let resp = ui.add(egui::TextEdit::singleline(&mut ui_st.perf_name).desired_width(100.0));
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
            resp.surrender_focus();
        }

        let mut eff = ui_st.pced[98];
        ui.label("Effect (0=Off,1=Dly,2=Pan,3=Chd):");
        if ui.add(egui::DragValue::new(&mut eff).range(0..=3)).changed() {
            ui_st.pced[98] = eff;
        }
        let mut mic_tbl = ui_st.pced[96];
        ui.label("MicroTbl:");
        if ui
            .add(egui::DragValue::new(&mut mic_tbl).range(0..=12))
            .changed()
        {
            ui_st.pced[96] = mic_tbl;
        }
    });
    ui.separator();

    for row in 0..2 {
        ui.columns(4, |cols| {
            for col in 0..4 {
                let idx = row * 4 + col;
                let b = idx * 12;
                let ui = &mut cols[col];
                ui.group(|ui| {
                    ui.heading(format!("Inst {}", idx + 1));
                    pced_u8_row(ui, "Max Notes", &mut ui_st.pced[b], 0..=8);
                    pced_u8_row(ui, "Bank MSB (0=I,1-4=A-D)", &mut ui_st.pced[b + 1], 0..=4);
                    pced_u8_row(ui, "Voice (0-31)", &mut ui_st.pced[b + 2], 0..=127);
                    pced_u8_row(ui, "Rcv Ch (16=Omni)", &mut ui_st.pced[b + 3], 0..=16);
                    pced_u8_row(ui, "Low Note", &mut ui_st.pced[b + 4], 0..=127);
                    pced_u8_row(ui, "High Note", &mut ui_st.pced[b + 5], 0..=127);
                    pced_u8_row(ui, "Detune (7=0)", &mut ui_st.pced[b + 6], 0..=14);
                    pced_u8_row(ui, "Note Shift (24=0)", &mut ui_st.pced[b + 7], 0..=48);
                    pced_u8_row(ui, "Volume", &mut ui_st.pced[b + 8], 0..=99);
                    pced_u8_row(ui, "Out (0-3)", &mut ui_st.pced[b + 9], 0..=3);
                    pced_u8_row(ui, "LFO Sel (0-3)", &mut ui_st.pced[b + 10], 0..=3);
                    let mut mt = ui_st.pced[b + 11] != 0;
                    if ui.checkbox(&mut mt, "Micro Tune").changed() {
                        ui_st.pced[b + 11] = mt as u8;
                    }
                });
            }
        });
    }
}

fn pced_u8_row(ui: &mut egui::Ui, label: &str, val: &mut u8, range: std::ops::RangeInclusive<u8>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(val).range(range));
    });
}

fn draw_patches_tab(
    ui: &mut egui::Ui,
    params: &Tx81zParams,
    midi_state: &SharedMidiState,
    cmd_tx: &crossbeam_channel::Sender<WorkerCmd>,
    ui_st: &mut EditorUiState,
    setter: &ParamSetter,
) {
    let dir = EditorUiState::patch_dir();
    ui.label(format!("Patch Folder: {}", dir.display()));

    ui.horizontal(|ui| {
        ui.label("Filename:");
        let resp = ui.add(egui::TextEdit::singleline(&mut ui_st.patch_filename).desired_width(180.0));
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
            resp.surrender_focus();
        }

        if ui.button("Save Voice .syx").clicked() {
            let ch = (params.midi_channel.value() - 1).clamp(0, 15) as u8;
            let mut data = build_vced_bulk_dump(ch, &params.to_vced_bytes(&ui_st.voice_name));
            data.extend_from_slice(&build_aced_bulk_dump(ch, &params.to_aced_bytes()));
            let path = dir.join(&ui_st.patch_filename);
            ui_st.patch_status = match fs::write(&path, &data) {
                Ok(_) => {
                    ui_st.refresh_patch_list();
                    format!("Saved Voice: {}", path.display())
                }
                Err(e) => format!("Save error: {}", e),
            };
        }

        if ui.button("Save Perf .syx").clicked() {
            let ch = (params.midi_channel.value() - 1).clamp(0, 15) as u8;
            let data = build_pced_bulk_dump(ch, &ui_st.pced);
            let path = dir.join(&ui_st.patch_filename);
            ui_st.patch_status = match fs::write(&path, &data) {
                Ok(_) => {
                    ui_st.refresh_patch_list();
                    format!("Saved Perf: {}", path.display())
                }
                Err(e) => format!("Save error: {}", e),
            };
        }

        if ui.button("Refresh List").clicked() {
            ui_st.refresh_patch_list();
        }
    });

    ui.separator();
    let (cap_count, cap_bytes) = {
        let dumps = midi_state.captured_system_dumps.lock().unwrap();
        let bytes: usize = dumps.iter().map(|d| d.len()).sum();
        (dumps.len(), bytes)
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!(
            "SysEx Recorder (Captured: {} msg(s), {} bytes):",
            cap_count, cap_bytes
        )).strong());
        if ui.button("Clear Capture").clicked() {
            midi_state.captured_system_dumps.lock().unwrap().clear();
            ui_st.patch_status = "Capture buffer cleared. Trigger TRANS? (ALL) on TX81Z now.".to_string();
        }
        if ui.button("Save Capture .syx").clicked() {
            let dumps = midi_state.captured_system_dumps.lock().unwrap();
            if dumps.is_empty() {
                ui_st.patch_status = "Nothing captured yet! Trigger TRANS? on TX81Z first.".to_string();
            } else {
                let mut combined = Vec::new();
                for d in dumps.iter() {
                    combined.extend_from_slice(d);
                }
                let path = dir.join(&ui_st.patch_filename);
                ui_st.patch_status = match fs::write(&path, &combined) {
                    Ok(_) => {
                        ui_st.refresh_patch_list();
                        format!("Saved {} msg(s) ({} bytes) to {}", dumps.len(), combined.len(), path.display())
                    }
                    Err(e) => format!("Save error: {}", e),
                };
            }
        }
        if ui.button("Request S0+S1+S2").clicked() {
            let _ = cmd_tx.send(WorkerCmd::RequestSystemDump);
            ui_st.patch_status = "Requested S0 + S1 + S2 from TX81Z...".to_string();
        }
    });

    if !ui_st.patch_status.is_empty() {
        ui.label(RichText::new(&ui_st.patch_status).color(Color32::LIGHT_GREEN));
    }

    ui.separator();
    ui.heading("Saved .syx Files (Click to Load & Send to TX81Z)");
    for fname in ui_st.patch_files.clone() {
        if ui.button(&fname).clicked() {
            let path = dir.join(&fname);
            match fs::read(&path) {
                Ok(bytes) => {
                    let msgs = split_syx_messages(&bytes);
                    for m in &msgs {
                        if let Some(parsed) = crate::sysex::parse_sysex_dump(m) {
                            match parsed {
                                ParsedDump::Vced(b) => {
                                    apply_vced(params, &b, midi_state, ui_st, setter)
                                }
                                ParsedDump::Aced(b) => {
                                    apply_aced(params, &b, midi_state, setter)
                                }
                                ParsedDump::Pced(b) => apply_pced(&b, ui_st),
                                ParsedDump::Other(_) => {}
                            }
                        }
                    }
                    let count = msgs.len();
                    let _ = cmd_tx.send(WorkerCmd::SendRawSysex(msgs));
                    ui_st.patch_status = format!("Loaded & sent {} SysEx msg(s) from {}", count, fname);
                }
                Err(e) => ui_st.patch_status = format!("Read error: {}", e),
            }
        }
    }

    ui.separator();
    ui.heading("TX81Z Remote Front Panel");
    ui.horizontal(|ui| {
        for (label, sw) in [
            ("PLAY", 67u8),
            ("EDIT", 66u8),
            ("UTILITY", 65u8),
            ("STORE", 64u8),
            ("PARAM -", 68u8),
            ("PARAM +", 69u8),
            ("DATA -", 70u8),
            ("DATA +", 71u8),
            ("CURSOR", 72u8),
        ] {
            if ui.button(label).clicked() {
                let _ = cmd_tx.send(WorkerCmd::SendRemoteSwitch(sw));
            }
        }
    });
}

fn draw_log_tab(ui: &mut egui::Ui, midi_state: &SharedMidiState) {
    let status = midi_state.status.lock().unwrap().clone();
    let all_text = {
        let logs = midi_state.logs.lock().unwrap();
        logs.iter().cloned().collect::<Vec<_>>().join("\n")
    };

    ui.horizontal(|ui| {
        ui.label(format!("Status: {}", status));
        if ui.button("Copy All").clicked() {
            ui.ctx().copy_text(all_text.clone());
        }
        if ui.button("Clear").clicked() {
            if let Ok(mut logs) = midi_state.logs.lock() {
                logs.clear();
            }
        }
    });
    ui.separator();

    let logs = midi_state.logs.lock().unwrap();
    for line in logs.iter().rev() {
        ui.monospace(line);
    }
}

fn int_row(
    ui: &mut egui::Ui,
    label: &str,
    param: &IntParam,
    range: std::ops::RangeInclusive<i32>,
    setter: &ParamSetter,
) {
    let mut val = param.value();
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.add(egui::Slider::new(&mut val, range)).changed() {
            setter.begin_set_parameter(param);
            setter.set_parameter(param, val);
            setter.end_set_parameter(param);
        }
    });
}

fn bool_row(ui: &mut egui::Ui, label: &str, param: &BoolParam, setter: &ParamSetter) {
    let mut val = param.value();
    if ui.checkbox(&mut val, label).changed() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, val);
        setter.end_set_parameter(param);
    }
}

fn apply_incoming_dumps(
    params: &Tx81zParams,
    midi_state: &SharedMidiState,
    ui_st: &mut EditorUiState,
    setter: &ParamSetter,
) {
    let Ok(mut q) = midi_state.incoming_dumps.lock() else {
        return;
    };
    while let Some(dump) = q.pop_front() {
        match dump {
            ParsedDump::Vced(buf) => apply_vced(params, &buf, midi_state, ui_st, setter),
            ParsedDump::Aced(buf) => apply_aced(params, &buf, midi_state, setter),
            ParsedDump::Pced(buf) => apply_pced(&buf, ui_st),
            ParsedDump::Other(_) => {}
        }
    }
}

fn set_i(setter: &ParamSetter, p: &IntParam, v: u8) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v as i32);
    setter.end_set_parameter(p);
}

fn set_b(setter: &ParamSetter, p: &BoolParam, v: u8) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v != 0);
    setter.end_set_parameter(p);
}

fn apply_vced(
    params: &Tx81zParams,
    buf: &[u8; 93],
    midi_state: &SharedMidiState,
    ui_st: &mut EditorUiState,
    setter: &ParamSetter,
) {
    midi_state.suppress_tx_for(350);
    let ops: [&OperatorParams; 4] = params.operators();
    for (idx, op) in ops.iter().enumerate() {
        let b = OP_VCED_BASE[idx] as usize;
        set_i(setter, &op.ar, buf[b]);
        set_i(setter, &op.d1r, buf[b + 1]);
        set_i(setter, &op.d2r, buf[b + 2]);
        set_i(setter, &op.rr, buf[b + 3]);
        set_i(setter, &op.d1l, buf[b + 4]);
        set_i(setter, &op.ls, buf[b + 5]);
        set_i(setter, &op.rs, buf[b + 6]);
        set_i(setter, &op.ebs, buf[b + 7]);
        set_b(setter, &op.ame, buf[b + 8]);
        set_i(setter, &op.kvs, buf[b + 9]);
        set_i(setter, &op.out, buf[b + 10]);
        set_i(setter, &op.crs, buf[b + 11]);
        set_i(setter, &op.det, buf[b + 12]);
    }
    set_i(setter, &params.algorithm, buf[52]);
    set_i(setter, &params.feedback, buf[53]);
    set_i(setter, &params.lfo_speed, buf[54]);
    set_i(setter, &params.lfo_delay, buf[55]);
    set_i(setter, &params.pmd, buf[56]);
    set_i(setter, &params.amd, buf[57]);
    set_b(setter, &params.lfo_sync, buf[58]);
    set_i(setter, &params.lfo_wave, buf[59]);
    set_i(setter, &params.p_mod_sens, buf[60]);
    set_i(setter, &params.a_mod_sens, buf[61]);
    set_i(setter, &params.transpose, buf[62]);
    set_b(setter, &params.mono_mode, buf[63]);
    set_i(setter, &params.pitch_bend_range, buf[64]);
    set_b(setter, &params.portamento_mode, buf[65]);
    set_i(setter, &params.portamento_time, buf[66]);
    set_i(setter, &params.fc_volume, buf[67]);
    set_i(setter, &params.mw_pitch, buf[71]);
    set_i(setter, &params.mw_amp, buf[72]);
    set_i(setter, &params.bc_pitch, buf[73]);
    set_i(setter, &params.bc_amp, buf[74]);
    set_i(setter, &params.bc_pitch_bias, buf[75]);
    set_i(setter, &params.bc_eg_bias, buf[76]);

    ui_st.voice_name = String::from_utf8_lossy(&buf[77..87]).trim_end().to_string();
}

fn apply_aced(
    params: &Tx81zParams,
    buf: &[u8; 23],
    midi_state: &SharedMidiState,
    setter: &ParamSetter,
) {
    midi_state.suppress_tx_for(350);
    let ops: [&OperatorParams; 4] = params.operators();
    for (idx, op) in ops.iter().enumerate() {
        let b = OP_ACED_BASE[idx] as usize;
        set_b(setter, &op.fix, buf[b]);
        set_i(setter, &op.fix_rng, buf[b + 1]);
        set_i(setter, &op.fine, buf[b + 2]);
        set_i(setter, &op.wave, buf[b + 3]);
        set_i(setter, &op.eg_shift, buf[b + 4]);
    }
    set_i(setter, &params.reverb_rate, buf[20]);
    set_i(setter, &params.fc_pitch, buf[21]);
    set_i(setter, &params.fc_amp, buf[22]);
}

fn apply_pced(buf: &[u8; 110], ui_st: &mut EditorUiState) {
    ui_st.pced = *buf;
    ui_st.perf_name = String::from_utf8_lossy(&buf[100..110])
        .trim_end()
        .to_string();
}
