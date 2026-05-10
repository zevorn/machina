// IRQ routing primitives — lines, OR-gates, fan-out.

use std::sync::{Arc, Mutex};

/// Receives interrupt level changes.
pub trait IrqSink: Send + Sync {
    /// Drive the input line `irq` to `level` (true = high,
    /// false = low). Implementations must be thread-safe and
    /// must not block; level edges are conveyed as the post-
    /// transition level, never as transient pulses.
    fn set_irq(&self, irq: u32, level: bool);
}

/// A single interrupt wire connecting a source to a sink.
#[derive(Clone)]
pub struct IrqLine {
    sink: Arc<dyn IrqSink>,
    irq: u32,
}

impl IrqLine {
    /// Bind this wire to `sink` at downstream input number
    /// `irq`. The line is conceptually low until `set`/`raise`
    /// is called.
    pub fn new(sink: Arc<dyn IrqSink>, irq: u32) -> Self {
        Self { sink, irq }
    }

    /// Drive the wire to `level`; forwarded verbatim to the
    /// sink as `set_irq(self.irq, level)`.
    pub fn set(&self, level: bool) {
        self.sink.set_irq(self.irq, level);
    }

    /// Drive the wire high. Equivalent to `self.set(true)`.
    pub fn raise(&self) {
        self.set(true);
    }

    /// Drive the wire low. Equivalent to `self.set(false)`.
    pub fn lower(&self) {
        self.set(false);
    }
}

/// OR gate: output is high if any input is high.
pub struct OrIrq {
    levels: Mutex<Vec<bool>>,
    output: IrqLine,
}

impl OrIrq {
    /// Build an OR gate with `num_inputs` independent input
    /// lines, all initially low, that drives `output` high
    /// whenever any input is high.
    pub fn new(output: IrqLine, num_inputs: usize) -> Self {
        Self {
            levels: Mutex::new(vec![false; num_inputs]),
            output,
        }
    }
}

impl IrqSink for OrIrq {
    fn set_irq(&self, irq: u32, level: bool) {
        let mut levels = self.levels.lock().unwrap();
        levels[irq as usize] = level;
        let any_high = levels.iter().any(|&l| l);
        self.output.set(any_high);
    }
}

/// Fan-out: one input drives multiple outputs.
pub struct SplitIrq {
    outputs: Vec<IrqLine>,
}

impl SplitIrq {
    /// Build a fan-out from a single input to all entries in
    /// `outputs`. An empty `outputs` vector is permitted and
    /// turns this into a no-op sink.
    pub fn new(outputs: Vec<IrqLine>) -> Self {
        Self { outputs }
    }
}

impl IrqSink for SplitIrq {
    fn set_irq(&self, _irq: u32, level: bool) {
        for out in &self.outputs {
            out.set(level);
        }
    }
}

/// Lock-free interrupt output. Can be called from any
/// thread with `&self`. Uses the underlying `IrqSink`
/// which operates atomically (e.g. AtomicU64 for CPU
/// mip bits).
pub struct InterruptSource {
    sink: Arc<dyn IrqSink>,
    irq_num: u32,
}

impl InterruptSource {
    /// Bind a logical interrupt source to `sink` at input
    /// number `irq_num`. The bound `irq_num` is fixed for the
    /// lifetime of the source.
    pub fn new(sink: Arc<dyn IrqSink>, irq_num: u32) -> Self {
        Self { sink, irq_num }
    }

    /// Drive the bound input high. Equivalent to
    /// `self.set(true)` and safe to call from any thread.
    pub fn raise(&self) {
        self.sink.set_irq(self.irq_num, true);
    }

    /// Drive the bound input low. Equivalent to
    /// `self.set(false)` and safe to call from any thread.
    pub fn lower(&self) {
        self.sink.set_irq(self.irq_num, false);
    }

    /// Drive the bound input to `level`. The level is forwarded
    /// to the sink unchanged; no edge filtering is performed.
    pub fn set(&self, level: bool) {
        self.sink.set_irq(self.irq_num, level);
    }

    /// The downstream input number this source was bound to.
    pub fn irq_num(&self) -> u32 {
        self.irq_num
    }
}
