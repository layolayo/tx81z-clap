use nih_plug::prelude::*;
use crate::sysex::Subgroup;

/// VCED byte base offsets for [OP1, OP2, OP3, OP4].
/// Note: TX81Z stores operators in VCED in order OP4 (0), OP2 (13), OP3 (26), OP1 (39).
pub const OP_VCED_BASE: [u8; 4] = [39, 13, 26, 0];

/// ACED byte base offsets for [OP1, OP2, OP3, OP4].
/// Note: TX81Z stores operators in ACED in order OP4 (0), OP2 (5), OP3 (10), OP1 (15).
pub const OP_ACED_BASE: [u8; 4] = [15, 5, 10, 0];

#[derive(Params)]
pub struct OperatorParams {
    #[id = "ar"]
    pub ar: IntParam,
    #[id = "d1r"]
    pub d1r: IntParam,
    #[id = "d2r"]
    pub d2r: IntParam,
    #[id = "rr"]
    pub rr: IntParam,
    #[id = "d1l"]
    pub d1l: IntParam,
    #[id = "ls"]
    pub ls: IntParam,
    #[id = "rs"]
    pub rs: IntParam,
    #[id = "ebs"]
    pub ebs: IntParam,
    #[id = "ame"]
    pub ame: BoolParam,
    #[id = "kvs"]
    pub kvs: IntParam,
    #[id = "out"]
    pub out: IntParam,
    #[id = "crs"]
    pub crs: IntParam,
    #[id = "det"]
    pub det: IntParam,

    // ACED per-operator parameters
    #[id = "fix"]
    pub fix: BoolParam,
    #[id = "fix_rng"]
    pub fix_rng: IntParam,
    #[id = "fine"]
    pub fine: IntParam,
    #[id = "wave"]
    pub wave: IntParam,
    #[id = "eg_shift"]
    pub eg_shift: IntParam,
}

impl OperatorParams {
    pub fn new(op_num: u8, default_out: i32) -> Self {
        let p = |suffix: &str| format!("OP{} {}", op_num, suffix);
        Self {
            ar: IntParam::new(p("AR"), 31, IntRange::Linear { min: 0, max: 31 }),
            d1r: IntParam::new(p("D1R"), 0, IntRange::Linear { min: 0, max: 31 }),
            d2r: IntParam::new(p("D2R"), 0, IntRange::Linear { min: 0, max: 31 }),
            rr: IntParam::new(p("RR"), 7, IntRange::Linear { min: 0, max: 15 }),
            d1l: IntParam::new(p("D1L"), 15, IntRange::Linear { min: 0, max: 15 }),
            ls: IntParam::new(p("LS"), 0, IntRange::Linear { min: 0, max: 99 }),
            rs: IntParam::new(p("RS"), 0, IntRange::Linear { min: 0, max: 3 }),
            ebs: IntParam::new(p("EBS"), 0, IntRange::Linear { min: 0, max: 7 }),
            ame: BoolParam::new(p("AME"), false),
            kvs: IntParam::new(p("KVS"), 0, IntRange::Linear { min: 0, max: 7 }),
            out: IntParam::new(p("Out"), default_out, IntRange::Linear { min: 0, max: 99 }),
            crs: IntParam::new(p("Coarse"), 1, IntRange::Linear { min: 0, max: 63 }),
            det: IntParam::new(p("Detune"), 3, IntRange::Linear { min: 0, max: 6 }),
            fix: BoolParam::new(p("Fixed Freq"), false),
            fix_rng: IntParam::new(p("Fix Range"), 0, IntRange::Linear { min: 0, max: 7 }),
            fine: IntParam::new(p("Fine"), 0, IntRange::Linear { min: 0, max: 15 }),
            wave: IntParam::new(p("Wave"), 0, IntRange::Linear { min: 0, max: 7 }),
            eg_shift: IntParam::new(p("EG Shift"), 0, IntRange::Linear { min: 0, max: 3 }),
        }
    }

