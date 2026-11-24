use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::path::{Path, PathBuf};
use uvision_core::builder::{build_project_with_mode, ToolchainMode};
use uvision_core::elf::LoadedElf;
use uvision_core::hardware::{flash_via_openocd, HardwareTarget};
use uvision_core::project::UvProject;
use uvision_core::repl::{format_hex_dump, format_regs, format_step_summary, run_repl};
use uvision_core::sim::{self, Simulator};
use uvision_core::target::DebugTarget;
use uvision_core::toolchain::ToolchainInfo;
use uvision_core::{server};

#[derive(Parser)]
#[command(
    name = "uvcli",
    version,
    about = "uVision Fast CLI — Fast Keil ARM Cortex-M3 / STM32F1 build, simulator, hardware SWD & JSON-RPC server"
)]
struct Cli {
    #[arg(long, global = true, help = "Override Keil MDK installation directory")]
    keil_root: Option<String>,

    #[arg(long, global = true, help = "Override openocd.exe path")]
    openocd: Option<String>,

    #[arg(long, global = true, default_value = "auto", help = "Toolchain backend: auto, builtin, keil, or gnu")]
    toolchain: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show discovered Keil toolchain, CMSIS packs, and OpenOCD paths
    Info {
        #[arg(long)]
        json: bool,
    },
    /// Create a new Keil uVision-compatible project (.uvprojx)
    New {
        dir: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "STM32F100RB")]
        device: String,
        #[arg(long, default_value = "asm")]
        template: String,
    },
    /// Build a .uvprojx project or standalone .s/.c file using Keil armasm/armclang/armlink
    Build {
        path: PathBuf,
        #[arg(long)]
        rebuild: bool,
        #[arg(long)]
        json: bool,
    },
    /// Build (if needed) and run in the Cortex-M3 simulator, then print final registers/memory
    Run {
        path: PathBuf,
        #[arg(long, default_value_t = 100_000)]
        max_steps: usize,
        #[arg(long, default_value_t = 8_000_000)]
        clock_hz: u32,
        #[arg(long)]
        dump_addr: Option<String>,
        #[arg(long, default_value_t = 64)]
        dump_len: usize,
        #[arg(long)]
        no_build: bool,
        #[arg(long)]
        json: bool,
    },
    /// Start an interactive Keil-style debugger REPL (simulator or real ST-Link hardware)
    Debug {
        path: PathBuf,
        #[arg(long)]
        hw: bool,
        #[arg(long, default_value = "stlink")]
        interface: String,
        #[arg(long, default_value = "stm32f1x")]
        target: String,
        #[arg(long)]
        no_program: bool,
    },
    /// Flash firmware (.uvprojx, .s/.c, .hex, .axf, or .bin) to real STM32 hardware via OpenOCD
    Flash {
        path: PathBuf,
        #[arg(long, default_value = "stlink")]
        interface: String,
        #[arg(long, default_value = "stm32f1x")]
        target: String,
    },
    /// Manage CMSIS packs via Keil cpackget
    Pack {
        #[command(subcommand)]
        action: PackCmd,
    },
    /// Start newline-delimited JSON-RPC 2.0 server over stdio (for VS Code extension)
    Serve,
}

