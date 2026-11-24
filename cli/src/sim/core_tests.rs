//! Cortex-M3 core tests written as assembly programs: flags, IT blocks, exception entry and
//! return (stack alignment, EXC_RETURN), NVIC priorities, PRIMASK/BASEPRI, preemption,
//! tail-chaining, level-sensitive re-entry and UNDEFINED encodings.

use super::lab_tests::{boot, run_until};
use crate::sim::cpu::HaltReason;
use crate::sim::Simulator;

/// Run until BKPT; panics on a fault or if BKPT is not reached.
fn run_to_bkpt(sim: &mut Simulator) {
    for _ in 0..2_000_000 {
        sim.cpu.halted_reason = HaltReason::Running;
        sim.cpu.step(&mut sim.mem);
        match &sim.cpu.halted_reason {
            HaltReason::Bkpt(_) => return,
            HaltReason::Fault(f) => panic!("fault at 0x{:08X}: {}", sim.cpu.r[15], f),
            _ => {}
        }
    }
    panic!("BKPT not reached; pc=0x{:08X}", sim.cpu.r[15]);
}

fn prog(body: &str, extra: &str) -> String {
    format!(
        "    AREA main, CODE, READONLY\n    EXPORT _main\n{extra}    ENTRY\n_main PROC\n{body}\n    BKPT #0\n    ENDP\n    END\n"
    )
}

const N: u32 = 1 << 31;
const Z: u32 = 1 << 30;
const C: u32 = 1 << 29;
const V: u32 = 1 << 28;
const Q: u32 = 1 << 27;

#[test]
fn flags_and_arithmetic() {
    let src = prog(
        "
    LDR r0, =0x7FFFFFFF
    ADDS r0, r0, #1             ; signed overflow
    MRS r8, APSR
    MOVS r1, #0
    SUBS r1, r1, #1             ; borrow: C clear
    MRS r9, APSR
    CMP r1, r1                  ; equal: Z and C set
    MRS r10, APSR
    LDR r2, =0xFFFFFFFF         ; 64-bit add 0x00000001_FFFFFFFF + 1
    MOVS r3, #1
    ADDS r2, r2, #1
    ADC r3, r3, #0
    LDR r4, =0x80000001
    ASRS r5, r4, #32            ; all sign bits, C = bit 31
    MRS r11, APSR
    LSLS r6, r4, #1             ; C = old bit 31
    MRS r12, APSR
    MOVS r7, #0
    UDIV r7, r4, r7             ; divide by zero gives 0 (DIV_0_TRP clear)
    LDR r0, =0x12345
    SSAT r0, #8, r0             ; saturates to 127, sets Q
",
        "",
    );
    let mut sim = boot(&src);
    run_to_bkpt(&mut sim);
    let r = sim.cpu.r;
    assert_eq!(r[8] & (N | Z | C | V), N | V, "0x7FFFFFFF + 1");
    assert_eq!(r[9] & (N | Z | C | V), N, "0 - 1");
    assert_eq!(r[10] & (N | Z | C | V), Z | C, "CMP equal");
    assert_eq!((r[2], r[3]), (0, 2), "carry into the high word");
    assert_eq!(r[5], 0xFFFF_FFFF);
    assert_eq!(r[11] & (N | C), N | C);
    assert_eq!(r[6], 2);
    assert_eq!(r[12] & (N | Z | C), C);
    assert_eq!(r[7], 0);
    assert_eq!(r[0], 127);
    assert_ne!(sim.cpu.xpsr & Q, 0, "SSAT sets the sticky Q flag");
}