    /// Return `(Subgroup, param_number, value)` pairs for this operator.
    pub fn sysex_values(&self, op_idx: usize) -> [(Subgroup, u8, u8); 18] {
        let vb = OP_VCED_BASE[op_idx];
        let ab = OP_ACED_BASE[op_idx];
        [
            (Subgroup::Vced, vb, self.ar.value() as u8),
            (Subgroup::Vced, vb + 1, self.d1r.value() as u8),
            (Subgroup::Vced, vb + 2, self.d2r.value() as u8),
            (Subgroup::Vced, vb + 3, self.rr.value() as u8),
            (Subgroup::Vced, vb + 4, self.d1l.value() as u8),
            (Subgroup::Vced, vb + 5, self.ls.value() as u8),
            (Subgroup::Vced, vb + 6, self.rs.value() as u8),
            (Subgroup::Vced, vb + 7, self.ebs.value() as u8),
            (Subgroup::Vced, vb + 8, self.ame.value() as u8),
            (Subgroup::Vced, vb + 9, self.kvs.value() as u8),
            (Subgroup::Vced, vb + 10, self.out.value() as u8),
            (Subgroup::Vced, vb + 11, self.crs.value() as u8),
            (Subgroup::Vced, vb + 12, self.det.value() as u8),
            (Subgroup::Aced, ab, self.fix.value() as u8),
            (Subgroup::Aced, ab + 1, self.fix_rng.value() as u8),
            (Subgroup::Aced, ab + 2, self.fine.value() as u8),
            (Subgroup::Aced, ab + 3, self.wave.value() as u8),
            (Subgroup::Aced, ab + 4, self.eg_shift.value() as u8),
        ]
    }
}

#[derive(Params)]
pub struct Tx81zParams {
    #[persist = "midi_out"]
    pub saved_midi_out: std::sync::Mutex<String>,
    #[persist = "midi_in"]
    pub saved_midi_in: std::sync::Mutex<String>,

    #[id = "midi_ch"]
    pub midi_channel: IntParam,
    #[id = "op1_on"]
    pub op1_on: BoolParam,
    #[id = "op2_on"]
    pub op2_on: BoolParam,
    #[id = "op3_on"]
    pub op3_on: BoolParam,
    #[id = "op4_on"]
    pub op4_on: BoolParam,

    // Global Voice VCED (52..76)
    #[id = "alg"]
    pub algorithm: IntParam,
    #[id = "fb"]
    pub feedback: IntParam,
    #[id = "lfo_spd"]
    pub lfo_speed: IntParam,
    #[id = "lfo_dly"]
    pub lfo_delay: IntParam,
    #[id = "pmd"]
    pub pmd: IntParam,
    #[id = "amd"]
    pub amd: IntParam,
    #[id = "lfo_sync"]
    pub lfo_sync: BoolParam,
    #[id = "lfo_wave"]
    pub lfo_wave: IntParam,
    #[id = "pms"]
    pub p_mod_sens: IntParam,
    #[id = "ams"]
    pub a_mod_sens: IntParam,
    #[id = "trans"]
    pub transpose: IntParam,
    #[id = "mono"]
    pub mono_mode: BoolParam,
    #[id = "pb_rng"]
    pub pitch_bend_range: IntParam,
    #[id = "porta_m"]
    pub portamento_mode: BoolParam,
    #[id = "porta_t"]
    pub portamento_time: IntParam,
    #[id = "fc_vol"]
    pub fc_volume: IntParam,
    #[id = "mw_p"]
    pub mw_pitch: IntParam,
    #[id = "mw_a"]
    pub mw_amp: IntParam,
    #[id = "bc_p"]
    pub bc_pitch: IntParam,
    #[id = "bc_a"]
    pub bc_amp: IntParam,
    #[id = "bc_pb"]
    pub bc_pitch_bias: IntParam,
    #[id = "bc_eb"]
    pub bc_eg_bias: IntParam,

    // Global ACED (20..22)
    #[id = "rev_rate"]
    pub reverb_rate: IntParam,
    #[id = "fc_p"]
    pub fc_pitch: IntParam,
    #[id = "fc_a"]
    pub fc_amp: IntParam,