#[derive(Subcommand)]
enum PackCmd {
    /// List installed CMSIS packs
    List {
        #[arg(long)]
        json: bool,
    },
    /// Install a CMSIS pack file or Vendor.Pack.Version
    Add { spec: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let tc = ToolchainInfo::discover(cli.keil_root.as_deref(), cli.openocd.as_deref());
    let tc_mode = ToolchainMode::parse(&cli.toolchain);

    match cli.command {
        Commands::Info { json: as_json } => {
            if as_json {
                println!("{}", serde_json::to_string_pretty(&tc)?);
            } else {
                println!("Builtin Asm    : {}", tc.builtin_assembler);
                println!("Keil Root      : {}", tc.keil_root.as_deref().unwrap_or("NOT FOUND (using built-in assembler)"));
                println!("armasm         : {}", tc.armasm.as_deref().unwrap_or("NOT FOUND"));
                println!("armclang       : {}", tc.armclang.as_deref().unwrap_or("NOT FOUND"));
                println!("armlink        : {}", tc.armlink.as_deref().unwrap_or("NOT FOUND"));
                println!("fromelf        : {}", tc.fromelf.as_deref().unwrap_or("NOT FOUND"));
                println!("Arm GNU GCC    : {}", tc.arm_gcc.as_deref().unwrap_or("NOT FOUND"));
                println!("Packs Dir      : {}", tc.packs_dir.as_deref().unwrap_or("NOT FOUND"));
                println!("OpenOCD        : {}", tc.openocd.as_deref().unwrap_or("NOT FOUND"));
                println!("OpenOCD Scripts: {}", tc.openocd_scripts.as_deref().unwrap_or("NOT FOUND"));
                println!("Installed Packs: {}", tc.packs.len());
                for p in &tc.packs {
                    println!("  - {}", p.id);
                }
            }
        }

        Commands::New {
            dir,
            name,
            device,
            template,
        } => {
            let proj_name = name.unwrap_or_else(|| {
                dir.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Project")
                    .to_string()
            });
            let proj = UvProject::create_new(&dir, &proj_name, &device, &template)?;
            println!(
                "Created Keil project: {}",
                proj.project_file.as_deref().unwrap_or(&proj.root_dir)
            );
        }

        Commands::Build {
            path,
            rebuild,
            json: as_json,
        } => {
            let proj = UvProject::open(&path)?;
            let res = build_project_with_mode(&proj, &tc, rebuild, tc_mode)?;
            if as_json {
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                println!("{}", res.log);
            }
            if !res.success {
                std::process::exit(1);
            }
        }

        Commands::Run {
            path,
            max_steps,
            clock_hz,
            dump_addr,
            dump_len,
            no_build,
            json: as_json,
        } => {
            let axf = resolve_or_build_axf(&path, &tc, tc_mode, no_build)?;
            let elf = LoadedElf::load(&axf, &tc)?;
            let mut sim = Simulator::new(elf, clock_hz);
            let st = sim.run(max_steps, true)?;
            let mem = if let Some(addr_s) = dump_addr {
                let addr = sim::parse_num_u32(&addr_s).ok_or_else(|| anyhow!("Invalid dump address"))?;
                Some(sim.read_memory(addr, dump_len)?)
            } else {
                None
            };
            let periph = sim.peripherals()?;

            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "state": st,
                        "memory": mem,
                        "peripherals": periph
                    }))?
                );
            } else {
                println!("{}", format_step_summary(&st));
                println!("{}", format_regs(&st));
                if let Some(m) = mem {
                    println!("\nMemory @ 0x{:08X}:", m.addr);
                    println!("{}", format_hex_dump(m.addr, &m.bytes));
                }
                if let Some(p) = periph {
                    if !p.uart1_text.is_empty() {
                        println!("\nUSART1 TX: {}", p.uart1_text);
                    }
                }
            }
        }

        Commands::Debug {
            path,
            hw,
            interface,
            target,
            no_program,
        } => {
            let axf = resolve_or_build_axf(&path, &tc, tc_mode, false)?;
            let elf = LoadedElf::load(&axf, &tc)?;
            if hw {
                let mut hw_tgt = HardwareTarget::connect(&tc, elf.clone(), &interface, &target, !no_program)?;
                run_repl(&mut hw_tgt, &elf)?;
            } else {
                let mut sim = Simulator::new(elf.clone(), 8_000_000);
                run_repl(&mut sim, &elf)?;
            }
        }

        Commands::Flash {
            path,
            interface,
            target,
        } => {
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let fw = if matches!(ext.as_str(), "hex" | "axf" | "elf" | "bin") {
                path
            } else {
                let proj = UvProject::open(&path)?;
                let res = build_project_with_mode(&proj, &tc, false, tc_mode)?;
                println!("{}", res.log);
                if !res.success {
                    std::process::exit(1);
                }
                if proj.hex_path().exists() {
                    proj.hex_path()
                } else {
                    proj.axf_path()
                }
            };
            let log = flash_via_openocd(&tc, &fw, &interface, &target)?;
            println!("{}", log);
        }

        Commands::Pack { action } => match action {
            PackCmd::List { json: as_json } => {
                if as_json {
                    println!("{}", serde_json::to_string_pretty(&tc.packs)?);
                } else {
                    let txt = tc.pack_list_via_cpackget()?;
                    print!("{}", txt);
                }
            }
            PackCmd::Add { spec } => {
                let txt = tc.pack_add(&spec)?;
                print!("{}", txt);
            }
        },

        Commands::Serve => {
            server::run_json_rpc_server(cli.keil_root.as_deref(), cli.openocd.as_deref())?;
        }
    }

    Ok(())
}

fn resolve_or_build_axf(path: &Path, tc: &ToolchainInfo, tc_mode: ToolchainMode, no_build: bool) -> Result<PathBuf> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "axf" || ext == "elf" {
        return Ok(path.to_path_buf());
    }
    let proj = UvProject::open(path)?;
    let axf = proj.axf_path();
    if no_build && axf.exists() {
        return Ok(axf);
    }
    let res = build_project_with_mode(&proj, tc, false, tc_mode)?;
    if !res.success {
        eprintln!("{}", res.log);
        return Err(anyhow!("Build failed ({} error(s))", res.errors));
    }
    Ok(axf)
}
