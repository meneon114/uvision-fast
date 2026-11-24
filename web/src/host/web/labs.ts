import type { ExampleSpec } from './examples';

// CSE 3106 lab starters for the STM32F103C8 Blue Pill, as Keil projects.
export function uvprojx(name: string, sources: string[], device = 'STM32F103C8'): string {
  const files = sources
    .map(
      (f) => `            <File>
              <FileName>${f}</FileName>
              <FileType>2</FileType>
              <FilePath>.\\${f}</FilePath>
            </File>`,
    )
    .join('\n');
  return `<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<Project xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="project_projx.xsd">
  <SchemaVersion>2.1</SchemaVersion>
  <Header>### uVision Project, (C) Keil Software</Header>
  <Targets>
    <Target>
      <TargetName>Target 1</TargetName>
      <ToolsetNumber>0x4</ToolsetNumber>
      <ToolsetName>ARM-ADS</ToolsetName>
      <TargetOption>
        <TargetCommonOption>
          <Device>${device}</Device>
          <PackID>Keil.STM32F1xx_DFP.2.4.1</PackID>
          <OutputDirectory>.\\Objects\\</OutputDirectory>
          <OutputName>${name}</OutputName>
          <CreateExecutable>1</CreateExecutable>
          <CreateHexFile>1</CreateHexFile>
        </TargetCommonOption>
        <DebugOption>
          <Simulator>
            <UseSimulator>1</UseSimulator>
          </Simulator>
        </DebugOption>
        <TargetArmAds>
          <ArmAdsMisc>
            <uAC6>1</uAC6>
            <OnChipMemories>
              <IROM1><StartAddress>0x08000000</StartAddress><Size>0x10000</Size></IROM1>
              <IRAM1><StartAddress>0x20000000</StartAddress><Size>0x5000</Size></IRAM1>
            </OnChipMemories>
          </ArmAdsMisc>
          <Aads><ClangAsOpt>1</ClangAsOpt></Aads>
        </TargetArmAds>
      </TargetOption>
      <Groups>
        <Group>
          <GroupName>Source Group 1</GroupName>
          <Files>
${files}
          </Files>
        </Group>
      </Groups>
    </Target>
  </Targets>
</Project>
`;
}

const BLINKY_S = `; blinky.s  toggle PC13, the Blue Pill's on-board LED (Lab 3)
RCC_APB2ENR     EQU     0x40021018
GPIOC_CRH       EQU     0x40011004
GPIOC_ODR       EQU     0x4001100C

                PRESERVE8
                THUMB

                AREA    RESET, DATA, READONLY
                EXPORT  __Vectors
__Vectors       DCD     0x20005000
                DCD     Reset_Handler

                AREA    |.text|, CODE, READONLY
                ENTRY
                EXPORT  Reset_Handler
Reset_Handler
                LDR     R0, =RCC_APB2ENR
                LDR     R1, [R0]
                ORR     R1, R1, #0x10       ; IOPCEN
                STR     R1, [R0]
                LDR     R0, =GPIOC_CRH
                LDR     R1, =0x44344444     ; PC13 push-pull
                STR     R1, [R0]
                LDR     R2, =GPIOC_ODR
loop            LDR     R1, [R2]
                EOR     R1, R1, #0x2000     ; toggle PC13
                STR     R1, [R2]
                BL      delay
                B       loop

delay           LDR     R3, =20000
wait            SUBS    R3, R3, #1
                BNE     wait
                BX      LR

                END
`;

