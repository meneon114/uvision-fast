//! End-to-end tests for RUET CSE 3106 STM32 Labs 3-6 on a Blue Pill (STM32F103C8T6).
//!
//! Each program is written in the lab's Keil style (`AREA main` / `_main PROC`, no vector
//! table: the startup code, SystemInit at 72 MHz and the vector table are synthesised) and
//! checked on pin levels, cycle timing at 72 MHz and interrupts on button edges, through the
//! same pin-stimulus calls the UI uses.

use crate::elf::LoadedElf;
use crate::project::TargetOptions;
use crate::sim::cpu::HaltReason;
use crate::sim::Simulator;
use crate::target::DebugTarget;

const PA: usize = 0;
const PB: usize = 1;
const PC: usize = 2;
const MS: u64 = 72_000;

pub(super) fn boot(src: &str) -> Simulator {
    let opts = TargetOptions { device: "STM32F103C8".into(), ..Default::default() };
    let b = crate::asm::assemble_sources(&opts, "lab.axf", None, None, &[("lab.s".to_string(), src.to_string())]);
    assert!(b.result.success, "{}", b.result.log);
    let elf = LoadedElf::load_from_bytes("lab.axf", b.elf_bytes.as_ref().unwrap()).unwrap();
    Simulator::new(elf, 8_000_000)
}

pub(super) fn step(sim: &mut Simulator) {
    sim.cpu.halted_reason = HaltReason::Running;
    sim.cpu.step(&mut sim.mem);
    if let HaltReason::Fault(f) = &sim.cpu.halted_reason {
        panic!("fault at 0x{:08X}: {}", sim.cpu.r[15], f);
    }
}

/// Step until `pred` holds or `max` cycles pass; returns the cycle count reached.
pub(super) fn run_until(sim: &mut Simulator, max: u64, mut pred: impl FnMut(&mut Simulator) -> bool) -> Option<u64> {
    let end = sim.cpu.cycles + max;
    while sim.cpu.cycles < end {
        step(sim);
        if pred(sim) {
            return Some(sim.cpu.cycles);
        }
    }
    None
}

fn run_for(sim: &mut Simulator, cycles: u64) {
    run_until(sim, cycles, |_| false);
}

fn pin(sim: &Simulator, port: usize, n: u32) -> bool {
    sim.mem.peripherals.gpios[port].pins & (1 << n) != 0
}

/// Cycle stamps of the next `n` level changes of a pin.
fn edges(sim: &mut Simulator, port: usize, n_pin: u32, n: usize, max_each: u64) -> Vec<(u64, bool)> {
    let mut out = Vec::new();
    for _ in 0..n {
        let before = pin(sim, port, n_pin);
        let t = run_until(sim, max_each, |s| pin(s, port, n_pin) != before)
            .unwrap_or_else(|| panic!("no edge on pin {} within {} cycles (have {:?})", n_pin, max_each, out));
        out.push((t, !before));
    }
    out
}

fn boot_to_main(src: &str) -> Simulator {
    let mut sim = boot(src);
    let main = sim.elf.sym_by_name["_main"] & !1;
    run_until(&mut sim, 2_000_000, |s| s.cpu.r[15] == main).expect("never reached _main");
    assert_eq!(sim.mem.peripherals.hclk_hz(), 72_000_000, "SystemInit must set 72 MHz");
    sim
}

// ---------------------------------------------------------------------------------------
// Lab 3: GPIO
// ---------------------------------------------------------------------------------------

#[test]
fn lab3_exp1_led_on_pa5() {
    let mut sim = boot_to_main(crate::asm::tests::LAB3_EXP1);
    let stop = sim.elf.sym_by_name["stop"] & !1;
    run_until(&mut sim, 10_000, |s| s.cpu.r[15] == stop).expect("never reached stop");
    let g = &sim.mem.peripherals.gpios[PA];
    assert_eq!((g.crl >> 20) & 0xF, 3, "PA5 output push-pull 50 MHz");
    assert!(pin(&sim, PA, 5), "PA5 high");
    assert_eq!(g.odr, 1 << 5);
    // the lab runs on plain flash addresses: the LED stays on in the B stop loop
    run_for(&mut sim, 1000);
    assert!(pin(&sim, PA, 5));
}

