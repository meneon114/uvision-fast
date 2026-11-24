//! Synthesised startup code, written in the same assembly dialect and assembled together with
//! the user's files. It stands in for what a Keil MDK project links in automatically:
//!
//! * `startup_stm32f10x_md.s`: the vector table at the start of flash, a weak `Reset_Handler`
//!   that calls `SystemInit` and then `__main`, and weak `B .` default handlers;
//! * `system_stm32f10x.c`: `SystemInit` as in CMSIS (`SetSysClockTo72`): HSE 8 MHz x 9 =
//!   72 MHz SYSCLK, APB1 /2, flash 2 wait states + prefetch, VTOR = 0x08000000;
//! * the C library's `__main`: scatter-load (copy RW data from flash, zero ZI) then the user
//!   entry point.
//!
//! All symbols are WEAK, so a user definition (EXPORTed, like with armlink) replaces them.

/// Core exception handler names for vectors 2..15 (`None` = reserved, 0).
pub const CORE_VECTORS: [Option<&str>; 14] = [
    Some("NMI_Handler"),
    Some("HardFault_Handler"),
    Some("MemManage_Handler"),
    Some("BusFault_Handler"),
    Some("UsageFault_Handler"),
    None,
    None,
    None,
    None,
    Some("SVC_Handler"),
    Some("DebugMon_Handler"),
    None,
    Some("PendSV_Handler"),
    Some("SysTick_Handler"),
];

/// STM32F10x IRQ names (connectivity-line superset used by the CMSIS headers, 60 vectors).
pub const IRQ_NAMES: [&str; 60] = [
    "WWDG",
    "PVD",
    "TAMPER",
    "RTC",
    "FLASH",
    "RCC",
    "EXTI0",
    "EXTI1",
    "EXTI2",
    "EXTI3",
    "EXTI4",
    "DMA1_Channel1",
    "DMA1_Channel2",
    "DMA1_Channel3",
    "DMA1_Channel4",
    "DMA1_Channel5",
    "DMA1_Channel6",
    "DMA1_Channel7",
    "ADC1_2",
    "USB_HP_CAN1_TX",
    "USB_LP_CAN1_RX0",
    "CAN1_RX1",
    "CAN1_SCE",
    "EXTI9_5",
    "TIM1_BRK",
    "TIM1_UP",
    "TIM1_TRG_COM",
    "TIM1_CC",
    "TIM2",
    "TIM3",
    "TIM4",
    "I2C1_EV",
    "I2C1_ER",
    "I2C2_EV",
    "I2C2_ER",
    "SPI1",
    "SPI2",
    "USART1",
    "USART2",
    "USART3",
    "EXTI15_10",
    "RTCAlarm",
    "USBWakeUp",
    "TIM8_BRK",
    "TIM8_UP",
    "TIM8_TRG_COM",
    "TIM8_CC",
    "ADC3",
    "FSMC",
    "SDIO",
    "TIM5",
    "SPI3",
    "UART4",
    "UART5",
    "TIM6",
    "TIM7",
    "DMA2_Channel1",
    "DMA2_Channel2",
    "DMA2_Channel3",
    "DMA2_Channel4_5",
];

/// All handler names that appear in the vector table.
pub fn handler_names() -> Vec<String> {
    let mut v: Vec<String> = CORE_VECTORS.iter().flatten().map(|s| s.to_string()).collect();
    v.extend(IRQ_NAMES.iter().map(|n| format!("{}_IRQHandler", n)));
    v
}

/// What the startup file has to provide.
pub struct Plan {
    /// Emit the vector table + Reset_Handler + default handlers.
    pub vectors: bool,
    pub initial_sp: u32,
    /// Emit a weak SystemInit.
    pub system_init: bool,
    /// Emit `__main` (scatter-load, then call `__uvf_user_entry`).
    pub main: bool,
    /// Reset_Handler jumps to the user's own `__main` (no scatter-load), as Keil does.
    pub reset_to_user_main: bool,
}