#[test]
fn it_blocks_condition_and_flag_setting() {
    let src = prog(
        "
    MOVS r0, #1
    MOVS r1, #0
    MOVS r2, #0
    MOVS r3, #0
    CMP r0, #1                  ; Z = 1
    ITTEE EQ
    MOVEQ r1, #11               ; executed
    ADDEQ r2, r2, #5            ; executed
    MOVNE r3, #7                ; skipped
    ADDNE r3, r3, #1            ; skipped
    MOVS r4, #0
    CMP r4, #0                  ; Z = 1, C = 1
    IT EQ
    ADDEQ r4, r4, r0            ; 16-bit ADD inside IT does not set flags
    MRS r5, APSR
    CMP r0, #2                  ; NE
    ITE NE
    MOVNE r6, #1
    BEQ fail                    ; last in IT: not taken
    MOVS r7, #0x55
    B done
fail
    MOVS r7, #0xEE
done
",
        "",
    );
    let mut sim = boot(&src);
    run_to_bkpt(&mut sim);
    let r = sim.cpu.r;
    assert_eq!((r[1], r[2], r[3]), (11, 5, 0));
    assert_eq!(r[4], 1);
    assert_eq!(r[5] & (N | Z | C), Z | C, "flags unchanged by ADDEQ in IT");
    assert_eq!(r[6], 1);
    assert_eq!(r[7], 0x55);
    assert_eq!(sim.cpu.xpsr & 0x0600_FC00, 0, "ITSTATE cleared after the block");
}