const LAB3_EXP2: &str = r#"
RCC_APB2ENR EQU 0x40021018
GPIOB_CRL   EQU 0x40010C00
GPIOB_ODR   EQU 0x40010C0C
GPIOC_CRH   EQU 0x40011004
GPIOC_IDR   EQU 0x40011008

    AREA main, CODE, READONLY
    EXPORT _main
    ENTRY

_main PROC
    LDR r0, =RCC_APB2ENR
    LDR r1, [r0]
    ORR r1, r1, #(3 << 3)           ; GPIOB, GPIOC
    STR r1, [r0]
    LDR r0, =GPIOB_CRL
    LDR r1, [r0]
    BIC r1, r1, #0xF
    ORR r1, r1, #3                  ; PB0 output
    STR r1, [r0]
    LDR r0, =GPIOC_CRH
    LDR r1, [r0]
    BIC r1, r1, #(0xF << 28)
    ORR r1, r1, #(4 << 28)          ; PC15 floating input
    STR r1, [r0]
    LDR r4, =GPIOC_IDR
    LDR r5, =GPIOB_ODR
    MOVS r6, #0                     ; lastButton
    MOVS r7, #0                     ; ledOn
loop
    LDR r0, [r4]
    UBFX r0, r0, #15, #1
    CMP r0, r6
    BEQ settled
    LDR r1, =50000                  ; debounce delay
debounce
    SUBS r1, r1, #1
    BNE debounce
    LDR r0, [r4]
    UBFX r0, r0, #15, #1
settled
    CMP r6, #0
    BNE no_toggle
    CMP r0, #1
    BNE no_toggle
    EOR r7, r7, #1                  ; rising edge: toggle
no_toggle
    MOV r6, r0
    LDR r1, [r5]
    BIC r1, r1, #1
    ORR r1, r1, r7
    STR r1, [r5]
    B loop
    ENDP
    END
"#;

#[test]
fn lab3_exp2_button_toggles_led_with_debounce() {
    let mut sim = boot_to_main(LAB3_EXP2);
    sim.set_gpio_pin("GPIOC", 15, false).unwrap();
    run_for(&mut sim, 20 * MS);
    assert!(!pin(&sim, PB, 0), "LED starts off");
    // press: a bounce shorter than the debounce delay must not count twice
    sim.set_gpio_pin("GPIOC", 15, true).unwrap();
    run_for(&mut sim, 200);
    sim.set_gpio_pin("GPIOC", 15, false).unwrap();
    run_for(&mut sim, 200);
    sim.set_gpio_pin("GPIOC", 15, true).unwrap();
    run_for(&mut sim, 20 * MS);
    assert!(pin(&sim, PB, 0), "first press turns the LED on");
    sim.set_gpio_pin("GPIOC", 15, false).unwrap();
    run_for(&mut sim, 20 * MS);
    assert!(pin(&sim, PB, 0), "release keeps it on");
    sim.set_gpio_pin("GPIOC", 15, true).unwrap();
    run_for(&mut sim, 20 * MS);
    assert!(!pin(&sim, PB, 0), "second press turns it off");
}

// ---------------------------------------------------------------------------------------
// Lab 4: SysTick
// ---------------------------------------------------------------------------------------

const LAB4_SYSTICK_BLINK: &str = r#"
RCC_APB2ENR EQU 0x40021018
GPIOC_CRH   EQU 0x40011004
GPIOC_ODR   EQU 0x4001100C
SYST_CSR    EQU 0xE000E010
SYST_RVR    EQU 0xE000E014
SYST_CVR    EQU 0xE000E018
DELAY       EQU 500

    AREA main, CODE, READONLY
    EXPORT _main
    ENTRY

_main PROC
    BL systick_init
    BL gpio_config
blink
    LDR r0, =GPIOC_ODR
    MOV r1, #(1 << 15)
    STR r1, [r0]                    ; LED on
    MOV r0, #DELAY
    BL delay_t
    LDR r0, =GPIOC_ODR
    MOVS r1, #0
    STR r1, [r0]                    ; LED off
    MOV r0, #DELAY
    BL delay_t
    B blink
    ENDP

systick_init PROC
    LDR r0, =SYST_RVR
    LDR r1, =72000-1                ; 1 ms at 72 MHz
    STR r1, [r0]
    LDR r0, =SYST_CVR
    MOVS r1, #0
    STR r1, [r0]
    LDR r0, =SYST_CSR
    MOVS r1, #5                     ; ENABLE | CLKSOURCE=processor
    STR r1, [r0]
    BX lr
    ENDP

delay_ms PROC
    LDR r1, =SYST_CSR