const BUTTON_S = `; button.s  PB0 LED follows the PC15 push button (Lab 3)
; PC15 is an input with pull-up: the button pulls it low when pressed.
RCC_APB2ENR     EQU     0x40021018
GPIOB_CRL       EQU     0x40010C00
GPIOB_ODR       EQU     0x40010C0C
GPIOC_CRH       EQU     0x40011004
GPIOC_IDR       EQU     0x40011008
GPIOC_ODR       EQU     0x4001100C

                PRESERVE8
                THUMB

                AREA    RESET, DATA, READONLY
                EXPORT  __Vectors
__Vectors       DCD     0x20005000
                DCD     Reset_Handler

                AREA    |.text|, CODE, READONLY
                ENTRY
                EXPORT  Reset_Handler
Reset_Handler
                LDR     R0, =RCC_APB2ENR
                MOV     R1, #0x18           ; IOPBEN | IOPCEN
                STR     R1, [R0]
                LDR     R0, =GPIOB_CRL
                LDR     R1, =0x44444443     ; PB0 push-pull output
                STR     R1, [R0]
                LDR     R0, =GPIOC_CRH
                LDR     R1, =0x84444444     ; PC15 input pull-up/down
                STR     R1, [R0]
                LDR     R0, =GPIOC_ODR
                MOV     R1, #0x8000         ; select pull-up on PC15
                STR     R1, [R0]
                LDR     R3, =GPIOC_IDR
                LDR     R5, =GPIOB_ODR
poll            LDR     R1, [R3]
                TST     R1, #0x8000
                BEQ     pressed
                MOV     R4, #0              ; released: LED off
                STR     R4, [R5]
                B       poll
pressed         MOV     R4, #1              ; pressed: LED on
                STR     R4, [R5]
                B       poll

                END
`;

const SYSTICK_S = `; systick.s  blink PC15 with SysTick millisecond delays (Lab 4)
RCC_APB2ENR     EQU     0x40021018
GPIOC_CRH       EQU     0x40011004
GPIOC_ODR       EQU     0x4001100C
STK_CTRL        EQU     0xE000E010
STK_LOAD        EQU     0xE000E014
STK_VAL         EQU     0xE000E018

                PRESERVE8
                THUMB

                AREA    RESET, DATA, READONLY
                EXPORT  __Vectors
__Vectors       DCD     0x20005000
                DCD     Reset_Handler

                AREA    |.text|, CODE, READONLY
                ENTRY
                EXPORT  Reset_Handler
Reset_Handler
                LDR     R0, =RCC_APB2ENR
                MOV     R1, #0x10           ; IOPCEN
                STR     R1, [R0]
                LDR     R0, =GPIOC_CRH
                LDR     R1, =0x34444444     ; PC15 push-pull output
                STR     R1, [R0]
                LDR     R0, =STK_LOAD
                LDR     R1, =7999           ; 1 ms at 8 MHz
                STR     R1, [R0]
                LDR     R0, =STK_VAL
                MOV     R1, #0
                STR     R1, [R0]
                LDR     R0, =STK_CTRL
                MOV     R1, #5              ; ENABLE | CLKSOURCE
                STR     R1, [R0]
                LDR     R2, =GPIOC_ODR
blink           LDR     R1, [R2]
                EOR     R1, R1, #0x8000     ; toggle PC15
                STR     R1, [R2]
                MOV     R0, #5
                BL      delay_ms
                B       blink

; R0 = milliseconds; waits for COUNTFLAG once per millisecond
delay_ms        LDR     R3, =STK_CTRL
tick            LDR     R1, [R3]
                TST     R1, #0x10000        ; COUNTFLAG
                BEQ     tick
                SUBS    R0, R0, #1
                BNE     tick
                BX      LR

                END
`;

