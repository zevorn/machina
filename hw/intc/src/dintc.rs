use std::sync::Arc;

use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const VIRT_DINTC_BASE: u64 = 0x2FE0_0000;
const NUM_MSGIS_WORDS: usize = 4;

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = lower_outputs)]
pub struct Dintc {
    state: parking_lot::Mutex<SysBusDeviceState>,
    #[allow(dead_code)]
    num_cpus: u32,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
    pending_vectors: parking_lot::Mutex<Vec<[u64; NUM_MSGIS_WORDS]>>,
}

impl Dintc {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("dintc", 1)
    }

    #[must_use]
    pub fn new_named(local_id: &str, num_cpus: u32) -> Self {
        let count = num_cpus.max(1) as usize;
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            num_cpus: num_cpus.max(1),
            outputs: parking_lot::Mutex::new({
                let mut v = Vec::with_capacity(count);
                v.resize_with(count, || None);
                v
            }),
            pending_vectors: parking_lot::Mutex::new(vec![
                [0u64;
                    NUM_MSGIS_WORDS];
                count
            ]),
        }
    }

    pub fn connect_output(&self, cpu_id: u32, irq: InterruptSource) {
        let mut outputs = self.outputs.lock();
        while outputs.len() <= cpu_id as usize {
            outputs.push(None);
        }
        outputs[cpu_id as usize] = Some(irq);
    }

    // Observable per-CPU pending vector. Returns the pending bitmap for a
    // given CPU so tests can verify which IRQ vectors were delivered.
    #[must_use]
    pub fn pending_vector(&self, cpu_id: u32) -> u64 {
        self.pending_vector_word(cpu_id, 0)
    }

    #[must_use]
    pub fn pending_vector_word(&self, cpu_id: u32, word: usize) -> u64 {
        self.pending_vectors
            .lock()
            .get(cpu_id as usize)
            .and_then(|words| words.get(word))
            .copied()
            .unwrap_or(0)
    }

    pub fn reset_runtime(&self) {
        self.lower_outputs();
        let mut pending = self.pending_vectors.lock();
        for words in pending.iter_mut() {
            *words = [0; NUM_MSGIS_WORDS];
        }
    }

    fn lower_outputs(&self) {
        let outputs = self.outputs.lock();
        for line in outputs.iter().flatten() {
            line.lower();
        }
    }
}

impl Default for Dintc {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DintcMmio(pub Arc<Dintc>);

impl MmioOps for DintcMmio {
    fn read(&self, _offset: u64, _size: u32) -> u64 {
        0
    }

    fn write(&self, offset: u64, _size: u32, _val: u64) {
        let msg_addr = offset + VIRT_DINTC_BASE;
        let cpu_num = ((msg_addr >> 12) & 0xff) as u32;
        let irq_num = ((msg_addr >> 4) & 0xff) as u32;
        // Set the pending vector bit for this CPU.
        {
            let mut pending = self.0.pending_vectors.lock();
            if let Some(words) = pending.get_mut(cpu_num as usize) {
                let word = (irq_num / 64) as usize;
                let bit = irq_num % 64;
                if let Some(slot) = words.get_mut(word) {
                    *slot |= 1u64 << bit;
                }
            }
        }
        let outputs = self.0.outputs.lock();
        if let Some(Some(line)) = outputs.get(cpu_num as usize) {
            line.set(true);
        }
    }
}