wait_flag
    LDR r2, [r1]
    TST r2, #0x10000                ; COUNTFLAG (cleared by the read)
    BEQ wait_flag
    BX lr
    ENDP

delay_t PROC
    PUSH {r4, lr}
    MOV r4, r0
delay_loop
    CBZ r4, delay_done
    BL delay_ms
    SUBS r4, r4, #1
    B delay_loop
delay_done
    POP {r4, pc}
    ENDP

gpio_config PROC
    LDR r0, =RCC_APB2ENR
    LDR r1, [r0]
    ORR r1, r1, #(1 << 4)
    STR r1, [r0]
    LDR r0, =GPIOC_CRH
    LDR r1, [r0]
    BIC r1, r1, #(0xF << 28)
    ORR r1, r1, #(2 << 28)          ; PC15 output 2 MHz
    STR r1, [r0]
    BX lr
    ENDP
    END
"#;

#[test]
fn lab4_systick_1ms_delay_and_500ms_blink() {
    // Exp 1: one delay_ms() is one SysTick period of exactly 72000 cycles
    let mut sim = boot_to_main(&LAB4_SYSTICK_BLINK.replace("DELAY       EQU 500", "DELAY       EQU 5"));
    let e = edges(&mut sim, PC, 15, 4, 20 * MS);
    for w in e.windows(2).skip(1) {
        let d = w[1].0 - w[0].0;
        assert!(d.abs_diff(5 * MS) <= 30, "5 ms half period took {} cycles", d);
    }
    // Exp 2: 500 ms on / 500 ms off
    let mut sim = boot_to_main(LAB4_SYSTICK_BLINK);
    let e = edges(&mut sim, PC, 15, 3, 600 * MS);
    assert!(e[0].1, "first edge turns the LED on");
    for w in e.windows(2).skip(1) {
        let d = w[1].0 - w[0].0;
        assert!(d.abs_diff(500 * MS) <= 30, "half period {} cycles, expected {}", d, 500 * MS);
    }
    let st = sim.mem.peripherals.systick.clone();
    assert_eq!(st.load, 71_999);
}

// ---------------------------------------------------------------------------------------
// Lab 5: keypad and external interrupts
// ---------------------------------------------------------------------------------------

const LAB5_KEYPAD: &str = r#"
RCC_APB2ENR EQU 0x40021018
GPIOA_CRL   EQU 0x40010800
GPIOA_IDR   EQU 0x40010808
GPIOA_ODR   EQU 0x4001080C

    AREA keybuf, DATA, READWRITE
input_string SPACE 100

    AREA main, CODE, READONLY
    EXPORT _main
    ENTRY

_main PROC
    LDR r0, =RCC_APB2ENR
    LDR r1, [r0]
    ORR r1, r1, #(1 << 2)
    STR r1, [r0]
    LDR r0, =GPIOA_CRL
    LDR r1, =0x44443333             ; PA0-PA3 rows: outputs, PA4-PA6 columns: inputs
    STR r1, [r0]
    LDR r4, =GPIOA_IDR
    LDR r5, =GPIOA_ODR
    LDR r6, =input_string
    MOVS r7, #0                     ; input_index
scan
    LDR r1, [r5]
    BIC r1, r1, #0xF                ; all rows low
    STR r1, [r5]
    LDR r0, [r4]
    AND r0, r0, #0x70
    CMP r0, #0x70
    BEQ scan                        ; no key down
    MOVS r2, #0                     ; get_column
    TST r0, #0x10
    BEQ got_col
    MOVS r2, #1
    TST r0, #0x20
    BEQ got_col
    MOVS r2, #2
    TST r0, #0x40
    BEQ got_col
    B scan
got_col
    MOVS r3, #0                     ; get_row: drive one row low at a time
row_loop
    LDR r1, [r5]
    BIC r1, r1, #0xF
    MOVS r0, #1
    LSLS r0, r0, r3
    EOR r0, r0, #0xF
    ORR r1, r1, r0
    STR r1, [r5]
    LDR r0, [r4]
    AND r0, r0, #0x70
    CMP r0, #0x70
    BNE got_row
    ADDS r3, r3, #1
    CMP r3, #4
    BLO row_loop
    B scan
got_row
    ADD r0, r3, r3, LSL #1          ; row * 3 + col
    ADD r0, r0, r2
    LDR r1, =key_map
    LDRB r0, [r1, r0]
    CMP r7, #99
    BHS wait_release
    STRB r0, [r6, r7]
    ADDS r7, r7, #1
    MOVS r1, #0
    STRB r1, [r6, r7]