    #[nested(id_prefix = "op1", group = "Operator 1")]
    pub op1: OperatorParams,
    #[nested(id_prefix = "op2", group = "Operator 2")]
    pub op2: OperatorParams,
    #[nested(id_prefix = "op3", group = "Operator 3")]
    pub op3: OperatorParams,
    #[nested(id_prefix = "op4", group = "Operator 4")]
    pub op4: OperatorParams,
}

impl Default for Tx81zParams {
    fn default() -> Self {
        Self {
            saved_midi_out: std::sync::Mutex::new(String::new()),
            saved_midi_in: std::sync::Mutex::new(String::new()),
            midi_channel: IntParam::new("MIDI Channel", 1, IntRange::Linear { min: 1, max: 16 }),
            op1_on: BoolParam::new("OP1 On", true),
            op2_on: BoolParam::new("OP2 On", true),
            op3_on: BoolParam::new("OP3 On", true),
            op4_on: BoolParam::new("OP4 On", true),
            algorithm: IntParam::new("Algorithm", 0, IntRange::Linear { min: 0, max: 7 }),
            feedback: IntParam::new("Feedback", 0, IntRange::Linear { min: 0, max: 7 }),
            lfo_speed: IntParam::new("LFO Speed", 35, IntRange::Linear { min: 0, max: 99 }),
            lfo_delay: IntParam::new("LFO Delay", 0, IntRange::Linear { min: 0, max: 99 }),
            pmd: IntParam::new("LFO PMD", 0, IntRange::Linear { min: 0, max: 99 }),
            amd: IntParam::new("LFO AMD", 0, IntRange::Linear { min: 0, max: 99 }),
            lfo_sync: BoolParam::new("LFO Sync", false),
            lfo_wave: IntParam::new("LFO Wave", 2, IntRange::Linear { min: 0, max: 3 }),
            p_mod_sens: IntParam::new("P Mod Sens", 3, IntRange::Linear { min: 0, max: 7 }),
            a_mod_sens: IntParam::new("A Mod Sens", 0, IntRange::Linear { min: 0, max: 3 }),
            transpose: IntParam::new("Transpose", 24, IntRange::Linear { min: 0, max: 48 }),
            mono_mode: BoolParam::new("Mono Mode", false),
            pitch_bend_range: IntParam::new("Pitch Bend", 2, IntRange::Linear { min: 0, max: 12 }),
            portamento_mode: BoolParam::new("Porta Fingered", false),
            portamento_time: IntParam::new("Porta Time", 0, IntRange::Linear { min: 0, max: 99 }),
            fc_volume: IntParam::new("FC Volume", 0, IntRange::Linear { min: 0, max: 99 }),
            mw_pitch: IntParam::new("MW Pitch", 0, IntRange::Linear { min: 0, max: 99 }),
            mw_amp: IntParam::new("MW Amp", 0, IntRange::Linear { min: 0, max: 99 }),
            bc_pitch: IntParam::new("BC Pitch", 0, IntRange::Linear { min: 0, max: 99 }),
            bc_amp: IntParam::new("BC Amp", 0, IntRange::Linear { min: 0, max: 99 }),
            bc_pitch_bias: IntParam::new("BC P Bias", 50, IntRange::Linear { min: 0, max: 99 }),
            bc_eg_bias: IntParam::new("BC EG Bias", 0, IntRange::Linear { min: 0, max: 99 }),
            reverb_rate: IntParam::new("Reverb Rate", 0, IntRange::Linear { min: 0, max: 7 }),
            fc_pitch: IntParam::new("FC Pitch", 0, IntRange::Linear { min: 0, max: 99 }),
            fc_amp: IntParam::new("FC Amp", 0, IntRange::Linear { min: 0, max: 99 }),
            op1: OperatorParams::new(1, 90),
            op2: OperatorParams::new(2, 0),
            op3: OperatorParams::new(3, 0),
            op4: OperatorParams::new(4, 0),
        }
    }
}