pub fn generate(p: &Plan) -> String {
    let mut s = String::new();
    s.push_str("    THUMB\n");
    if p.vectors {
        s.push_str(&format!("__initial_sp EQU 0x{:08X}\n", p.initial_sp));
        s.push_str("    AREA RESET, DATA, READONLY\n    EXPORT __Vectors\n__Vectors\n");
        s.push_str("    DCD __initial_sp\n    DCD Reset_Handler\n");
        for v in CORE_VECTORS {
            match v {
                Some(n) => s.push_str(&format!("    DCD {}\n", n)),
                None => s.push_str("    DCD 0\n"),
            }
        }
        for n in IRQ_NAMES {
            s.push_str(&format!("    DCD {}_IRQHandler\n", n));
        }
        s.push_str("__Vectors_End\n");
    }
    s.push_str("    AREA |.text.uvf_startup|, CODE, READONLY\n");
    if p.vectors {
        s.push_str("    EXPORT Reset_Handler [WEAK]\nReset_Handler PROC\n");
        s.push_str("    LDR r0, =SystemInit\n    BLX r0\n");
        if p.reset_to_user_main {
            s.push_str("    LDR r0, =__uvf_user_entry\n");
        } else {
            s.push_str("    LDR r0, =__main\n");
        }
        s.push_str("    BX r0\n    ENDP\n");
        for n in handler_names() {
            s.push_str(&format!("    EXPORT {} [WEAK]\n{} PROC\n    B .\n    ENDP\n", n, n));
        }
    }
    if p.main {
        s.push_str(
            "    EXPORT __main [WEAK]
__main PROC
    LDR r0, =__uvf_rw_load
    LDR r1, =__uvf_rw_base
    LDR r2, =__uvf_rw_limit
__uvf_copy
    CMP r1, r2
    BHS __uvf_zero_init
    LDR r3, [r0], #4
    STR r3, [r1], #4
    B __uvf_copy
__uvf_zero_init
    LDR r2, =__uvf_zi_limit
    MOVS r3, #0
__uvf_zero
    CMP r1, r2
    BHS __uvf_call
    STR r3, [r1], #4
    B __uvf_zero
__uvf_call
    LDR r0, =__uvf_user_entry
    BLX r0
__uvf_exit
    B __uvf_exit
    ENDP
",
        );
    }
    if p.system_init {
        s.push_str(
            "    EXPORT SystemInit [WEAK]
SystemInit PROC
    LDR r0, =0x40021000
    LDR r1, [r0]
    ORR r1, r1, #1
    STR r1, [r0]
    LDR r1, [r0, #4]
    LDR r2, =0xF8FF0000
    AND r1, r1, r2
    STR r1, [r0, #4]
    LDR r1, [r0]
    LDR r2, =0xFEF6FFFF
    AND r1, r1, r2
    STR r1, [r0]
    LDR r1, [r0]
    BIC r1, r1, #0x40000
    STR r1, [r0]
    LDR r1, [r0, #4]
    BIC r1, r1, #0x7F0000
    STR r1, [r0, #4]
    MOV r1, #0x9F0000
    STR r1, [r0, #8]
    LDR r1, [r0]
    ORR r1, r1, #0x10000
    STR r1, [r0]
    MOV r2, #0x500
__uvf_hse_wait
    LDR r1, [r0]
    TST r1, #0x20000
    BNE __uvf_hse_ok
    SUBS r2, r2, #1
    BNE __uvf_hse_wait
    B __uvf_vtor
__uvf_hse_ok
    LDR r3, =0x40022000
    LDR r1, [r3]
    ORR r1, r1, #0x10
    BIC r1, r1, #3
    ORR r1, r1, #2
    STR r1, [r3]
    LDR r1, [r0, #4]
    ORR r1, r1, #0x400
    BIC r1, r1, #0x3F0000
    ORR r1, r1, #0x1D0000
    STR r1, [r0, #4]
    LDR r1, [r0]
    ORR r1, r1, #0x1000000
    STR r1, [r0]
__uvf_pll_wait
    LDR r1, [r0]
    TST r1, #0x2000000
    BEQ __uvf_pll_wait
    LDR r1, [r0, #4]
    BIC r1, r1, #3
    ORR r1, r1, #2
    STR r1, [r0, #4]
__uvf_sws_wait
    LDR r1, [r0, #4]
    AND r1, r1, #0xC
    CMP r1, #8
    BNE __uvf_sws_wait
__uvf_vtor
    LDR r0, =0xE000ED08
    MOV r1, #0x08000000
    STR r1, [r0]
    BX lr
    ENDP
",
        );
    }
    s.push_str("    END\n");
    s
}