const EXTI_S = `; exti.s  PA0 button on EXTI0 toggles the PC15 LED (Lab 5)
; PA0 is an input with pull-down; the button drives it high, rising edge interrupts.
RCC_APB2ENR     EQU     0x40021018
GPIOA_CRL       EQU     0x40010800
GPIOA_ODR       EQU     0x4001080C
GPIOC_CRH       EQU     0x40011004
GPIOC_ODR       EQU     0x4001100C
AFIO_EXTICR1    EQU     0x40010008
EXTI_IMR        EQU     0x40010400
EXTI_RTSR       EQU     0x40010408
EXTI_PR         EQU     0x40010414
NVIC_ISER0      EQU     0xE000E100

                PRESERVE8
                THUMB

                AREA    RESET, DATA, READONLY
                EXPORT  __Vectors
__Vectors       DCD     0x20005000          ; Top of stack
                DCD     Reset_Handler
                DCD     0                   ; NMI
                DCD     0                   ; HardFault
                DCD     0                   ; MemManage
                DCD     0                   ; BusFault
                DCD     0                   ; UsageFault
                DCD     0
                DCD     0
                DCD     0
                DCD     0
                DCD     0                   ; SVCall
                DCD     0                   ; DebugMon
                DCD     0
                DCD     0                   ; PendSV
                DCD     0                   ; SysTick
                DCD     0                   ; IRQ0 WWDG
                DCD     0                   ; IRQ1 PVD
                DCD     0                   ; IRQ2 TAMPER
                DCD     0                   ; IRQ3 RTC
                DCD     0                   ; IRQ4 FLASH
                DCD     0                   ; IRQ5 RCC
                DCD     EXTI0_IRQHandler    ; IRQ6 EXTI0

                AREA    |.text|, CODE, READONLY
                ENTRY
                EXPORT  Reset_Handler
                EXPORT  EXTI0_IRQHandler
Reset_Handler
                LDR     R0, =RCC_APB2ENR
                MOV     R1, #0x15           ; AFIOEN | IOPAEN | IOPCEN
                STR     R1, [R0]
                LDR     R0, =GPIOA_CRL
                LDR     R1, =0x44444448     ; PA0 input pull-up/down
                STR     R1, [R0]
                LDR     R0, =GPIOA_ODR
                MOV     R1, #0              ; select pull-down on PA0
                STR     R1, [R0]
                LDR     R0, =GPIOC_CRH
                LDR     R1, =0x34444444     ; PC15 push-pull output
                STR     R1, [R0]
                LDR     R0, =AFIO_EXTICR1
                MOV     R1, #0              ; EXTI0 <- PA0
                STR     R1, [R0]
                LDR     R0, =EXTI_RTSR
                MOV     R1, #1              ; rising edge
                STR     R1, [R0]
                LDR     R0, =EXTI_IMR
                MOV     R1, #1              ; unmask line 0
                STR     R1, [R0]
                LDR     R0, =NVIC_ISER0
                MOV     R1, #0x40           ; enable IRQ6
                STR     R1, [R0]
main_loop       B       main_loop

EXTI0_IRQHandler
                LDR     R0, =EXTI_PR
                MOV     R1, #1              ; clear pending line 0
                STR     R1, [R0]
                LDR     R0, =GPIOC_ODR
                LDR     R1, [R0]
                EOR     R1, R1, #0x8000     ; toggle PC15
                STR     R1, [R0]
                BX      LR

                END
`;

const KEYPAD_S = `; keypad.s  scan a 4x3 keypad (Lab 5)
; Rows PA0-PA3 are outputs, columns PA4-PA6 are inputs with pull-ups.
; A pressed key pulls its column low while its row is driven low.
; The key number (1-12, 0 = none) is kept in R7 and at 0x20000000.
RCC_APB2ENR     EQU     0x40021018
GPIOA_CRL       EQU     0x40010800
GPIOA_IDR       EQU     0x40010808
GPIOA_ODR       EQU     0x4001080C
KEY_RAM         EQU     0x20000000

                PRESERVE8
                THUMB

                AREA    RESET, DATA, READONLY
                EXPORT  __Vectors
__Vectors       DCD     0x20005000
                DCD     Reset_Handler

                AREA    |.text|, CODE, READONLY
                ENTRY
                EXPORT  Reset_Handler
Reset_Handler
                LDR     R0, =RCC_APB2ENR
                MOV     R1, #0x04           ; IOPAEN
                STR     R1, [R0]
                LDR     R0, =GPIOA_CRL
                LDR     R1, =0x48883333     ; PA0-3 outputs, PA4-6 pull-up inputs
                STR     R1, [R0]
                LDR     R4, =GPIOA_ODR
                LDR     R5, =GPIOA_IDR
                LDR     R6, =KEY_RAM
scan            MOV     R7, #0
                MOV     R2, #0              ; row index
row_loop        MOV     R1, #1
                LSL     R1, R1, R2
                EOR     R1, R1, #0x7F       ; drive this row low, others high, keep pull-ups
                STR     R1, [R4]
                LDR     R1, [R5]
                LSR     R1, R1, #4
                AND     R1, R1, #7
                CMP     R1, #7
                BNE     found
                ADD     R2, R2, #1
                CMP     R2, #4
                BNE     row_loop
                STR     R7, [R6]
                B       scan
found           MOV     R3, #3
                MUL     R7, R2, R3          ; R7 = row * 3
                TST     R1, #1
                BEQ     col0
                TST     R1, #2
                BEQ     col1
                ADD     R7, R7, #3          ; column 2
                B       store
col1            ADD     R7, R7, #2
                B       store
col0            ADD     R7, R7, #1
store           STR     R7, [R6]
                B       scan

                END
`;