impl Tx81zParams {
    pub fn operators(&self) -> [&OperatorParams; 4] {
        [&self.op1, &self.op2, &self.op3, &self.op4]
    }

    pub fn op_enable_mask(&self) -> u8 {
        ((self.op4_on.value() as u8) & 1)
            | (((self.op3_on.value() as u8) & 1) << 1)
            | (((self.op2_on.value() as u8) & 1) << 2)
            | (((self.op1_on.value() as u8) & 1) << 3)
    }

    /// Collect all `(Subgroup, param_num, value)` tuples for the full Voice (VCED + ACED).
    pub fn all_sysex_values(&self) -> Vec<(Subgroup, u8, u8)> {
        let mut out = Vec::with_capacity(100);
        for (idx, op) in self.operators().iter().enumerate() {
            out.extend_from_slice(&op.sysex_values(idx));
        }
        out.extend_from_slice(&[
            (Subgroup::Vced, 52, self.algorithm.value() as u8),
            (Subgroup::Vced, 53, self.feedback.value() as u8),
            (Subgroup::Vced, 54, self.lfo_speed.value() as u8),
            (Subgroup::Vced, 55, self.lfo_delay.value() as u8),
            (Subgroup::Vced, 56, self.pmd.value() as u8),
            (Subgroup::Vced, 57, self.amd.value() as u8),
            (Subgroup::Vced, 58, self.lfo_sync.value() as u8),
            (Subgroup::Vced, 59, self.lfo_wave.value() as u8),
            (Subgroup::Vced, 60, self.p_mod_sens.value() as u8),
            (Subgroup::Vced, 61, self.a_mod_sens.value() as u8),
            (Subgroup::Vced, 62, self.transpose.value() as u8),
            (Subgroup::Vced, 63, self.mono_mode.value() as u8),
            (Subgroup::Vced, 64, self.pitch_bend_range.value() as u8),
            (Subgroup::Vced, 65, self.portamento_mode.value() as u8),
            (Subgroup::Vced, 66, self.portamento_time.value() as u8),
            (Subgroup::Vced, 67, self.fc_volume.value() as u8),
            (Subgroup::Vced, 71, self.mw_pitch.value() as u8),
            (Subgroup::Vced, 72, self.mw_amp.value() as u8),
            (Subgroup::Vced, 73, self.bc_pitch.value() as u8),
            (Subgroup::Vced, 74, self.bc_amp.value() as u8),
            (Subgroup::Vced, 75, self.bc_pitch_bias.value() as u8),
            (Subgroup::Vced, 76, self.bc_eg_bias.value() as u8),
            (Subgroup::Aced, 20, self.reverb_rate.value() as u8),
            (Subgroup::Aced, 21, self.fc_pitch.value() as u8),
            (Subgroup::Aced, 22, self.fc_amp.value() as u8),
        ]);
        out
    }

    /// Serialize all parameters into a 93-byte VCED bulk buffer.
    pub fn to_vced_bytes(&self, voice_name: &str) -> [u8; 93] {
        let mut buf = [0u8; 93];
        for (sub, p, v) in self.all_sysex_values() {
            if sub == Subgroup::Vced && (p as usize) < 93 {
                buf[p as usize] = v & 0x7F;
            }
        }
        // Voice Name (bytes 77..87, 10 ASCII chars padded with spaces)
        let mut name_bytes = [b' '; 10];
        for (i, b) in voice_name.bytes().take(10).enumerate() {
            name_bytes[i] = if (32..=126).contains(&b) { b } else { b' ' };
        }
        buf[77..87].copy_from_slice(&name_bytes);
        buf
    }

    /// Serialize all additional parameters into a 23-byte ACED bulk buffer.
    pub fn to_aced_bytes(&self) -> [u8; 23] {
        let mut buf = [0u8; 23];
        for (sub, p, v) in self.all_sysex_values() {
            if sub == Subgroup::Aced && (p as usize) < 23 {
                buf[p as usize] = v & 0x7F;
            }
        }
        buf
    }
}
