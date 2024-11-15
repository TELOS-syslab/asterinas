// SPDX-License-Identifier: MPL-2.0

//! Symmetric Multi-Processing (SMP) support.
//!
//! This module provides a way to execute code on other processors via inter-
//! processor interrupts.

use alloc::collections::VecDeque;

use spin::Once;

use crate::{
    cpu::{CpuId, CpuSet, PinCurrentCpu},
    cpu_local,
    sync::SpinLock,
    task::disable_preempt,
    trap::{self, IrqLine, TrapFrame},
};

/// Execute a function on other processors.
///
/// The provided function `f` will be executed on all target processors
/// specified by `targets`. It can also be executed on the current processor.
/// The function should be short and non-blocking, as it will be executed in
/// interrupt context with interrupts disabled.
///
/// The executed function will be provided by the caller CPU ID.
///
/// This function does not block until all the target processors acknowledges
/// the interrupt. So if any of the target processors disables IRQs for too
/// long that the controller cannot queue them, the function will not be
/// executed.
///
/// The function `f` will be executed asynchronously on the target processors.
/// However if called on the current processor, it will be synchronous.
pub fn inter_processor_call(targets: &CpuSet, f: fn(CpuId)) {
    let irq_guard = trap::disable_local();
    let this_cpu_id = irq_guard.current_cpu();
    let irq_num = INTER_PROCESSOR_CALL_IRQ.get().unwrap().num();

    let mut call_on_self = false;
    for cpu_id in targets.iter() {
        if cpu_id == this_cpu_id {
            call_on_self = true;
            continue;
        }
        CALL_QUEUES
            .get_on_cpu(cpu_id)
            .lock()
            .push_back((f, this_cpu_id));
    }
    for cpu_id in targets.iter() {
        if cpu_id == this_cpu_id {
            continue;
        }
        // SAFETY: It is safe to send inter processor call IPI to other CPUs.
        unsafe {
            crate::arch::irq::send_ipi(cpu_id, irq_num);
        }
    }
    if call_on_self {
        // Execute the function synchronously.
        f(this_cpu_id);
    }
}

static INTER_PROCESSOR_CALL_IRQ: Once<IrqLine> = Once::new();

cpu_local! {
    static CALL_QUEUES: SpinLock<VecDeque<(fn(CpuId), CpuId)>> = SpinLock::new(VecDeque::new());
}

fn do_inter_processor_call(_trapframe: &TrapFrame) {
    let preempt_guard = disable_preempt();
    let cur_cpu = preempt_guard.current_cpu();

    let mut queue = CALL_QUEUES.get_on_cpu(cur_cpu).lock();
    while let Some((f, id)) = queue.pop_front() {
        log::trace!(
            "Performing inter-processor call to {:#?} on CPU {:#?}",
            f,
            cur_cpu
        );
        f(id);
    }
}

pub(super) fn init() {
    let mut irq = IrqLine::alloc().unwrap();
    irq.on_active(do_inter_processor_call);
    INTER_PROCESSOR_CALL_IRQ.call_once(|| irq);
}