wait_release
    LDR r1, [r5]
    BIC r1, r1, #0xF
    STR r1, [r5]
release_loop
    LDR r0, [r4]
    AND r0, r0, #0x70
    CMP r0, #0x70
    BNE release_loop
    B scan
    ENDP

key_map DCB "123456789*0#"
    END
"#;

#[test]
fn lab5_exp1_keypad_scan_captures_string() {
    let mut sim = boot_to_main(LAB5_KEYPAD);
    // the keypad module pulls the column lines up
    for c in 4..=6 {
        sim.set_gpio_pin("GPIOA", c, true).unwrap();
    }
    let buf = sim.elf.sym_by_name["input_string"];
    let keys = "50#17*";
    let layout = "123456789*0#";
    for (i, k) in keys.chars().enumerate() {
        let idx = layout.find(k).unwrap() as u8;
        let (row, col) = (idx / 3, idx % 3);
        let (r, c) = (format!("PA{}", row), format!("PA{}", 4 + col));
        sim.connect_pins(&r, &c, true).unwrap();
        run_until(&mut sim, 5 * MS, |s| s.mem.read8(buf + i as u32) != 0)
            .unwrap_or_else(|| panic!("key '{}' (row {}, col {}) not captured", k, row, col));
        sim.connect_pins(&r, &c, false).unwrap();
        run_for(&mut sim, MS);
    }
    let got: String = (0..keys.len() as u32).map(|i| sim.mem.read8(buf + i) as char).collect();
    assert_eq!(got, keys);
    assert_eq!(sim.mem.read8(buf + keys.len() as u32), 0, "string is NUL-terminated");
}

const LAB5_EXTI: &str = r#"
RCC_APB2ENR EQU 0x40021018
GPIOA_CRL   EQU 0x40010800
GPIOA_IDR   EQU 0x40010808
GPIOC_CRH   EQU 0x40011004
GPIOC_ODR   EQU 0x4001100C
AFIO_EXTICR1 EQU 0x40010008
EXTI_BASE   EQU 0x40010400
NVIC_ISER0  EQU 0xE000E100

    AREA main, CODE, READONLY
    EXPORT _main
    EXPORT EXTI0_IRQHandler
    ENTRY

