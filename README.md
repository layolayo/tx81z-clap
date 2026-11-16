# tx81z-clap

A Linux **CLAP** plugin for controlling and automating the **Yamaha TX81Z** FM synthesizer via MIDI SysEx (designed for Bitwig Studio and other CLAP hosts).

## Features
* **Full Single Voice Editor (`VCED` + `ACED`):**
  * Interactive **8-Algorithm FM Routing Diagram** with Carrier/Modulator badges, OP4 Feedback, and per-operator On/Off mutes
  * **4-Operator Cards** with live EG envelope graphs, `W1–W8` waveform previews, Ratio/Fixed frequency, Coarse/Fine/Detune, Level/Rate Scaling, Velocity/EG Bias, and AM Enable
  * **Voice / LFO / Controllers** tab with live dual-curve **PMD/AMD LFO preview**, Transpose (`C3` reset), Poly/Mono, Portamento, Reverb Rate, and Mod/Foot/Breath Controller routings
* **Performance Editor (`PCED`):** 8-instrument multi-timbral editor with **Fetch Perf** and **Push Perf**
* **Patches & Utility (`.syx`) Librarian:**
  * Save/load `.syx` files in `~/TX81Z_Patches/`
  * **SysEx Recorder** for capturing full `TRANS? (ALL)` hardware dumps (`VMEM`, `PMEM`, `S0`, `S1`, `S2`, etc.)
  * **Remote Front Panel** (`PLAY`, `EDIT`, `UTILITY`, `STORE`, `PARAM -/+`, `DATA -/+`, `CURSOR`)
* **Direct ALSA Sequencer MIDI I/O + Note Forwarding:** Bypasses DAW SysEx stripping, throttles SysEx (`45 ms` spacing / bulk push) to avoid `MIDI Buffer Full`, and forwards DAW Notes/CCs/PitchBend/Program Changes.

## Building on Linux

### Prerequisites
* Rust toolchain (`cargo`, `rustc`)
* ALSA development headers (`libasound2-dev` on Debian/Ubuntu) and `pkg-config`

### Build & Install
```bash
cargo xtask bundle tx81z-clap --release
cp -r target/bundled/tx81z-clap.clap ~/.clap/
```

## License
GPL-3.0-or-later
