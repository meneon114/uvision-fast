import { LAB_EXAMPLES } from './labs';

export interface ExampleSpec {
  id: string;
  title: string;
  lab: string;
  description: string;
  entryPath: string;
  files: Record<string, string>;
}

const STARTER_UVPROJX = `<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
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
          <Device>STM32F100RB</Device>
          <PackID>Keil.STM32F1xx_DFP.2.4.1</PackID>
          <OutputDirectory>.\\Objects\\</OutputDirectory>
          <OutputName>Starter_Lab</OutputName>
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
              <IROM1><StartAddress>0x08000000</StartAddress><Size>0x20000</Size></IROM1>
              <IRAM1><StartAddress>0x20000000</StartAddress><Size>0x2000</Size></IRAM1>
            </OnChipMemories>
          </ArmAdsMisc>
          <Aads><ClangAsOpt>1</ClangAsOpt></Aads>
        </TargetArmAds>
      </TargetOption>
      <Groups>
        <Group>
          <GroupName>Source Group 1</GroupName>
          <Files>
            <File>
              <FileName>main.s</FileName>
              <FileType>2</FileType>
              <FilePath>.\\main.s</FilePath>
            </File>
          </Files>
        </Group>
      </Groups>
    </Target>
  </Targets>
</Project>
`;

const STARTER_MAIN_S = `;;; Starter Lab - ARM Cortex-M3 Sum & SRAM Store (CSE 3108)
    PRESERVE8
    THUMB

    AREA    RESET, DATA, READONLY
    EXPORT  __Vectors
__Vectors
    DCD     0x20001000      ; Initial Stack Pointer
    DCD     Reset_Handler   ; Reset Vector
    ALIGN

    AREA    MYCODE, CODE, READONLY
    ENTRY
    EXPORT  Reset_Handler

Reset_Handler
    MOV     R0, #0          ; Accumulator = 0
    MOV     R1, #1          ; Counter = 1..10
    LDR     R4, =0x20000000 ; SRAM base address

LOOP
    ADD     R0, R0, R1      ; Sum += R1
    STR     R0, [R4], #4    ; Store running partial sum in SRAM
    ADD     R1, R1, #1
    CMP     R1, #11
    BNE     LOOP

    LDR     R2, =0x20000028 ; Store final sum at 0x20000028
    STR     R0, [R2, #0]
STOP
    B       STOP

    END
`;

const BITSHIFT_DEMO_S = `;;; Barrel Shifter & APSR Flags Demo
    PRESERVE8
    THUMB

    AREA    RESET, DATA, READONLY
    EXPORT  __Vectors
__Vectors
    DCD     0x20001000
    DCD     Reset_Handler
    ALIGN

    AREA    MYCODE, CODE, READONLY
    ENTRY
    EXPORT  Reset_Handler

Reset_Handler
    MOV     R1, #0xFF000000
    ORR     R1, #0xFF0000
    ORR     R1, #0xFF00     ; R1 = 0xFFFFFF00 (-256)
    MOV     R2, #4
    LSL     R3, R1, #4      ; R3 = 0xFFFFF000
    LSR     R4, R1, #4      ; R4 = 0x0FFFFFF0
    ASR     R5, R1, R2      ; R5 = 0xFFFFFFF0 (sign-extended)
    ROR     R6, R1, #3      ; R6 = 0x1FFFFFE0
    LDR     R0, =0x12345678
STOP
    B       STOP

    END
`;

const GPIO_UART_DEMO_S = `;;; STM32F1 RCC + GPIOA + USART1 Demo (View -> UART #1 & System Viewer)
    PRESERVE8
    THUMB

RCC_APB2ENR EQU 0x40021018
GPIOA_CRL   EQU 0x40010800
GPIOA_CRH   EQU 0x40010804
GPIOA_ODR   EQU 0x4001080C
USART1_SR   EQU 0x40013800
USART1_DR   EQU 0x40013804
USART1_BRR  EQU 0x40013808
USART1_CR1  EQU 0x4001380C

    AREA    RESET, DATA, READONLY
    EXPORT  __Vectors
__Vectors
    DCD     0x20001000
    DCD     Reset_Handler
    ALIGN

    AREA    MYCODE, CODE, READONLY
    ENTRY
    EXPORT  Reset_Handler

Reset_Handler
    ; Enable GPIOA (bit 2) & USART1 (bit 14) clocks
    LDR     R0, =RCC_APB2ENR
    MOV     R1, #0x4004
    STR     R1, [R0]

    ; Configure PA0..PA1 as output push-pull (CRL = 0x22)
    LDR     R0, =GPIOA_CRL
    MOV     R1, #0x22
    STR     R1, [R0]

    ; Drive PA0 & PA1 HIGH (check System Viewer -> GPIOA & Logic Analyzer)
    LDR     R0, =GPIOA_ODR
    MOV     R1, #0x0003
    STR     R1, [R0]

    ; PA9 = USART1_TX: alternate-function push-pull, 50 MHz (CRH[7:4] = 0xB)
    LDR     R0, =GPIOA_CRH
    LDR     R1, =0x444444B4
    STR     R1, [R0]

    ; 115200 baud from the 8 MHz reset clock (HSI): 8000000 / 115200 = 69.4 -> 0x45
    LDR     R0, =USART1_BRR
    MOV     R1, #0x45
    STR     R1, [R0]

    ; Enable the USART (UE, bit 13) and its transmitter (TE, bit 3)
    LDR     R0, =USART1_CR1
    MOV     R1, #0x2008
    STR     R1, [R0]

    ; Transmit banner on USART1 (check UART #1 tab)
    LDR     R5, =USART1_SR
    LDR     R2, =USART1_DR
    ADR     R3, MSG
TX_LOOP
    LDRB    R4, [R3], #1
    CMP     R4, #0
    BEQ     STOP
TX_WAIT
    LDR     R6, [R5]        ; wait for TXE (bit 7): the data register is free
    TST     R6, #0x80
    BEQ     TX_WAIT
    STR     R4, [R2]
    B       TX_LOOP

STOP
    B       STOP

MSG
    DCB     "HELLO STM32!", 13, 10, 0
    ALIGN

    END
`;