_main PROC
    LDR r0, =RCC_APB2ENR
    LDR r1, [r0]
    ORR r1, r1, #((1 << 2) + (1 << 4) + 1)   ; GPIOA, GPIOC, AFIO
    STR r1, [r0]
    LDR r0, =GPIOA_CRL
    LDR r1, [r0]
    BIC r1, r1, #0xF
    ORR r1, r1, #4                  ; PA0 floating input
    STR r1, [r0]
    LDR r0, =GPIOC_CRH
    LDR r1, [r0]
    BIC r1, r1, #(0xF << 28)
    ORR r1, r1, #(3 << 28)          ; PC15 output push-pull
    STR r1, [r0]
    LDR r0, =AFIO_EXTICR1
    LDR r1, [r0]
    BIC r1, r1, #0xF                ; EXTI0 <- PA0
    STR r1, [r0]
    LDR r0, =EXTI_BASE
    LDR r1, [r0]
    ORR r1, r1, #1
    STR r1, [r0]                    ; IMR
    LDR r1, [r0, #8]
    ORR r1, r1, #1
    STR r1, [r0, #8]                ; RTSR
    LDR r1, [r0, #12]
    ORR r1, r1, #1
    STR r1, [r0, #12]               ; FTSR
    LDR r0, =NVIC_ISER0
    MOVS r1, #(1 << 6)              ; EXTI0_IRQn = 6
    STR r1, [r0]
idle
    B idle
    ENDP

EXTI0_IRQHandler PROC
    LDR r0, =EXTI_BASE
    LDR r1, [r0, #0x14]
    TST r1, #1
    BEQ exti_done
    MOVS r1, #1
    STR r1, [r0, #0x14]             ; clear PR0
    LDR r0, =GPIOA_IDR
    LDR r1, [r0]
    LDR r2, =GPIOC_ODR
    LDR r3, [r2]
    TST r1, #1
    ITE NE
    ORRNE r3, r3, #(1 << 15)
    BICEQ r3, r3, #(1 << 15)
    STR r3, [r2]
exti_done
    BX lr
    ENDP
    END
"#;

#[test]
fn lab5_exp2_exti0_follows_button_edges() {
    let mut sim = boot_to_main(LAB5_EXTI);
    let idle = sim.elf.sym_by_name["idle"] & !1;
    let isr = sim.elf.sym_by_name["EXTI0_IRQHandler"] & !1;
    run_until(&mut sim, 10_000, |s| s.cpu.r[15] == idle).expect("setup finished");
    assert!(sim.mem.peripherals.nvic.enabled[16 + 6], "EXTI0 enabled in NVIC");
    assert!(!pin(&sim, PC, 15));

    // rising edge -> handler within the 12-cycle exception entry (+ a few for the B loop)
    sim.set_gpio_pin("GPIOA", 0, true).unwrap();
    let t0 = sim.cpu.cycles;
    let t = run_until(&mut sim, 100, |s| s.cpu.r[15] == isr).expect("EXTI0 handler entered");
    assert!(t - t0 <= 16, "interrupt latency {} cycles", t - t0);
    assert_eq!(sim.cpu.xpsr & 0x1FF, 22, "IPSR = 16 + EXTI0_IRQn");
    run_until(&mut sim, 1000, |s| s.cpu.r[15] == idle).expect("returned to thread mode");
    assert!(pin(&sim, PC, 15), "LED on after the press");
    assert_eq!(sim.mem.peripherals.exti.pr, 0, "handler cleared PR");

    // falling edge -> LED off
    sim.set_gpio_pin("GPIOA", 0, false).unwrap();
    run_for(&mut sim, 1000);
    assert!(!pin(&sim, PC, 15), "LED off after the release");

    // an edge that arrives inside the handler after PR is cleared is not lost: it is
    // latched again and tail-chains into a second handler run
    sim.set_gpio_pin("GPIOA", 0, true).unwrap();
    run_until(&mut sim, 100, |s| s.cpu.r[15] == isr).unwrap();
    run_until(&mut sim, 200, |s| s.mem.peripherals.exti.pr == 0).expect("PR cleared");
    sim.set_gpio_pin("GPIOA", 0, false).unwrap();
    assert_eq!(sim.mem.peripherals.exti.pr, 1, "second edge latched");
    let mut entries = 0;
    let mut prev_pc = 0;
    run_until(&mut sim, 2000, |s| {
        if s.cpu.r[15] == isr && prev_pc != isr {
            entries += 1;
        }
        prev_pc = s.cpu.r[15];
        s.cpu.r[15] == idle
    })
    .expect("back to idle");
    assert_eq!(entries, 1, "tail-chained into the handler once more");
    assert!(!pin(&sim, PC, 15), "LED follows the final (low) level");
}

// ---------------------------------------------------------------------------------------
// Lab 6: TIM2
// ---------------------------------------------------------------------------------------

const LAB6_OPM: &str = r#"
RCC_APB2ENR EQU 0x40021018
RCC_APB1ENR EQU 0x4002101C
GPIOA_CRL   EQU 0x40010800
GPIOA_ODR   EQU 0x4001080C
TIM2_BASE   EQU 0x40000000
TIM_CR1     EQU 0x00
TIM_CNT     EQU 0x24
TIM_PSC     EQU 0x28
TIM_ARR     EQU 0x2C

    AREA main, CODE, READONLY
    EXPORT _main
    ENTRY

_main PROC
    LDR r0, =RCC_APB2ENR
    LDR r1, [r0]
    ORR r1, r1, #(1 << 2)
    STR r1, [r0]
    LDR r0, =GPIOA_CRL
    LDR r1, [r0]
    BIC r1, r1, #0xF
    ORR r1, r1, #3                  ; PA0 output
    STR r1, [r0]
    LDR r0, =RCC_APB1ENR
    LDR r1, [r0]
    ORR r1, r1, #1                  ; TIM2
    STR r1, [r0]
    LDR r0, =TIM2_BASE
    MOVS r1, #0
    STR r1, [r0, #TIM_CNT]
    LDR r1, =3600-1
    STR r1, [r0, #TIM_PSC]
    LDR r1, =10000-1
    STR r1, [r0, #TIM_ARR]
    LDR r1, [r0, #TIM_CR1]
    ORR r1, r1, #(1 << 3)           ; OPM
    STR r1, [r0, #TIM_CR1]
    LDR r2, =GPIOA_ODR
again
    LDR r1, [r0, #TIM_CR1]
    ORR r1, r1, #1                  ; start
    STR r1, [r0, #TIM_CR1]
wait_cen
    LDR r1, [r0, #TIM_CR1]
    TST r1, #1
    BNE wait_cen                    ; CEN clears at the update event
    LDR r1, [r2]
    EOR r1, r1, #1                  ; toggle the LED
    STR r1, [r2]
    B again
    ENDP
    END
"#;

#[test]
fn lab6_exp1_tim2_one_pulse_toggles_pa0() {
    let mut sim = boot_to_main(LAB6_OPM);
    // TIM2 runs from 2 x PCLK1 = 72 MHz. PSC is preloaded, so the first pulse still counts
    // with prescaler 1 (10000 cycles); afterwards each pulse is 3600 * 10000 / 72 MHz = 0.5 s.
    let e = edges(&mut sim, PA, 0, 3, 600 * MS);
    assert!(e[0].1);
    for w in e.windows(2) {
        let d = w[1].0 - w[0].0;
        assert!(d.abs_diff(500 * MS) <= 100, "pulse {} cycles, expected {}", d, 500 * MS);
    }
}

const LAB6_OC: &str = r#"
RCC_APB2ENR EQU 0x40021018
RCC_APB1ENR EQU 0x4002101C
GPIOA_CRL   EQU 0x40010800
TIM2_BASE   EQU 0x40000000

    AREA main, CODE, READONLY
    EXPORT _main
    ENTRY

_main PROC
    LDR r0, =RCC_APB1ENR
    LDR r1, [r0]
    ORR r1, r1, #1
    STR r1, [r0]
    LDR r0, =RCC_APB2ENR
    LDR r1, [r0]
    ORR r1, r1, #(1 << 2)
    STR r1, [r0]
    LDR r0, =GPIOA_CRL
    LDR r1, [r0]
    BIC r1, r1, #0xF
    ORR r1, r1, #11                 ; PA0 = TIM2_CH1, AF push-pull 50 MHz
    STR r1, [r0]
    LDR r0, =TIM2_BASE
    LDR r1, =3600-1
    STR r1, [r0, #0x28]             ; PSC
    LDR r1, =10000-1
    STR r1, [r0, #0x2C]             ; ARR
    LDR r1, =5000-1
    STR r1, [r0, #0x34]             ; CCR1
    LDR r1, [r0, #0x18]
    ORR r1, r1, #(3 << 4)           ; CCMR1 OC1M = 011 toggle
    STR r1, [r0, #0x18]
    LDR r1, [r0, #0x20]
    ORR r1, r1, #1                  ; CCER CC1E
    STR r1, [r0, #0x20]
    LDR r1, [r0]
    ORR r1, r1, #1                  ; CEN
    STR r1, [r0]
forever
    B forever
    ENDP
    END
"#;

#[test]
fn lab6_exp2_tim2_output_compare_toggles_pa0() {
    let mut sim = boot_to_main(LAB6_OC);
    // TIM2 clock = 2 x PCLK1 = 72 MHz; 72 MHz / 3600 = 20 kHz, ARR 9999 -> 0.5 s per
    // counter period, one toggle per period: PA0 changes every 0.5 s
    let forever = sim.elf.sym_by_name["forever"] & !1;
    // CEN is set by the STR just before the loop
    let t_cen = run_until(&mut sim, 1000, |s| s.cpu.r[15] == forever).unwrap();
    let e = edges(&mut sim, PA, 0, 4, 600 * MS);
    assert!(e[0].1, "toggle mode starts from a low OC1REF");
    // first match: CNT = 4999 with the reset prescaler (1 count per cycle)
    assert!((e[0].0 - t_cen).abs_diff(5000) <= 8, "first toggle {} cycles after CEN", e[0].0 - t_cen);
    // update at 10000 cycles loads PSC = 3599; next match 4999 counts of 3600 cycles later
    let second = e[0].0 + 5000 + 4999 * 3600;
    assert!(e[1].0.abs_diff(second) <= 8, "second toggle at {}, expected {}", e[1].0, second);
    for w in e.windows(2).skip(1) {
        assert_eq!(w[1].0 - w[0].0, 500 * MS, "steady state: one toggle per 0.5 s counter period");
    }
    let t = &sim.mem.peripherals.timers[1];
    assert_eq!((t.psc, t.arr, t.ccr[0]), (3599, 9999, 4999));
}