#[test]
fn exception_entry_realigns_stack_and_returns() {
    let src = prog(
        "
    SUB sp, sp, #4              ; SP is now 4 mod 8
    MOV r0, sp
    MOVS r5, #0x5A
    SVC #1
    MOV r1, sp
    ADD sp, sp, #4
",
        "    EXPORT SVC_Handler\n",
    ) + "";
    let src = src.replace(
        "    END\n",
        "SVC_Handler PROC
    MOV r8, sp
    LDR r9, [sp, #28]           ; stacked xPSR
    MOV r10, lr                 ; EXC_RETURN
    LDR r11, [sp, #24]          ; stacked return address
    MRS r6, IPSR
    MOVS r5, #0                 ; not part of the frame: stays 0 after return
    MOVS r0, #0                 ; part of the frame: restored on return
    BX lr
    ENDP
    END
",
    );
    let mut sim = boot(&src);
    run_to_bkpt(&mut sim);
    let r = sim.cpu.r;
    assert_eq!(r[0] % 8, 4, "r0 restored from the frame");
    assert_eq!(r[8], (r[0] - 32) & !7, "frame realigned to 8 bytes");
    assert_ne!(r[9] & (1 << 9), 0, "stacked xPSR bit 9 records the realignment");
    assert_eq!(r[10], 0xFFFF_FFF9, "return to Thread mode, MSP");
    assert_eq!(r[6], 11, "IPSR = SVCall");
    assert_eq!(r[1], r[0], "SP restored exactly on return");
    let svc = sim.elf.symbols.iter().find(|s| s.name == "_main").unwrap().addr;
    assert!(r[11] > svc && r[11] & 1 == 0, "stacked PC = instruction after SVC, no Thumb bit");
    assert_eq!(r[5], 0, "registers outside the frame keep the handler's value");
    assert_eq!(sim.cpu.xpsr & 0x1FF, 0, "back in Thread mode");
}

const NVIC_PROG: &str = r#"
NVIC_ISER0 EQU 0xE000E100
NVIC_ISPR0 EQU 0xE000E200
NVIC_IPR   EQU 0xE000E400

    AREA vars, DATA, READWRITE
log_idx  SPACE 4
nest_flag SPACE 4
log_buf  SPACE 64

    AREA main, CODE, READONLY
    EXPORT _main
    EXPORT EXTI0_IRQHandler
    EXPORT EXTI1_IRQHandler
    ENTRY

_main PROC
    LDR r0, =NVIC_IPR
    MOVS r1, #0x80
    STRB r1, [r0, #6]           ; EXTI0 (IRQ 6): priority 0x80
    MOVS r1, #0x40
    STRB r1, [r0, #7]           ; EXTI1 (IRQ 7): priority 0x40 (more urgent)
    LDR r0, =NVIC_ISER0
    MOVS r1, #0xC0
    STR r1, [r0]
    LDR r4, =NVIC_ISPR0

    ; 1: PRIMASK holds a pending interrupt back
    CPSID i
    MOVS r1, #0x40
    STR r1, [r4]
    MOVS r0, #0xA1
    BL log
    CPSIE i
    MOVS r0, #0xA2
    BL log

    ; 2: BASEPRI masks priority >= 0x80 only
    MOVS r0, #0x80
    MSR BASEPRI, r0
    MOVS r1, #0xC0
    STR r1, [r4]
    MOVS r0, #0xA3
    BL log
    MOVS r0, #0
    MSR BASEPRI, r0
    MOVS r0, #0xA4
    BL log

    ; 3: two pending: the more urgent first, then tail-chain into the other
    CPSID i
    MOVS r1, #0xC0
    STR r1, [r4]
    CPSIE i
    MOVS r0, #0xA5
    BL log

    ; 4: EXTI1 pended inside the EXTI0 handler preempts it
    LDR r0, =nest_flag
    MOVS r1, #1
    STR r1, [r0]
    MOVS r1, #0x40
    STR r1, [r4]
    MOVS r0, #0xA6
    BL log
    BKPT #0
    ENDP

EXTI0_IRQHandler PROC
    PUSH {r4, lr}
    MOVS r0, #6
    BL log
    LDR r0, =nest_flag
    LDR r1, [r0]
    CBZ r1, exti0_end
    MOVS r1, #0
    STR r1, [r0]
    LDR r0, =NVIC_ISPR0
    MOVS r1, #0x80
    STR r1, [r0]
    DSB
    ISB
exti0_end
    MOVS r0, #0x16
    BL log
    POP {r4, pc}
    ENDP

EXTI1_IRQHandler PROC
    PUSH {r4, lr}
    MOVS r0, #7
    BL log
    POP {r4, pc}
    ENDP

log PROC
    LDR r1, =log_idx
    LDR r2, [r1]
    LDR r3, =log_buf
    STRB r0, [r3, r2]
    ADDS r2, r2, #1
    STR r2, [r1]
    BX lr
    ENDP
    END
"#;

#[test]
fn nvic_priorities_masking_preemption_and_tail_chaining() {
    let mut sim = boot(NVIC_PROG);
    run_to_bkpt(&mut sim);
    let idx = sim.elf.sym_by_name["log_idx"];
    let buf = sim.elf.sym_by_name["log_buf"];
    let n = sim.mem.read32(idx);
    let log: Vec<u8> = (0..n).map(|i| sim.mem.read8(buf + i)).collect();
    assert_eq!(
        log,
        vec![
            0xA1, 6, 0x16, 0xA2, // PRIMASK
            7, 0xA3, 6, 0x16, 0xA4, // BASEPRI
            7, 6, 0x16, 0xA5, // priority order + tail-chain
            6, 7, 0x16, 0xA6, // preemption
        ]
    );
}

#[test]
fn tail_chain_skips_unstacking() {
    // with both pending, the second handler is entered straight from the first one's
    // exception return: 6-cycle tail-chain instead of 12-cycle unstack + 12-cycle stack
    let mut sim = boot(NVIC_PROG);
    let e0 = sim.elf.sym_by_name["EXTI0_IRQHandler"] & !1;
    let e1 = sim.elf.sym_by_name["EXTI1_IRQHandler"] & !1;
    // skip scenarios 1 and 2
    let mut seen = 0;
    run_until(&mut sim, 1_000_000, |s| {
        if s.cpu.r[15] == e1 {
            seen += 1;
        }
        seen == 2
    })
    .expect("EXTI1 entered for scenario 3");
    let mut in_thread = false;
    let mut last_pc = 0;
    run_until(&mut sim, 10_000, |s| {
        if s.cpu.xpsr & 0x1FF == 0 {
            in_thread = true;
        }
        let hit = s.cpu.r[15] == e0 && last_pc != e0;
        last_pc = s.cpu.r[15];
        hit
    })
    .expect("EXTI0 tail-chained");
    assert!(!in_thread, "no Thread-mode instruction between the two handlers");
    assert_eq!(sim.cpu.xpsr & 0x1FF, 22);
}

#[test]
fn level_sensitive_irq_reenters_until_source_cleared() {
    // EXTI pending bit not cleared -> the NVIC line stays high -> handler re-entered
    let src = r#"
EXTI_BASE  EQU 0x40010400
    AREA vars, DATA, READWRITE
count SPACE 4
    AREA main, CODE, READONLY
    EXPORT _main
    EXPORT EXTI0_IRQHandler
    ENTRY
_main PROC
    LDR r0, =0x40021018
    MOVS r1, #1                 ; AFIO clock
    STR r1, [r0]
    LDR r0, =EXTI_BASE
    MOVS r1, #1
    STR r1, [r0]                ; IMR0
    LDR r2, =0xE000E100
    MOVS r3, #(1 << 6)
    STR r3, [r2]                ; enable EXTI0
    STR r1, [r0, #0x10]         ; SWIER0: software trigger
    NOP
    BKPT #0
    ENDP
EXTI0_IRQHandler PROC
    LDR r0, =count
    LDR r1, [r0]
    ADDS r1, r1, #1
    STR r1, [r0]
    CMP r1, #3
    BLO keep
    LDR r0, =EXTI_BASE
    MOVS r1, #1
    STR r1, [r0, #0x14]         ; clear PR0 on the third run
    DSB
keep
    BX lr
    ENDP
    END
"#;
    let mut sim = boot(src);
    run_to_bkpt(&mut sim);
    let count = sim.mem.read32(sim.elf.sym_by_name["count"]);
    assert_eq!(count, 3);
    assert_eq!(sim.mem.peripherals.exti.pr, 0);
}

#[test]
fn undefined_wide_store_encoding_faults() {
    // F9C0 1000 is STR.W with the signed bit set: UNDEFINED, not a store
    let src = prog(
        "
    LDR r0, =0x20000000
    MOVS r1, #0x77
    DCW 0xF9C0, 0x1000
",
        "",
    );
    let mut sim = boot(&src);
    let hf = sim.elf.sym_by_name["HardFault_Handler"] & !1;
    // the simulator stops like a debugger when the fault escalates; keep stepping through it
    let mut taken = false;
    for _ in 0..1_000_000 {
        sim.cpu.halted_reason = HaltReason::Running;
        sim.cpu.step(&mut sim.mem);
        if sim.cpu.r[15] == hf {
            taken = true;
            break;
        }
    }
    assert!(taken, "HardFault taken");
    let cfsr = sim.mem.read32(0xE000_ED28);
    let hfsr = sim.mem.read32(0xE000_ED2C);
    assert_ne!(cfsr & (1 << 16), 0, "UFSR.UNDEFINSTR");
    assert_ne!(hfsr & (1 << 30), 0, "HFSR.FORCED (UsageFault disabled)");
    assert_ne!(sim.mem.read32(0x2000_0000), 0x77, "nothing was stored");
}

#[test]
fn bkpt_in_it_block_is_unconditional_and_unprivileged_mrs_reads_zero() {
    let src = prog(
        "
    MOVS r0, #1
    MSR PRIMASK, r0
    MOVS r0, #0x80
    MSR BASEPRI, r0
    MOVS r0, #1
    MSR CONTROL, r0             ; unprivileged Thread mode
    ISB
    MRS r4, PRIMASK
    MRS r5, BASEPRI
    CMP r0, #0                  ; Z clear
    IT EQ
    BKPT #7                     ; still stops: BKPT ignores the IT condition
",
        "",
    );
    let mut sim = boot(&src);
    for _ in 0..1_000_000 {
        sim.cpu.halted_reason = HaltReason::Running;
        sim.cpu.step(&mut sim.mem);
        if !matches!(sim.cpu.halted_reason, HaltReason::Running) {
            break;
        }
    }
    assert!(matches!(sim.cpu.halted_reason, HaltReason::Bkpt(7)), "{:?}", sim.cpu.halted_reason);
    assert_eq!((sim.cpu.r[4], sim.cpu.r[5]), (0, 0), "mask registers read as zero when unprivileged");
}

#[test]
fn failed_unstacking_escalates_to_hardfault_without_resuming() {
    let src = prog("    SVC #0\n    MOVS r7, #0x77              ; must never run\n", "    EXPORT SVC_Handler\n").replace(
        "    END\n",
        "SVC_Handler PROC
    LDR r0, =0x30000000         ; nothing is mapped here
    MSR PSP, r0
    LDR r0, =0xFFFFFFFD         ; return to Thread mode on the PSP
    BX r0
    ENDP
    END
",
    );
    let mut sim = boot(&src);
    let hf = sim.elf.symbols.iter().find(|s| s.name == "HardFault_Handler").unwrap().addr;
    let mut taken = false;
    for _ in 0..1_000_000 {
        sim.cpu.halted_reason = HaltReason::Running;
        sim.cpu.step(&mut sim.mem);
        if sim.cpu.r[15] == hf {
            taken = true;
            break;
        }
    }
    assert!(taken, "HardFault taken during the return, pc=0x{:08X}", sim.cpu.r[15]);
    assert_ne!(sim.mem.read32(0xE000_ED28) & (1 << 11), 0, "BFSR.UNSTKERR");
    assert_ne!(sim.mem.read32(0xE000_ED2C) & (1 << 30), 0, "HFSR.FORCED (BusFault disabled)");
    assert_ne!(sim.cpu.r[7], 0x77);
    assert_eq!(sim.cpu.r[14], 0xFFFF_FFFD, "derived exception keeps the original EXC_RETURN");
}

#[test]
fn nmi_return_with_faultmask_does_not_tail_chain() {
    let src = prog(
        "
    LDR r0, =0xE000E100
    MOVS r1, #0x40              ; enable EXTI0 (IRQ 6)
    STR r1, [r0]
    LDR r0, =0xE000E200
    STR r1, [r0]                ; pend EXTI0
    CPSID f                     ; FAULTMASK = 1: EXTI0 stays pending
    LDR r0, =0xE000ED04
    MOVS r1, #1
    LSLS r1, r1, #31
    STR r1, [r0]                ; NMIPENDSET
    MRS r6, IPSR                ; back in Thread mode, EXTI0 not taken
    CPSIE f
    NOP
",
        "    EXPORT NMI_Handler\n    EXPORT EXTI0_IRQHandler\n",
    )
    .replace(
        "    END\n",
        "NMI_Handler PROC
    BX lr
    ENDP
EXTI0_IRQHandler PROC
    MRS r8, FAULTMASK
    ADDS r9, r9, #1
    LDR r0, =0xE000E280
    MOVS r1, #0x40
    STR r1, [r0]                ; clear pending
    BX lr
    ENDP
    END
",
    );
    let mut sim = boot(&src);
    run_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[6], 0, "NMI return did not tail-chain into EXTI0 while FAULTMASK was set");
    assert_eq!(sim.cpu.r[9], 1, "EXTI0 ran once FAULTMASK was cleared");
    assert_eq!(sim.cpu.r[8], 0);
}

/// Run until BKPT, stepping past fault reports (the debugger stops on each one).
fn run_through_faults_to_bkpt(sim: &mut Simulator) {
    for _ in 0..1_000_000 {
        sim.cpu.halted_reason = HaltReason::Running;
        sim.cpu.step(&mut sim.mem);
        assert!(!sim.cpu.locked_up, "locked up: {:?}", sim.cpu.halted_reason);
        if matches!(sim.cpu.halted_reason, HaltReason::Bkpt(_)) {
            return;
        }
    }
    panic!("BKPT not reached; pc=0x{:08X}", sim.cpu.r[15]);
}

/// SVC with the main stack 16 bytes into SRAM: half of the exception frame lands below it.
fn stacking_fault_prog(setup: &str) -> String {
    let body = format!("{setup}    LDR r0, =0x20000010\n    MOV sp, r0\n    SVC #0\n");
    prog(&body, "    EXPORT SVC_Handler\n    EXPORT HardFault_Handler\n    EXPORT BusFault_Handler\n").replace(
        "    END\n",
        "SVC_Handler PROC
    MOVS r7, #0x11
    BKPT #1
    ENDP
HardFault_Handler PROC
    MOVS r7, #0x42
    BKPT #2
    ENDP
BusFault_Handler PROC
    MOVS r7, #0x33
    BKPT #3
    ENDP
    END
",
    )
}

#[test]
fn stacking_fault_is_taken_as_hardfault_instead_of_the_svc() {
    let mut sim = boot(&stacking_fault_prog(""));
    run_through_faults_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[7], 0x42, "HardFault handler runs; no lockup from a second stacking attempt");
    assert_ne!(sim.mem.read32(0xE000_ED28) & (1 << 12), 0, "BFSR.STKERR");
    assert_ne!(sim.mem.read32(0xE000_ED2C) & (1 << 30), 0, "HFSR.FORCED (BusFault disabled)");
    assert!(sim.mem.peripherals.nvic.pending[11], "the SVC stays pending");
}

#[test]
fn stacking_fault_escalates_when_basepri_blocks_busfault() {
    let setup = "    LDR r0, =0xE000ED24
    LDR r1, [r0]
    ORR r1, r1, #(1 << 17)      ; SHCSR.BUSFAULTENA
    STR r1, [r0]
    LDR r0, =0xE000ED18
    MOVS r1, #0x4000            ; BusFault priority 0x40, SVC stays at 0
    STR r1, [r0]
    MOVS r0, #0x40
    MSR BASEPRI, r0
";
    let mut sim = boot(&stacking_fault_prog(setup));
    run_through_faults_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[7], 0x42, "BusFault cannot preempt BASEPRI 0x40: forced HardFault");
    assert_ne!(sim.mem.read32(0xE000_ED2C) & (1 << 30), 0, "HFSR.FORCED");
}

#[test]
fn enabled_busfault_from_stacking_waits_behind_a_higher_priority_svc() {
    let setup = "    LDR r0, =0xE000ED24
    LDR r1, [r0]
    ORR r1, r1, #(1 << 17)      ; SHCSR.BUSFAULTENA
    STR r1, [r0]
    LDR r0, =0xE000ED18
    MOVS r1, #0x4000            ; BusFault priority 0x40, SVC stays at 0
    STR r1, [r0]
";
    let mut sim = boot(&stacking_fault_prog(setup));
    run_through_faults_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[7], 0x11, "the SVC (priority 0) is taken first");
    assert!(sim.mem.peripherals.nvic.pending[5], "BusFault STKERR pending");
    assert_eq!(sim.mem.read32(0xE000_ED2C) & (1 << 30), 0, "no escalation");
}

/// TIM2 update interrupt raised with the IRQ left disabled in the NVIC, then WFE.
fn wfe_on_disabled_irq(scr: u32) -> String {
    prog(
        &format!(
            "    LDR r0, =0x4002101C
    MOVS r1, #1
    STR r1, [r0]                ; RCC_APB1ENR: TIM2
    LDR r0, =0x40000000
    MOVS r1, #99
    STR r1, [r0, #0x2C]         ; ARR
    MOVS r1, #1
    STR r1, [r0, #0x0C]         ; DIER.UIE (TIM2 IRQ stays disabled in the NVIC)
    STR r1, [r0, #0x00]         ; CEN
    LDR r0, =0xE000ED10
    MOVS r1, #{scr}
    STR r1, [r0]                ; SCR
    SEV
    WFE                         ; consumes the event register
    WFE                         ; sleeps
    MOVS r7, #1
"
        ),
        "",
    )
}

#[test]
fn sevonpend_wakes_wfe_when_a_disabled_interrupt_becomes_pending() {
    let mut sim = boot(&wfe_on_disabled_irq(0x10));
    run_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[7], 1);
    assert!(sim.mem.peripherals.nvic.pending[16 + 28], "TIM2 pending, not taken");

    let mut sim = boot(&wfe_on_disabled_irq(0));
    for _ in 0..2_000 {
        sim.cpu.step(&mut sim.mem);
    }
    assert!(sim.cpu.sleeping, "without SEVONPEND a disabled interrupt does not wake WFE");
    assert_ne!(sim.cpu.r[7], 1);
}

#[test]
fn sevonpend_latches_an_event_that_arrives_before_wfe() {
    let src = prog(
        "    LDR r0, =0xE000ED10
    MOVS r1, #0x10
    STR r1, [r0]                ; SCR.SEVONPEND
    SEV
    WFE                         ; consumes the event register
    LDR r0, =0xE000E200
    MOVS r1, #1
    STR r1, [r0]                ; NVIC_ISPR0: pend IRQ0, which is disabled
    WFE                         ; the latched event: continues without sleeping
    MOVS r7, #2
",
        "",
    );
    let mut sim = boot(&src);
    run_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[7], 2);
}

#[test]
fn equal_priority_stacking_busfault_does_not_displace_the_svc() {
    let setup = "    LDR r0, =0xE000ED24
    LDR r1, [r0]
    ORR r1, r1, #(1 << 17)      ; SHCSR.BUSFAULTENA, BusFault and SVC both at priority 0
    STR r1, [r0]
";
    let mut sim = boot(&stacking_fault_prog(setup));
    run_through_faults_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[7], 0x11, "the SVC enters first");
    assert!(sim.mem.peripherals.nvic.pending[5], "BusFault STKERR pending");
}

#[test]
fn step_out_of_a_handler_after_a_bl_stops_at_the_exception_return() {
    use crate::target::DebugTarget;
    let src = prog("    SVC #0\nback\n    MOVS r7, #5\n", "    EXPORT SVC_Handler\n").replace(
        "    END\n",
        "SVC_Handler PROC
    PUSH {r4, lr}
    BL helper
after_bl
    NOP
    POP {r4, pc}
    ENDP
helper PROC
    BX lr
    ENDP
    END
",
    );
    let mut sim = boot(&src);
    let after_bl = sim.elf.symbol_addr("after_bl").unwrap();
    run_until(&mut sim, 100_000, |s| s.cpu.r[15] == after_bl).expect("inside the handler after BL");
    let st = sim.step_out().unwrap();
    assert_eq!(st.halt_reason, "StepOut");
    assert_eq!(sim.mem.peripherals.ipsr, 0, "back in Thread mode");
    assert_eq!(sim.cpu.r[15], sim.elf.symbol_addr("back").unwrap());
}

#[test]
fn step_out_reports_a_fault_on_exception_return() {
    use crate::target::DebugTarget;
    let src = prog("    SVC #0\n", "    EXPORT SVC_Handler\n").replace(
        "    END\n",
        "SVC_Handler PROC
    LDR r0, =0x30000000         ; nothing is mapped here
    MSR PSP, r0
    LDR r0, =0xFFFFFFFD
    BX r0
    ENDP
    END
",
    );
    let mut sim = boot(&src);
    let svc = sim.elf.symbol_addr("SVC_Handler").unwrap();
    run_until(&mut sim, 100_000, |s| s.cpu.r[15] == svc).expect("SVC taken");
    let st = sim.step_out().unwrap();
    assert!(st.halt_reason.starts_with("Fault"), "reason {}", st.halt_reason);
}

#[test]
fn sevonpend_counts_a_transition_during_the_store_that_enables_it() {
    let src = prog(
        "    LDR r0, =0x4002101C
    MOVS r1, #1
    STR r1, [r0]                ; RCC_APB1ENR: TIM2
    LDR r0, =0x40000000
    MOVS r1, #3
    STR r1, [r0, #0x2C]         ; ARR
    MOVS r1, #1
    STR r1, [r0, #0x0C]         ; DIER.UIE, TIM2 IRQ disabled in the NVIC
    LDR r2, =0xE000ED10
    MOVS r3, #0x10
    SEV
    WFE                         ; event register clear
    STR r1, [r0, #0x00]         ; CEN
    STR r3, [r2]                ; SCR.SEVONPEND
    WFE
    MOVS r7, #3
",
        "",
    );
    let mut sim = boot(&src);
    run_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[7], 3);
}

#[test]
fn exception_entry_and_return_do_not_set_the_event_register_on_r1p1() {
    let src = prog("    SEV\n    WFE\n    SVC #0\n    WFE\n    MOVS r7, #4\n", "    EXPORT SVC_Handler\n").replace(
        "    END\n",
        "SVC_Handler PROC\n    BX lr\n    ENDP\n    END\n",
    );
    let mut sim = boot(&src);
    for _ in 0..2_000 {
        sim.cpu.step(&mut sim.mem);
    }
    assert!(sim.cpu.sleeping, "WFE after an SVC round trip sleeps (Arm erratum 563915)");
    assert_ne!(sim.cpu.r[7], 4);
}

#[test]
fn step_out_of_a_handler_that_calls_a_helper_in_a_loop() {
    use crate::target::DebugTarget;
    let src = prog("    SVC #0\nback\n    MOVS r7, #5\n", "    EXPORT SVC_Handler\n").replace(
        "    END\n",
        "SVC_Handler PROC
    PUSH {r4, lr}
    MOVS r4, #3
again
    BL helper
after_bl
    SUBS r4, r4, #1
    BNE again
    POP {r4, pc}
    ENDP
helper PROC
    BX lr
    ENDP
    END
",
    );
    let mut sim = boot(&src);
    let after_bl = sim.elf.symbol_addr("after_bl").unwrap();
    run_until(&mut sim, 100_000, |s| s.cpu.r[15] == after_bl).expect("after the first helper call");
    let st = sim.step_out().unwrap();
    assert_eq!(st.halt_reason, "StepOut");
    assert_eq!(sim.mem.peripherals.ipsr, 0, "left the handler, not the next loop iteration");
    assert_eq!(sim.cpu.r[15], sim.elf.symbol_addr("back").unwrap());
}

#[test]
fn sevonpend_event_survives_an_interrupt_taken_at_once() {
    let src = prog(
        "    LDR r0, =0xE000ED10
    MOVS r1, #0x10
    STR r1, [r0]                ; SCR.SEVONPEND
    SEV
    WFE                         ; event register clear
    LDR r0, =0xE000E100
    MOVS r1, #1
    STR r1, [r0]                ; NVIC_ISER0: IRQ0 enabled
    STR r1, [r0, #0x100]        ; NVIC_ISPR0: pended, then taken straight away
    WFE                         ; the pending transition was an event: continues
    MOVS r7, #6
",
        "    EXPORT WWDG_IRQHandler\n",
    )
    .replace("    END\n", "WWDG_IRQHandler PROC\n    BX lr\n    ENDP\n    END\n");
    let mut sim = boot(&src);
    run_to_bkpt(&mut sim);
    assert_eq!(sim.cpu.r[7], 6);
}

#[test]
fn step_out_of_a_local_helper_inside_a_handler_returns_to_its_caller() {
    use crate::target::DebugTarget;
    let src = prog("    SVC #0\n", "    EXPORT SVC_Handler\n").replace(
        "    END\n",
        "SVC_Handler PROC
    PUSH {r4, lr}
    BL local_helper
after_call
    POP {r4, pc}
local_helper
    NOP
    BX lr
    ENDP
    END
",
    );
    let mut sim = boot(&src);
    let helper = sim.elf.symbol_addr("local_helper").unwrap();
    run_until(&mut sim, 100_000, |s| s.cpu.r[15] == helper).expect("inside the helper");
    let st = sim.step_out().unwrap();
    assert_eq!(st.halt_reason, "StepOut");
    assert_eq!(sim.cpu.r[15], sim.elf.symbol_addr("after_call").unwrap(), "stops at the caller, still in the handler");
    assert_eq!(sim.mem.peripherals.ipsr, 11);
}