const TIMER_ARRAY_DEMO_S = `;;; STM32F1 TIM2 Counter & Memory Array Demo
    PRESERVE8
    THUMB

RCC_APB1ENR EQU 0x4002101C
TIM2_CR1    EQU 0x40000000
TIM2_PSC    EQU 0x40000028
TIM2_ARR    EQU 0x4000002C
TIM2_CNT    EQU 0x40000024

    AREA    RESET, DATA, READONLY
    EXPORT  __Vectors
__Vectors
    DCD     0x20001000
    DCD     Reset_Handler
    ALIGN

    AREA    MYCODE, CODE, READONLY
    ENTRY
    EXPORT  Reset_Handler

Reset_Handler
    ; Enable TIM2 clock and configure PSC=0, ARR=100, CEN=1
    LDR     R0, =RCC_APB1ENR
    MOV     R1, #1
    STR     R1, [R0]

    LDR     R0, =TIM2_PSC
    MOV     R1, #0
    STR     R1, [R0]

    LDR     R0, =TIM2_ARR
    MOV     R1, #100
    STR     R1, [R0]

    LDR     R0, =TIM2_CR1
    MOV     R1, #1
    STR     R1, [R0]

    ; Loop 8 times so TIM2 CNT advances in System Viewer -> TIM2
    MOV     R2, #8
    MOV     R3, #0
WORK
    ADD     R3, R3, R2
    SUBS    R2, R2, #1
    BNE     WORK

    LDR     R0, =TIM2_CNT
    LDR     R4, [R0]        ; Read current TIM2->CNT
    MOV     R5, #0x20000000
    STR     R3, [R5, #0]    ; Store 36 (0x24)
    STR     R4, [R5, #4]    ; Store TIM2->CNT
STOP
    B       STOP

    END
`;

export const STARTER_EXAMPLES: ExampleSpec[] = [
  ...LAB_EXAMPLES,
  {
    id: 'starter_lab',
    title: 'Sum and SRAM store',
    lab: 'Starter',
    description: 'Full Keil .uvprojx project summing 1..10 and writing partial sums into SRAM 0x20000000.',
    entryPath: '/Starter_Lab/Starter_Lab.uvprojx',
    files: {
      '/Starter_Lab/Starter_Lab.uvprojx': STARTER_UVPROJX,
      '/Starter_Lab/main.s': STARTER_MAIN_S,
    },
  },
  {
    id: 'gpio_uart',
    title: 'GPIO and USART1',
    lab: 'Starter',
    description: 'Drives PA0/PA1 LEDs in System Viewer -> GPIOA and prints "HELLO STM32!" to UART #1.',
    entryPath: '/Examples/GPIO_UART_Demo.s',
    files: {
      '/Examples/GPIO_UART_Demo.s': GPIO_UART_DEMO_S,
    },
  },
  {
    id: 'bitshift',
    title: 'Barrel shifter',
    lab: 'Starter',
    description: 'Demonstrates 32-bit Thumb-2 immediate shifts, APSR flags, and LDR =0x12345678 literal pools.',
    entryPath: '/Examples/BitShift_Demo.s',
    files: {
      '/Examples/BitShift_Demo.s': BITSHIFT_DEMO_S,
    },
  },
  {
    id: 'timer_tim2',
    title: 'TIM2 counter',
    lab: 'Starter',
    description: 'Enables TIM2 via RCC_APB1ENR, counts cycles, and snapshots TIM2->CNT into SRAM.',
    entryPath: '/Examples/Timer_Array_Demo.s',
    files: {
      '/Examples/Timer_Array_Demo.s': TIMER_ARRAY_DEMO_S,
    },
  },
];

export function collectSeedFiles(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const ex of STARTER_EXAMPLES) {
    for (const [p, content] of Object.entries(ex.files)) {
      out[p] = content;
    }
  }
  return out;
}
