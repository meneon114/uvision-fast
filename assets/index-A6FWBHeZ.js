var V=Object.defineProperty;var W=(c,e,t)=>e in c?V(c,e,{enumerable:!0,configurable:!0,writable:!0,value:t}):c[e]=t;var h=(c,e,t)=>W(c,typeof e!="symbol"?e+"":e,t);(function(){const e=document.createElement("link").relList;if(e&&e.supports&&e.supports("modulepreload"))return;for(const n of document.querySelectorAll('link[rel="modulepreload"]'))s(n);new MutationObserver(n=>{for(const o of n)if(o.type==="childList")for(const r of o.addedNodes)r.tagName==="LINK"&&r.rel==="modulepreload"&&s(r)}).observe(document,{childList:!0,subtree:!0});function t(n){const o={};return n.integrity&&(o.integrity=n.integrity),n.referrerPolicy&&(o.referrerPolicy=n.referrerPolicy),n.crossOrigin==="use-credentials"?o.credentials="include":n.crossOrigin==="anonymous"?o.credentials="omit":o.credentials="same-origin",o}function s(n){if(n.ep)return;n.ep=!0;const o=t(n);fetch(n.href,o)}})();const q="modulepreload",X=function(c,e){return new URL(c,e).href},C={},Y=function(e,t,s){let n=Promise.resolve();if(t&&t.length>0){let r=function(d){return Promise.all(d.map(l=>Promise.resolve(l).then(p=>({status:"fulfilled",value:p}),p=>({status:"rejected",reason:p}))))};const i=document.getElementsByTagName("link"),a=document.querySelector("meta[property=csp-nonce]"),u=(a==null?void 0:a.nonce)||(a==null?void 0:a.getAttribute("nonce"));n=r(t.map(d=>{if(d=X(d,s),d in C)return;C[d]=!0;const l=d.endsWith(".css"),p=l?'[rel="stylesheet"]':"";if(!!s)for(let b=i.length-1;b>=0;b--){const g=i[b];if(g.href===d&&(!l||g.rel==="stylesheet"))return}else if(document.querySelector(`link[href="${d}"]${p}`))return;const w=document.createElement("link");if(w.rel=l?"stylesheet":q,l||(w.as="script"),w.crossOrigin="",w.href=d,u&&w.setAttribute("nonce",u),document.head.appendChild(w),l)return new Promise((b,g)=>{w.addEventListener("load",b),w.addEventListener("error",()=>g(new Error(`Unable to preload CSS for ${d}`)))})}))}function o(r){const i=new Event("vite:preloadError",{cancelable:!0});if(i.payload=r,window.dispatchEvent(i),!i.defaultPrevented)throw r}return n.then(r=>{for(const i of r||[])i.status==="rejected"&&o(i.reason);return e().catch(o)})},z=`<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
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
`,K=`;;; Starter Lab - ARM Cortex-M3 Sum & SRAM Store (CSE 3108)
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
`,Q=`;;; Barrel Shifter & APSR Flags Demo
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
`,J=`;;; STM32F1 RCC + GPIOA + USART1 Demo (View -> UART #1 & System Viewer)
    PRESERVE8
    THUMB

RCC_APB2ENR EQU 0x40021018
GPIOA_CRL   EQU 0x40010800
GPIOA_ODR   EQU 0x4001080C
USART1_DR   EQU 0x40013804

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

    ; Transmit banner on USART1 (check UART #1 tab)
    LDR     R2, =USART1_DR
    ADR     R3, MSG
TX_LOOP
    LDRB    R4, [R3], #1
    CMP     R4, #0
    BEQ     STOP
    STR     R4, [R2]
    B       TX_LOOP

STOP
    B       STOP

MSG
    DCB     "HELLO STM32!", 13, 10, 0
    ALIGN

    END
`,Z=`;;; STM32F1 TIM2 Counter & Memory Array Demo
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
`,_=[{id:"starter_lab",title:"Starter Lab - Cortex-M3 Sum & SRAM (.uvprojx)",description:"Full Keil .uvprojx project summing 1..10 and writing partial sums into SRAM 0x20000000.",entryPath:"/Starter_Lab/Starter_Lab.uvprojx",files:{"/Starter_Lab/Starter_Lab.uvprojx":z,"/Starter_Lab/main.s":K}},{id:"gpio_uart",title:"STM32F1 GPIO + USART1 + Logic Analyzer",description:'Drives PA0/PA1 LEDs in System Viewer -> GPIOA and prints "HELLO STM32!" to UART #1.',entryPath:"/Examples/GPIO_UART_Demo.s",files:{"/Examples/GPIO_UART_Demo.s":J}},{id:"bitshift",title:"Barrel Shifter (LSL/LSR/ASR/ROR) & Literal Pool",description:"Demonstrates 32-bit Thumb-2 immediate shifts, APSR flags, and LDR =0x12345678 literal pools.",entryPath:"/Examples/BitShift_Demo.s",files:{"/Examples/BitShift_Demo.s":Q}},{id:"timer_tim2",title:"STM32F1 TIM2 Hardware Timer Counter",description:"Enables TIM2 via RCC_APB1ENR, counts cycles, and snapshots TIM2->CNT into SRAM.",entryPath:"/Examples/Timer_Array_Demo.s",files:{"/Examples/Timer_Array_Demo.s":Z}}];function F(){const c={};for(const e of _)for(const[t,s]of Object.entries(e.files))c[t]=s;return c}const tt="uvision-fast-vfs",et=1,y="files",A="meta",D="uvision_fast_pending_drafts_v1",N="uvision_fast_session_v1";function j(c){const e=c.replace(/\\/g,"/");return e.startsWith("/")?e:"/"+e}function I(){try{const c=localStorage.getItem(D);if(!c)return{};const e=JSON.parse(c);return e&&typeof e=="object"?e:{}}catch{return{}}}function st(c,e){try{const t=j(c),s=t.toLowerCase(),n=I();for(const o of Object.keys(n))o.toLowerCase()===s&&o!==t&&delete n[o];n[t]=e,localStorage.setItem(D,JSON.stringify(n))}catch{}}function rt(c){try{const e=j(c).toLowerCase(),t=I();let s=!1;for(const n of Object.keys(t))n.toLowerCase()===e&&(delete t[n],s=!0);if(!s)return;Object.keys(t).length===0?localStorage.removeItem(D):localStorage.setItem(D,JSON.stringify(t))}catch{}}function B(){try{const c=localStorage.getItem(N);return c?JSON.parse(c):null}catch{return null}}function ot(c){try{localStorage.setItem(N,JSON.stringify(c))}catch{}}class R{constructor(){h(this,"db",null);h(this,"cache",new Map);h(this,"keyMap",new Map)}static normDisp(e){const t=e.replace(/\\/g,"/");return t.startsWith("/")?t:"/"+t}static normKey(e){return R.normDisp(e).toLowerCase()}async init(){if(this.db=await new Promise((n,o)=>{const r=indexedDB.open(tt,et);r.onupgradeneeded=()=>{const i=r.result;i.objectStoreNames.contains(y)||i.createObjectStore(y),i.objectStoreNames.contains(A)||i.createObjectStore(A)},r.onsuccess=()=>n(r.result),r.onerror=()=>o(r.error||new Error("IndexedDB open failed"))}),await this.loadAllFromDb(),!await this.getMeta("seeded_v1")||this.cache.size===0){const n=F();for(const[o,r]of Object.entries(n))this.has(o)||await this.writeFile(o,r);await this.setMeta("seeded_v1",!0)}const t=I(),s=Object.entries(t);if(s.length>0){for(const[n,o]of s)await this.writeFile(n,String(o));try{localStorage.removeItem(D)}catch{}}}async loadAllFromDb(){this.db&&(this.cache.clear(),this.keyMap.clear(),await new Promise((e,t)=>{const o=this.db.transaction(y,"readonly").objectStore(y).openCursor();o.onsuccess=()=>{const r=o.result;if(r){const i=R.normDisp(String(r.key)),a=i.toLowerCase();this.keyMap.set(a,i),this.cache.set(a,String(r.value??"")),r.continue()}else e()},o.onerror=()=>t(o.error)}))}async getMeta(e){if(this.db)return new Promise(t=>{const n=this.db.transaction(A,"readonly").objectStore(A).get(e);n.onsuccess=()=>t(n.result),n.onerror=()=>t(void 0)})}async setMeta(e,t){this.db&&await new Promise((s,n)=>{const o=this.db.transaction(A,"readwrite");o.objectStore(A).put(t,e),o.oncomplete=()=>s(),o.onerror=()=>n(o.error)})}has(e){return this.cache.has(R.normKey(e))}readFile(e){const t=R.normKey(e),s=this.cache.get(t);if(s===void 0)throw new Error(`File not found in workspace: ${e}`);return{path:this.keyMap.get(t)||R.normDisp(e),content:s}}async writeFile(e,t){const s=R.normDisp(e),n=s.toLowerCase(),o=this.keyMap.get(n);return o&&o!==s&&this.db&&await this.deleteFromDb(o),this.keyMap.set(n,s),this.cache.set(n,t),this.db&&await new Promise((r,i)=>{const a=this.db.transaction(y,"readwrite");a.objectStore(y).put(t,s),a.oncomplete=()=>r(),a.onerror=()=>i(a.error)}),s}async deleteFromDb(e){this.db&&await new Promise(t=>{const s=this.db.transaction(y,"readwrite");s.objectStore(y).delete(e),s.oncomplete=()=>t(),s.onerror=()=>t()})}async deleteFile(e){const t=R.normKey(e),s=this.keyMap.get(t);this.cache.delete(t),this.keyMap.delete(t),s&&await this.deleteFromDb(s)}listFiles(){return Array.from(this.keyMap.values()).sort((e,t)=>e.localeCompare(t))}toRecord(){const e={};for(const[t,s]of this.keyMap.entries())e[s]=this.cache.get(t)??"";return e}async syncFromDump(e){for(const[t,s]of Object.entries(e)){const n=R.normKey(t);this.cache.get(n)!==s&&await this.writeFile(t,s)}}async resetExamples(){const e=F();for(const[t,s]of Object.entries(e))await this.writeFile(t,s)}}function E(c,e,t="application/octet-stream"){const s=typeof e=="string"?void 0:new Uint8Array(e),n=typeof e=="string"?new Blob([e],{type:t}):new Blob([s.buffer],{type:t}),o=URL.createObjectURL(n),r=document.createElement("a");r.href=o,r.download=c,document.body.appendChild(r),r.click(),setTimeout(()=>{r.remove(),URL.revokeObjectURL(o)},500)}function M(c){const e=atob(c),t=new Uint8Array(e.length);for(let s=0;s<e.length;s++)t[s]=e.charCodeAt(s);return t}class nt{constructor(){h(this,"instance",null);h(this,"encoder",new TextEncoder);h(this,"decoder",new TextDecoder);h(this,"initPromise",null)}async init(e="./uvision_core.wasm"){if(!this.instance)return this.initPromise?this.initPromise:(this.initPromise=(async()=>{const t=await fetch(e);if(!t.ok)throw new Error(`Failed loading ${e}: HTTP ${t.status}`);const s=await t.arrayBuffer(),{instance:n}=await WebAssembly.instantiate(s,{});this.instance=n})(),this.initPromise)}async rpc(e,t={}){await this.init();const s=this.instance.exports,n=e==="build.run"?performance.now():0,o=JSON.stringify({method:e,params:t}),r=this.encoder.encode(o),i=s.uv_alloc(r.length);new Uint8Array(s.memory.buffer,i,r.length).set(r);let a=0;try{a=s.uv_rpc(i,r.length)>>>0}finally{s.uv_free(i,r.length)}const u=s.uv_out_ptr()>>>0,d=new Uint8Array(s.memory.buffer,u,a),l=JSON.parse(this.decoder.decode(d));if(l.error)throw new Error(String(l.error));if(e==="build.run"&&l.result&&typeof l.result=="object"){const p=Math.max(1,Math.round(performance.now()-n));l.result.elapsed_ms=p,typeof l.result.log=="string"&&!l.result.log.includes(" ms]")&&(l.result.log+=` [${p} ms]`)}return l.result}}const it=121,at=31;class ct{constructor(e){h(this,"buf",[]);h(this,"reader");this.reader=e.readable.getReader()}async readBytes(e,t=2e3){const s=Date.now()+t;for(;this.buf.length<e;){const n=s-Date.now();if(n<=0)throw new Error(`Serial timeout waiting for ${e} byte(s)`);const o=new Promise(i=>setTimeout(()=>i({timeout:!0}),n)),r=await Promise.race([this.reader.read(),o]);if(r&&r.timeout)throw new Error("STM32 bootloader timed out. Ensure BOOT0=1, press RESET, and verify TX/RX wiring.");if(r.done)throw new Error("Serial port stream closed.");if(r.value)for(const i of r.value)this.buf.push(i)}return new Uint8Array(this.buf.splice(0,e))}async waitAck(e,t=2500){const[s]=await this.readBytes(1,t);if(s===at)throw new Error(`STM32 bootloader returned NACK at ${e}`);if(s!==it)throw new Error(`Unexpected bootloader byte 0x${s.toString(16)} at ${e} (expected 0x79 ACK)`)}release(){try{this.reader.releaseLock()}catch{}}}function U(){return typeof navigator<"u"&&"serial"in navigator}async function lt(c,e,t){if(!U())throw new Error("WebSerial API is not supported in this browser. Use Chromium / Chrome / Edge over HTTPS or localhost.");const s=navigator.serial;t({step:"Selecting serial port...",percent:2,log:"Requesting USB-UART adapter via WebSerial..."});const n=await s.requestPort();await n.open({baudRate:115200,dataBits:8,parity:"even",stopBits:1});const o=n.writable.getWriter(),r=new ct(n),i=async a=>{const u=a instanceof Uint8Array?a:new Uint8Array(a);await o.write(u)};try{t({step:"Syncing bootloader...",percent:8,log:"Sending 0x7F auto-baud sync byte (BOOT0=1)..."}),await i([127]),await r.waitAck("sync (0x7F)",3e3),await i([0,255]),await r.waitAck("GET command");const[a]=await r.readBytes(1),u=await r.readBytes(a+1);await r.waitAck("GET tail");const d=Array.from(u).includes(68);let l="unknown";try{await i([2,253]),await r.waitAck("GET ID");const[T]=await r.readBytes(1),x=await r.readBytes(T+1);await r.waitAck("GET ID tail"),l="0x"+Array.from(x).map(S=>S.toString(16).padStart(2,"0")).join("")}catch{}t({step:"Erasing flash...",percent:18,log:`Connected! STM32 PID=${l}. Erasing flash (${d?"Extended":"Standard"})...`}),d?(await i([68,187]),await r.waitAck("EXT_ERASE cmd"),await i([255,255,0]),await r.waitAck("EXT_ERASE mass",15e3)):(await i([67,188]),await r.waitAck("ERASE cmd"),await i([255,0]),await r.waitAck("ERASE mass",15e3));const p=c.length;let m=0;for(;m<p;){const T=c.subarray(m,Math.min(m+256,p)),x=T.length+3&-4,S=new Uint8Array(x);S.fill(255),S.set(T);const v=e+m>>>0,P=[v>>>24&255,v>>>16&255,v>>>8&255,v&255],$=P[0]^P[1]^P[2]^P[3];await i([49,206]),await r.waitAck("WRITE cmd"),await i([...P,$]),await r.waitAck(`WRITE addr 0x${v.toString(16)}`);const k=x-1;let L=k;for(const G of S)L^=G;const O=new Uint8Array(x+2);O[0]=k,O.set(S,1),O[x+1]=L&255,await i(O),await r.waitAck(`WRITE data @ 0x${v.toString(16)}`,5e3),m+=T.length;const H=20+Math.round(m/p*75);t({step:`Programming ${m}/${p} bytes...`,percent:H,log:`Wrote ${S.length} bytes @ 0x${v.toString(16).toUpperCase().padStart(8,"0")}`})}const w=e>>>0,b=[w>>>24&255,w>>>16&255,w>>>8&255,w&255],g=b[0]^b[1]^b[2]^b[3];await i([33,222]),await r.waitAck("GO cmd"),await i([...b,g]),await r.waitAck("GO addr"),t({step:"Completed!",percent:100,log:`Flashed ${p} bytes to 0x${e.toString(16).toUpperCase()} and jumped to application.`})}finally{try{o.releaseLock()}catch{}r.release();try{await n.close()}catch{}}}function f(c){return c.replace(/[&<>"']/g,e=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"})[e]||e)}class dt{constructor(e,t){h(this,"handlers",[]);h(this,"initPromise",null);h(this,"deferredInstallPrompt",null);h(this,"swReady",!1);h(this,"modalHost",null);h(this,"currentProjectRoot","/Starter_Lab");this.wasm=e,this.vfs=t,this.setupInstallAndSwListeners()}installAsGlobal(){const e=this;globalThis.__UVISION_WEB_BRIDGE__={isWeb:!0,postMessage(t){e.handleIncoming(t)},onMessage(t){e.handlers.push(t)},getSessionState(){return B()},setSessionState(t){ot(t)},saveDraftSync(t,s){st(t,s)},clearDraftSync(t){rt(t)}},this.setupDragAndDrop()}emit(e){for(const t of this.handlers)t(e)}async ensureInitialized(){return this.initPromise?this.initPromise:(this.initPromise=(async()=>{await this.vfs.init(),await this.wasm.init("./uvision_core.wasm"),await this.wasm.rpc("vfs.sync",{replace:!0,files:this.vfs.toRecord()})})(),this.initPromise)}ensureModalHost(){return this.modalHost||(this.modalHost=document.createElement("div"),this.modalHost.id="uv-web-modal-host",document.body.appendChild(this.modalHost)),this.modalHost}clearModal(){this.modalHost&&(this.modalHost.innerHTML="")}pushPwaState(){const e=!!this.deferredInstallPrompt,t=this.swReady;this.emit({type:"setPwaState",canInstall:e,offlineReady:t})}setupInstallAndSwListeners(){window.addEventListener("beforeinstallprompt",e=>{e.preventDefault(),this.deferredInstallPrompt=e,this.pushPwaState()}),window.addEventListener("appinstalled",()=>{this.deferredInstallPrompt=null,this.pushPwaState()}),"serviceWorker"in navigator&&(navigator.serviceWorker.addEventListener("controllerchange",()=>{this.swReady=!0,this.pushPwaState()}),window.addEventListener("load",async()=>{try{(await navigator.serviceWorker.register("./sw.js",{scope:"./"})).active&&(this.swReady=!0,this.pushPwaState()),await navigator.serviceWorker.ready,this.swReady=!0,this.pushPwaState()}catch{}}))}setupDragAndDrop(){window.addEventListener("dragover",e=>{e.preventDefault()}),window.addEventListener("drop",async e=>{var n;e.preventDefault();const t=(n=e.dataTransfer)==null?void 0:n.files;if(!t||t.length===0)return;await this.ensureInitialized();const s=await this.importFileList(t);s&&this.emit({type:"openTarget",path:s})})}async handleIncoming(e){if(!(!e||typeof e!="object")){if(e.type==="ready"){try{await this.ensureInitialized(),this.pushPwaState();const t=new URLSearchParams(location.search).get("open"),s=this.vfs.has("/Starter_Lab/Starter_Lab.uvprojx")?"/Starter_Lab/Starter_Lab.uvprojx":this.vfs.listFiles()[0];if(t&&this.vfs.has(t))this.emit({type:"openTarget",path:t});else{const n=B();n&&typeof n=="object"?this.emit({type:"restoreSession",session:n,fallbackPath:s}):s&&this.emit({type:"openTarget",path:s})}}catch(t){console.error("Initialization failed:",t)}return}if(e.type==="rpc"){const{reqId:t,method:s,params:n}=e;try{await this.ensureInitialized();const o=await this.wasm.rpc(s,n||{});if(s==="project.open"&&(o!=null&&o.root_dir)&&(this.currentProjectRoot=o.root_dir),s==="project.new"||s==="project.addFile"||s==="project.saveOptions"){const r=await this.wasm.rpc("vfs.dump");await this.vfs.syncFromDump(r),s==="project.new"&&(o!=null&&o.root_dir)&&(this.currentProjectRoot=o.root_dir)}this.emit({type:"rpcResponse",reqId:t,result:o})}catch(o){this.emit({type:"rpcResponse",reqId:t,error:(o==null?void 0:o.message)||String(o)})}return}if(e.type==="host"){const{reqId:t,action:s,payload:n}=e;try{await this.ensureInitialized();const o=await this.handleHostAction(s,n||{});this.emit({type:"hostResponse",reqId:t,result:o})}catch(o){this.emit({type:"hostResponse",reqId:t,error:(o==null?void 0:o.message)||String(o)})}}}}async handleHostAction(e,t){switch(e){case"readFile":{const s=String(t.path||"");return this.vfs.readFile(s)}case"writeFile":{const s=String(t.path||""),n=String(t.content??""),o=await this.vfs.writeFile(s,n);return await this.wasm.rpc("vfs.write",{path:o,content:n}),{ok:!0}}case"pickOpenProjectOrFile":return await this.showWorkspacePickerModal();case"openExamples":return await this.showExamplesModal();case"importLocalFile":return await this.promptLocalFileImport();case"pickExistingSourceFile":return await this.promptLocalFileImport(t.rootDir||this.currentProjectRoot);case"pickNewProjectDir":return"/Workspace/LabProject";case"promptText":return await this.showPromptModal(String(t.prompt||"Input"),String(t.value||""),String(t.placeHolder||""));case"downloadSource":{const s=String(t.name||"main.s"),n=String(t.content??"");return E(s,n,"text/plain;charset=utf-8"),{ok:!0}}case"downloadFirmware":return await this.showExportModal(String(t.outputName||"firmware"));case"flashWebHardware":return await this.showFlashModal(String(t.outputName||"firmware"),String(t.hwTarget||"stm32f1x"));case"installPwa":{if(!this.deferredInstallPrompt)return{installed:!1};this.deferredInstallPrompt.prompt();const s=await this.deferredInstallPrompt.userChoice;return(s==null?void 0:s.outcome)==="accepted"?(this.deferredInstallPrompt=null,this.pushPwaState(),{installed:!0}):{installed:!1}}case"pickPackFile":return null;default:throw new Error(`Unsupported web host action: ${e}`)}}async importFileList(e,t="/Imported"){const s=t.replace(/\\/g,"/").replace(/\/$/,"")||"/Imported";let n=null,o=null;for(const r of Array.from(e)){const i=await r.text(),a=`${s}/${r.name}`,u=await this.vfs.writeFile(a,i);await this.wasm.rpc("vfs.write",{path:u,content:i}),/\.uvprojx?$/i.test(r.name)&&!n?n=u:/\.(s|asm|c)$/i.test(r.name)&&!o&&(o=u)}return n||o}promptLocalFileImport(e="/Imported"){return new Promise(t=>{const s=document.createElement("input");s.type="file",s.multiple=!0,s.accept=".s,.asm,.c,.h,.uvprojx,.uvproj",s.style.display="none",document.body.appendChild(s);let n=!1;const o=r=>{n||(n=!0,s.remove(),t(r))};s.addEventListener("change",async()=>{if(s.files&&s.files.length>0){const r=await this.importFileList(s.files,e);o(r)}else o(null)}),s.addEventListener("cancel",()=>o(null)),s.click()})}showPromptModal(e,t,s){return new Promise(n=>{const o=this.ensureModalHost();o.innerHTML=`<div class="uv-modal-backdrop"><div class="uv-modal" style="width:420px">
        <div class="uv-modal-head"><span>uVision Fast</span><button class="uv-mini-btn" id="wp-x">×</button></div>
        <div class="uv-modal-body">
          <div style="font-size:11.5px;">${f(e)}</div>
          <input id="wp-val" type="text" value="${f(t)}" placeholder="${f(s)}" style="padding:4px 6px;border:1px solid #888;font-family:Consolas,monospace;font-size:12px;" />
        </div>
        <div class="uv-modal-foot">
          <button class="uv-btn primary" id="wp-ok">OK</button>
          <button class="uv-btn" id="wp-cancel">Cancel</button>
        </div>
      </div></div>`;const r=document.getElementById("wp-val");r.focus(),r.select();const i=a=>{this.clearModal(),n(a)};document.getElementById("wp-x").onclick=()=>i(void 0),document.getElementById("wp-cancel").onclick=()=>i(void 0),document.getElementById("wp-ok").onclick=()=>i(r.value),r.addEventListener("keydown",a=>{a.key==="Enter"&&i(r.value),a.key==="Escape"&&i(void 0)})})}showWorkspacePickerModal(){return new Promise(e=>{const t=this.ensureModalHost(),s=()=>{const o=this.vfs.listFiles().filter(r=>/\.(uvprojx?|s|asm|c|h)$/i.test(r)).map(r=>`<tr class="wb-pick-row" data-path="${f(r)}" style="cursor:pointer;">
              <td>${f(r)}</td>
              <td style="width:65px;text-align:center;">
                <button class="uv-mini-btn wb-open" data-path="${f(r)}">Open</button>
                <button class="uv-mini-btn wb-del" data-path="${f(r)}" title="Delete from IndexedDB">×</button>
              </td>
            </tr>`).join("");t.innerHTML=`<div class="uv-modal-backdrop"><div class="uv-modal" style="width:580px">
          <div class="uv-modal-head"><span>Open Workspace Project / File (Offline IndexedDB)</span><button class="uv-mini-btn" id="wb-x">×</button></div>
          <div class="uv-modal-body">
            <div style="font-size:11px;color:#444;">Saved locally in your browser (IndexedDB). Drag &amp; drop any <code>.s</code> or <code>.uvprojx</code> file anywhere onto the IDE to import.</div>
            <div style="max-height:260px;overflow:auto;border:1px solid #c0c0c0;background:#fff;">
              <table class="uv-watch-table">
                <thead><tr><th>Virtual Path</th><th style="width:65px;">Action</th></tr></thead>
                <tbody>${o||'<tr><td colspan="2">Workspace is empty.</td></tr>'}</tbody>
              </table>
            </div>
          </div>
          <div class="uv-modal-foot">
            <button class="uv-btn" id="wb-import">Import from Disk...</button>
            <button class="uv-btn" id="wb-reset">Restore Examples</button>
            <button class="uv-btn" id="wb-cancel">Cancel</button>
          </div>
        </div></div>`,document.getElementById("wb-x").onclick=()=>{this.clearModal(),e(null)},document.getElementById("wb-cancel").onclick=()=>{this.clearModal(),e(null)},document.getElementById("wb-import").onclick=async()=>{const r=await this.promptLocalFileImport();r&&(this.clearModal(),e(r))},document.getElementById("wb-reset").onclick=async()=>{await this.vfs.resetExamples(),await this.wasm.rpc("vfs.sync",{files:this.vfs.toRecord()}),s()},t.querySelectorAll(".wb-open").forEach(r=>{r.addEventListener("click",i=>{i.stopPropagation();const a=r.dataset.path;this.clearModal(),e(a)})}),t.querySelectorAll(".wb-pick-row").forEach(r=>{r.addEventListener("dblclick",()=>{const i=r.dataset.path;this.clearModal(),e(i)})}),t.querySelectorAll(".wb-del").forEach(r=>{r.addEventListener("click",async i=>{i.stopPropagation();const a=r.dataset.path;await this.vfs.deleteFile(a),await this.wasm.rpc("vfs.delete",{path:a}),s()})})};s()})}showExamplesModal(){return new Promise(e=>{const t=this.ensureModalHost(),s=_.map(o=>`<tr>
          <td><b>${f(o.title)}</b><br/><span style="color:#555;font-family:Segoe UI,sans-serif;">${f(o.description)}</span></td>
          <td style="width:72px;text-align:center;"><button class="uv-btn primary ex-open" data-id="${f(o.id)}">Open</button></td>
        </tr>`).join("");t.innerHTML=`<div class="uv-modal-backdrop"><div class="uv-modal" style="width:590px">
        <div class="uv-modal-head"><span>CSE 3108 Starter Examples (ARM Cortex-M3 / STM32F1)</span><button class="uv-mini-btn" id="ex-x">×</button></div>
        <div class="uv-modal-body">
          <table class="uv-watch-table"><thead><tr><th>Example Project</th><th>Action</th></tr></thead><tbody>${s}</tbody></table>
        </div>
        <div class="uv-modal-foot"><button class="uv-btn" id="ex-cancel">Close</button></div>
      </div></div>`;const n=o=>{this.clearModal(),e(o)};document.getElementById("ex-x").onclick=()=>n(null),document.getElementById("ex-cancel").onclick=()=>n(null),t.querySelectorAll(".ex-open").forEach(o=>{o.addEventListener("click",async()=>{const r=o.dataset.id,i=_.find(a=>a.id===r);if(!i)return n(null);for(const[a,u]of Object.entries(i.files))await this.vfs.writeFile(a,u),await this.wasm.rpc("vfs.write",{path:a,content:u});n(i.entryPath)})})})}async showExportModal(e){const t=await this.wasm.rpc("build.artifacts");if(!(t!=null&&t.has_artifacts))throw new Error("Build the project first (F7) before exporting artifacts.");const s=t.output_name||e||"firmware";return new Promise(n=>{const o=this.ensureModalHost();o.innerHTML=`<div class="uv-modal-backdrop"><div class="uv-modal" style="width:460px">
        <div class="uv-modal-head"><span>Export Built Firmware Artifacts</span><button class="uv-mini-btn" id="exp-x">×</button></div>
        <div class="uv-modal-body">
          <div style="font-size:11.5px;">Target output: <b>${f(s)}</b></div>
          <div style="display:flex;gap:8px;flex-wrap:wrap;margin-top:4px;">
            <button class="uv-btn primary" id="exp-hex">Download ${f(s)}.hex</button>
            <button class="uv-btn" id="exp-bin">Download ${f(s)}.bin</button>
            <button class="uv-btn" id="exp-axf">Download ${f(s)}.axf (ELF)</button>
          </div>
        </div>
        <div class="uv-modal-foot"><button class="uv-btn" id="exp-close">Close</button></div>
      </div></div>`,document.getElementById("exp-x").onclick=()=>{this.clearModal(),n({ok:!0})},document.getElementById("exp-close").onclick=()=>{this.clearModal(),n({ok:!0})},document.getElementById("exp-hex").onclick=()=>{t.hex_text&&E(`${s}.hex`,t.hex_text,"text/plain;charset=utf-8")},document.getElementById("exp-bin").onclick=()=>{t.bin_b64&&E(`${s}.bin`,M(t.bin_b64))},document.getElementById("exp-axf").onclick=()=>{t.elf_b64&&E(`${s}.axf`,M(t.elf_b64))}})}async showFlashModal(e,t){const s=await this.wasm.rpc("build.artifacts");if(!(s!=null&&s.has_artifacts)||!s.bin_b64)throw new Error("Build the project first (F7) before flashing hardware.");const n=s.output_name||e||"firmware",o=M(s.bin_b64),r=U();return new Promise(i=>{const a=this.ensureModalHost();a.innerHTML=`<div class="uv-modal-backdrop"><div class="uv-modal" style="width:560px">
        <div class="uv-modal-head"><span>Flash / Download to STM32 (${f(t)})</span><button class="uv-mini-btn" id="fl-x">×</button></div>
        <div class="uv-modal-body">
          <div style="font-size:11.5px;line-height:1.45;">
            <b>Option 1: WebSerial STM32 UART Bootloader (AN3155):</b><br/>
            Connect a USB-UART adapter to <b>USART1 (TX→PA10, RX→PA9)</b>, set <b>BOOT0 = 1</b>, press <b>RESET</b>, then click Flash below.<br/>
            <span style="color:#555;">(${o.length} bytes → Flash 0x08000000 • ${r?"WebSerial ready in this browser":"Use Chrome/Edge over HTTPS or localhost for WebSerial"})</span>
          </div>
          <div style="display:flex;gap:8px;align-items:center;">
            <button class="uv-btn primary" id="fl-go" ${r?"":"disabled"}>Connect Serial &amp; Flash STM32</button>
            <button class="uv-btn" id="fl-hex">Save .hex</button>
            <button class="uv-btn" id="fl-bin">Save .bin</button>
            <button class="uv-btn" id="fl-axf">Save .axf</button>
          </div>
          <div id="fl-bar-wrap" style="height:10px;border:1px solid #999;background:#fff;">
            <div id="fl-bar" style="width:0%;height:100%;background:#3399ff;"></div>
          </div>
          <div id="fl-log" class="uv-output-pre" style="max-height:140px;overflow:auto;border:1px solid #c0c0c0;background:#fff;">Ready. For ST-Link SWD flashing from desktop without UART bootloader, save .hex/.axf above or run the desktop CLI ("uvcli flash") / VS Code extension.</div>
        </div>
        <div class="uv-modal-foot"><button class="uv-btn" id="fl-close">Close</button></div>
      </div></div>`;const u=document.getElementById("fl-log"),d=document.getElementById("fl-bar"),l=p=>{u.textContent+=`
${p}`,u.scrollTop=u.scrollHeight};document.getElementById("fl-x").onclick=()=>{this.clearModal(),i({ok:!0})},document.getElementById("fl-close").onclick=()=>{this.clearModal(),i({ok:!0})},document.getElementById("fl-hex").onclick=()=>{s.hex_text&&E(`${n}.hex`,s.hex_text,"text/plain;charset=utf-8")},document.getElementById("fl-bin").onclick=()=>E(`${n}.bin`,o),document.getElementById("fl-axf").onclick=()=>{s.elf_b64&&E(`${n}.axf`,M(s.elf_b64))},document.getElementById("fl-go").onclick=async()=>{const p=document.getElementById("fl-go");p.disabled=!0,u.textContent="Starting STM32 WebSerial Bootloader sequence...",d.style.background="#3399ff",d.style.width="2%";try{await lt(o,134217728,m=>{d.style.width=`${m.percent}%`,l(`[${m.percent}%] ${m.log}`)}),d.style.background="#2e8b57"}catch(m){d.style.background="#cc0000",l(`Error: ${m.message||String(m)}`)}finally{p.disabled=!r}}})}}const ut=new nt,pt=new R,ht=new dt(ut,pt);ht.installAsGlobal();Y(()=>import("./main-B0SRjztB.js"),[],import.meta.url);
