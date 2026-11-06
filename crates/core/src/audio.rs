//! Som da sessão: captura o que o anfitrião está tocando, comprime em Opus
//! (48 kHz, estéreo, quadros de 20 ms) e toca do lado de quem acessa.
//!
//! Captura do "som que sai das caixas":
//! - Linux: `pw-record` (PipeWire) ou `parec` (PulseAudio), do monitor da saída padrão;
//! - Windows: WASAPI em modo loopback (cpal);
//! - macOS 14.2+: process tap do Core Audio (cpal; pede a permissão
//!   "Gravação de áudio do sistema").

use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use opus1::{Application, Bitrate, Channels, OpusDecoder, OpusEncoder, SampleRate};

/// Taxa e canais do que vai pela rede.
pub const RATE: u32 = 48_000;
/// Amostras por canal em cada pacote (20 ms).
pub const FRAME: usize = 960;
const BITRATE: u32 = 96_000;

// ── Reamostragem ────────────────────────────────────────────────────────

/// Muda a taxa de um som estéreo intercalado (interpolação linear: barata e
/// suficiente para 44,1 ↔ 48 kHz).
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: [f32; 2],
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, pos: 0.0, prev: [0.0; 2] }
    }

    pub fn same_rate(&self) -> bool {
        (self.step - 1.0).abs() < 1e-9
    }

    /// `input`: estéreo intercalado na taxa de origem. Anexa em `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.same_rate() {
            out.extend_from_slice(input);
            return;
        }
        let frames = input.len() / 2;
        // `pos` conta a partir do quadro anterior ao bloco (`prev` = índice -1).
        let at = |i: isize| -> [f32; 2] { if i < 0 { self.prev } else { [input[i as usize * 2], input[i as usize * 2 + 1]] } };
        while self.pos < frames as f64 {
            let i = self.pos.floor() as isize - 1;
            let t = (self.pos - self.pos.floor()) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.push(a[0] + (b[0] - a[0]) * t);
            out.push(a[1] + (b[1] - a[1]) * t);
            self.pos += self.step;
        }
        self.pos -= frames as f64;
        if frames > 0 {
            self.prev = at(frames as isize - 1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
/// Converte qualquer quantidade de canais para estéreo (mono duplica; mais
/// de dois, usa os dois primeiros).
fn to_stereo(data: &[f32], channels: usize, out: &mut Vec<f32>) {
    match channels {
        0 => {}
        1 => data.iter().for_each(|s| out.extend([*s, *s])),
        2 => out.extend_from_slice(data),
        n => data.chunks_exact(n).for_each(|f| out.extend([f[0], f[1]])),
    }
}

#[cfg(not(target_os = "linux"))]
fn to_f32<T: cpal::SizedSample>(d: &[T]) -> Vec<f32>
where
    f32: cpal::FromSample<T>,
{
    d.iter().map(|s| s.to_sample::<f32>()).collect()
}

// ── Codificação ─────────────────────────────────────────────────────────

/// Junta o som capturado em quadros de 20 ms e devolve os pacotes Opus.
pub struct Encoder {
    enc: OpusEncoder,
    pending: Vec<f32>,
}

impl Encoder {
    pub fn new() -> Result<Self, String> {
        let mut enc = OpusEncoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio).map_err(|e| format!("Opus: {e:?}"))?;
        enc.set_bitrate(Bitrate::BitsPerSecond(NonZeroU32::new(BITRATE).expect("taxa")));
        let _ = enc.set_complexity(5);
        Ok(Self { enc, pending: Vec::new() })
    }

    /// `pcm`: estéreo intercalado, 48 kHz.
    pub fn push(&mut self, pcm: &[f32]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(pcm);
        let mut packets = Vec::new();
        let mut used = 0;
        let mut buf = [0u8; 1500];
        while self.pending.len() - used >= FRAME * 2 {
            if let Ok(n) = self.enc.encode(&self.pending[used..used + FRAME * 2], FRAME, &mut buf) {
                packets.push(buf[..n].to_vec());
            }
            used += FRAME * 2;
        }
        self.pending.drain(..used);
        packets
    }
}

// ── Captura ─────────────────────────────────────────────────────────────

/// Som que o anfitrião está tocando, já em estéreo 48 kHz.
pub struct Capture {
    rx: std::sync::mpsc::Receiver<Vec<f32>>,
    #[cfg(target_os = "linux")]
    child: std::process::Child,
    #[cfg(not(target_os = "linux"))]
    _stream: cpal::Stream,
}

impl Capture {
    /// Tudo o que chegou desde a última leitura.
    pub fn read(&mut self, out: &mut Vec<f32>) {
        while let Ok(v) = self.rx.try_recv() {
            out.extend_from_slice(&v);
        }
    }

    #[cfg(target_os = "linux")]
    pub fn open() -> Result<Self, String> {
        use std::io::Read;
        use std::process::{Command, Stdio};
        // PipeWire (Ubuntu 22.10+, Fedora…) ou PulseAudio, do monitor da saída padrão.
        let tries: [(&str, &[&str]); 2] = [
            ("pw-record", &["-P", "{ stream.capture.sink=true node.name=elo-som }", "--format", "s16", "--rate", "48000", "--channels", "2", "-"]),
            ("parec", &["-d", "@DEFAULT_MONITOR@", "--format=s16le", "--rate=48000", "--channels=2", "--raw", "--client-name=Elo"]),
        ];
        let mut child = None;
        for (prog, args) in tries {
            if let Ok(c) = Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
                child = Some(c);
                break;
            }
        }
        let mut child = child.ok_or("não achei pw-record (PipeWire) nem parec (PulseAudio) neste computador")?;
        let mut out = child.stdout.take().ok_or("sem saída do gravador")?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("elo-som".into())
            .spawn(move || {
                let mut buf = vec![0u8; 3840];
                let mut carry: Option<u8> = None;
                loop {
                    let n = match out.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    let mut bytes: Vec<u8> = carry.take().into_iter().collect();
                    bytes.extend_from_slice(&buf[..n]);
                    if bytes.len() % 2 == 1 {
                        carry = bytes.pop();
                    }
                    let pcm: Vec<f32> = bytes.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect();
                    if tx.send(pcm).is_err() {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { rx, child })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn open() -> Result<Self, String> {
        // Fluxo de entrada sobre o dispositivo de SAÍDA: loopback (WASAPI) /
        // process tap (Core Audio).
        let device = cpal::default_host().default_output_device().ok_or("nenhuma saída de som")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let channels = config.channels() as usize;
        let rate = config.sample_rate();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
        let mut rs = Resampler::new(rate, RATE);
        let mut stereo = Vec::new();
        let mut deliver = move |data: &[f32]| {
            stereo.clear();
            to_stereo(data, channels, &mut stereo);
            let mut out = Vec::with_capacity(stereo.len() + 8);
            rs.process(&stereo, &mut out);
            let _ = tx.send(out);
        };
        let err = |e: cpal::Error| eprintln!("som: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(config.into(), move |d: &[f32], _: &cpal::InputCallbackInfo| deliver(d), err, None),
            cpal::SampleFormat::I16 => device.build_input_stream(config.into(), move |d: &[i16], _: &cpal::InputCallbackInfo| deliver(&to_f32(d)), err, None),
            cpal::SampleFormat::I32 => device.build_input_stream(config.into(), move |d: &[i32], _: &cpal::InputCallbackInfo| deliver(&to_f32(d)), err, None),
            f => return Err(format!("formato de som não suportado: {f}")),
        }
        .map_err(|e| {
            if cfg!(target_os = "macos") {
                format!("{e} (no Mac, precisa do macOS 14.2 ou mais novo e da permissão \"Gravação de áudio do sistema\" para o Elo)")
            } else {
                e.to_string()
            }
        })?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self { rx, _stream: stream })
    }
}

#[cfg(target_os = "linux")]
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ── Reprodução ──────────────────────────────────────────────────────────

/// Fila entre a rede e a placa de som. Espera juntar um pouco antes de tocar
/// (a rede chega em rajadas) e corta o atraso se acumular demais.
struct Jitter {
    buf: VecDeque<f32>,
    priming: bool,
    /// Amostras (já na taxa da placa, estéreo) para começar a tocar.
    start: usize,
    /// Acima disto, descarta o excesso (atraso demais).
    max: usize,
}

impl Jitter {
    fn push(&mut self, pcm: &[f32]) {
        self.buf.extend(pcm);
        if self.buf.len() > self.max {
            let extra = self.buf.len() - self.start;
            self.buf.drain(..extra & !1);
        }
    }

    fn pop(&mut self) -> Option<[f32; 2]> {
        if self.priming {
            if self.buf.len() < self.start {
                return None;
            }
            self.priming = false;
        }
        match (self.buf.pop_front(), self.buf.pop_front()) {
            (Some(l), Some(r)) => Some([l, r]),
            _ => {
                self.priming = true;
                None
            }
        }
    }
}

/// Preenche o buffer da placa com a fila (silêncio enquanto junta).
fn write_out<T: cpal::SizedSample + cpal::FromSample<f32>>(d: &mut [T], channels: usize, jitter: &Mutex<Jitter>, volume: &AtomicU32) {
    let gain = f32::from_bits(volume.load(Ordering::Relaxed));
    let mut j = jitter.lock().unwrap_or_else(|e| e.into_inner());
    for f in d.chunks_mut(channels.max(1)) {
        let s = j.pop().unwrap_or([0.0, 0.0]);
        for (c, x) in f.iter_mut().enumerate() {
            *x = T::from_sample((s[c % 2] * gain).clamp(-1.0, 1.0));
        }
    }
}

/// Toca os pacotes que chegam do anfitrião na saída de som padrão.
pub struct Player {
    _stream: cpal::Stream,
    jitter: Arc<Mutex<Jitter>>,
    dec: OpusDecoder,
    rs: Resampler,
    volume: Arc<AtomicU32>,
    pcm: Vec<f32>,
    out: Vec<f32>,
}

impl Player {
    pub fn open(volume: f32) -> Result<Self, String> {
        let device = cpal::default_host().default_output_device().ok_or("nenhuma saída de som neste computador")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let channels = config.channels() as usize;
        let rate = config.sample_rate();
        let per_ms = rate as usize * 2 / 1000;
        let jitter = Arc::new(Mutex::new(Jitter { buf: VecDeque::new(), priming: true, start: per_ms * 100, max: per_ms * 400 }));
        let vol = Arc::new(AtomicU32::new(volume.to_bits()));
        let (j, v) = (jitter.clone(), vol.clone());
        let err = |e: cpal::Error| eprintln!("som: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(config.into(), move |d: &mut [f32], _: &cpal::OutputCallbackInfo| write_out(d, channels, &j, &v), err, None),
            cpal::SampleFormat::I16 => device.build_output_stream(config.into(), move |d: &mut [i16], _: &cpal::OutputCallbackInfo| write_out(d, channels, &j, &v), err, None),
            cpal::SampleFormat::I32 => device.build_output_stream(config.into(), move |d: &mut [i32], _: &cpal::OutputCallbackInfo| write_out(d, channels, &j, &v), err, None),
            f => return Err(format!("formato de som não suportado: {f}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        let dec = OpusDecoder::new(SampleRate::Hz48000, Channels::Stereo).map_err(|e| format!("Opus: {e:?}"))?;
        Ok(Self { _stream: stream, jitter, dec, rs: Resampler::new(RATE, rate), volume: vol, pcm: vec![0.0; FRAME * 2 * 6], out: Vec::new() })
    }

    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.clamp(0.0, 1.5).to_bits(), Ordering::Relaxed);
    }

    /// Um pacote Opus vindo do anfitrião.
    pub fn push(&mut self, packet: &[u8]) {
        let Ok(n) = self.dec.decode(packet, FRAME * 6, &mut self.pcm) else { return };
        self.out.clear();
        self.rs.process(&self.pcm[..n * 2], &mut self.out);
        self.jitter.lock().unwrap_or_else(|e| e.into_inner()).push(&self.out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, rate: u32) -> Vec<f32> {
        (0..frames).flat_map(|i| {
            let s = 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
            [s, s]
        })
        .collect()
    }

    #[test]
    fn opus_round_trip_keeps_the_sound() {
        let mut enc = Encoder::new().unwrap();
        // Entra em pedaços de tamanhos quebrados, como chega da captura.
        let pcm = tone(FRAME * 30, RATE);
        let mut packets = Vec::new();
        for part in pcm.chunks(777 * 2) {
            packets.extend(enc.push(part));
        }
        assert_eq!(packets.len(), 30);
        assert!(packets.iter().all(|p| p.len() < 400), "pacotes grandes demais");
        let mut dec = OpusDecoder::new(SampleRate::Hz48000, Channels::Stereo).unwrap();
        let mut out = vec![0.0f32; FRAME * 2];
        let mut energy = 0.0f32;
        // O Opus entra suave nos primeiros ~150 ms: mede depois disso.
        for (i, p) in packets.iter().enumerate() {
            let n = dec.decode(p, FRAME, &mut out).unwrap();
            assert_eq!(n, FRAME);
            if i >= 20 {
                energy += out.iter().map(|s| s * s).sum::<f32>();
            }
        }
        // Tom de 0,4 de amplitude: energia média por amostra ≈ 0,08.
        let avg = energy / (10 * FRAME * 2) as f32;
        assert!((0.065..0.095).contains(&avg), "energia {avg}");
    }

    #[test]
    fn resampler_keeps_duration() {
        let mut rs = Resampler::new(44_100, RATE);
        let mut out = Vec::new();
        let pcm = tone(44_100, 44_100);
        for part in pcm.chunks(512 * 2) {
            rs.process(part, &mut out);
        }
        let frames = out.len() / 2;
        assert!((47_990..=48_010).contains(&frames), "{frames} quadros");
    }

    /// Com o som de verdade deste computador (não roda no CI):
    /// `cargo test -p elo-core --release -- --ignored som_real`.
    #[test]
    #[ignore]
    fn som_real_captura_e_saida() {
        let mut cap = Capture::open().expect("captura");
        let mut enc = Encoder::new().unwrap();
        let mut pcm = Vec::new();
        let mut packets = 0;
        let end = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while std::time::Instant::now() < end {
            let mut chunk = Vec::new();
            cap.read(&mut chunk);
            packets += enc.push(&chunk).len();
            pcm.extend(chunk);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let secs = pcm.len() as f32 / (RATE as f32 * 2.0);
        eprintln!("capturado: {secs:.2} s, {packets} pacotes");
        assert!(secs > 0.7, "captura curta demais: {secs} s");
        // Saída: toca silêncio por meio segundo.
        let mut player = Player::open(0.0).expect("saída de som");
        let mut silent = Encoder::new().unwrap();
        for p in silent.push(&vec![0.0; FRAME * 2 * 25]) {
            player.push(&p);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    #[test]
    fn jitter_waits_then_plays_and_caps_latency() {
        let mut j = Jitter { buf: VecDeque::new(), priming: true, start: 8, max: 20 };
        j.push(&[0.1; 4]);
        assert!(j.pop().is_none(), "ainda juntando");
        j.push(&[0.2; 4]);
        assert_eq!(j.pop(), Some([0.1, 0.1]));
        j.push(&[0.3; 40]);
        assert!(j.buf.len() <= 20);
    }
}