const TIM2_OPM_S = `; tim2_opm.s  1 ms one-pulse on PA0 (TIM2_CH1) after a 1 ms delay (Lab 6)
RCC_APB1ENR     EQU     0x4002101C
RCC_APB2ENR     EQU     0x40021018
GPIOA_CRL       EQU     0x40010800
TIM2_BASE       EQU     0x40000000
TIM_CR1         EQU     0x00
TIM_CCMR1       EQU     0x18
TIM_CCER        EQU     0x20
TIM_PSC         EQU     0x28
TIM_ARR         EQU     0x2C
TIM_CCR1        EQU     0x34

                PRESERVE8
                THUMB

                AREA    RESET, DATA, READONLY
                EXPORT  __Vectors
__Vectors       DCD     0x20005000
                DCD     Reset_Handler

                AREA    |.text|, CODE, READONLY
                ENTRY
                EXPORT  Reset_Handler
Reset_Handler
                LDR     R0, =RCC_APB2ENR
                MOV     R1, #0x05           ; AFIOEN | IOPAEN
                STR     R1, [R0]
                LDR     R0, =RCC_APB1ENR
                MOV     R1, #0x01           ; TIM2EN
                STR     R1, [R0]
                LDR     R0, =GPIOA_CRL
                LDR     R1, =0x4444444B     ; PA0 alternate push-pull
                STR     R1, [R0]
                LDR     R0, =TIM2_BASE
                MOV     R1, #7              ; 8 MHz / 8 = 1 MHz tick
                STR     R1, [R0, #TIM_PSC]
                LDR     R1, =1999           ; period 2 ms
                STR     R1, [R0, #TIM_ARR]
                LDR     R1, =1000           ; pulse starts after 1 ms
                STR     R1, [R0, #TIM_CCR1]
                MOV     R1, #0x70           ; OC1M = PWM mode 2
                STR     R1, [R0, #TIM_CCMR1]
                MOV     R1, #1              ; CC1E
                STR     R1, [R0, #TIM_CCER]
                MOV     R1, #0x09           ; OPM | CEN
                STR     R1, [R0, #TIM_CR1]
wait_done       LDR     R1, [R0, #TIM_CR1]
                TST     R1, #1              ; CEN clears when the pulse ends
                BNE     wait_done
done            B       done

                END
`;

function project(id: string, title: string, lab: string, description: string, dir: string, name: string, file: string, src: string): ExampleSpec {
  const projPath = `/${dir}/${name}.uvprojx`;
  return {
    id,
    title,
    lab,
    description,
    entryPath: projPath,
    files: {
      [projPath]: uvprojx(name, [file]),
      [`/${dir}/${file}`]: src,
    },
  };
}

export const LAB_EXAMPLES: ExampleSpec[] = [
  project('blinky', 'Blinky', 'Lab 3', 'Toggles the PC13 on-board LED with a delay loop.', 'Blinky', 'blinky', 'blinky.s', BLINKY_S),
  project('button', 'Button and LED', 'Lab 3', 'PB0 LED follows the PC15 push button (active low).', 'Button_LED', 'button', 'button.s', BUTTON_S),
  project('systick', 'SysTick delay', 'Lab 4', 'Blinks PC15 using SysTick COUNTFLAG millisecond delays.', 'SysTick_Delay', 'systick', 'systick.s', SYSTICK_S),
  project('exti', 'EXTI0 interrupt', 'Lab 5', 'PA0 rising edge runs EXTI0_IRQHandler, which toggles PC15.', 'EXTI_Button', 'exti', 'exti.s', EXTI_S),
  project('keypad', '4x3 keypad', 'Lab 5', 'Scans rows PA0-PA3 and columns PA4-PA6; key number in R7.', 'Keypad', 'keypad', 'keypad.s', KEYPAD_S),
  project(
    'tim2_opm',
    'TIM2 one-pulse',
    'Lab 6',
    'TIM2 channel 1 output compare: one 1 ms pulse on PA0.',
    'TIM2_OnePulse',
    'tim2_opm',
    'tim2_opm.s',
    TIM2_OPM_S,
  ),
];
