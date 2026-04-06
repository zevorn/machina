// IRQ routing primitives — lines, OR-gates, fan-out.

use std::sync::{Arc, Mutex};

/// Receives interrupt level changes.
pub trait IrqSink: Send + Sync {
    fn set_irq(&self, irq: u32, level: bool);
}

/// A single interrupt wire connecting a source to a sink.
#[derive(Clone)]
pub struct IrqLine {
    sink: Arc<dyn IrqSink>,
    irq: u32,
}

impl IrqLine {
    pub fn new(sink: Arc<dyn IrqSink>, irq: u32) -> Self {
        Self { sink, irq }
    }

    pub fn set(&self, level: bool) {
        self.sink.set_irq(self.irq, level);
    }

    pub fn raise(&self) {
        self.set(true);
    }

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
    pub fn new(sink: Arc<dyn IrqSink>, irq_num: u32) -> Self {
        Self { sink, irq_num }
    }

    pub fn raise(&self) {
        self.sink.set_irq(self.irq_num, true);
    }

    pub fn lower(&self) {
        self.sink.set_irq(self.irq_num, false);
    }

    pub fn set(&self, level: bool) {
        self.sink.set_irq(self.irq_num, level);
    }

    pub fn irq_num(&self) -> u32 {
        self.irq_num
    }
}
