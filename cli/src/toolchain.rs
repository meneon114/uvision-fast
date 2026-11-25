use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInfo {
    pub id: String,
    pub vendor: String,
    pub name: String,
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolchainInfo {
    pub builtin_assembler: bool,
    pub keil_root: Option<String>,
    pub armasm: Option<String>,
    pub armclang: Option<String>,
    pub armlink: Option<String>,
    pub fromelf: Option<String>,
    pub arm_gcc: Option<String>,
    pub arm_objcopy: Option<String>,
    pub cpackget: Option<String>,
    pub packs_dir: Option<String>,
    pub cmsis_include: Option<String>,
    pub stm32f1_include: Option<String>,
    pub stm32f1_startup_dir: Option<String>,
    pub openocd: Option<String>,
    pub openocd_scripts: Option<String>,
    pub packs: Vec<PackInfo>,
}

impl ToolchainInfo {
    pub fn discover(keil_override: Option<&str>, openocd_override: Option<&str>) -> Self {
        let keil_root = find_keil_root(keil_override);
        let bin_dir = keil_root.as_ref().map(|k| k.join("ARM").join("ARMCLANG").join("bin"));

        let armasm = bin_dir.as_ref().map(|b| b.join("armasm.exe")).filter(|p| p.exists());
        let armclang = bin_dir.as_ref().map(|b| b.join("armclang.exe")).filter(|p| p.exists());
        let armlink = bin_dir.as_ref().map(|b| b.join("armlink.exe")).filter(|p| p.exists());
        let fromelf = bin_dir.as_ref().map(|b| b.join("fromelf.exe")).filter(|p| p.exists());
        let (arm_gcc, arm_objcopy) = find_arm_gnu_toolchain();
        let cpackget = keil_root
            .as_ref()
            .map(|k| k.join("ARM").join("cmsis-toolbox").join("bin").join("cpackget.exe"))
            .filter(|p| p.exists());

        let packs_dir = keil_root.as_ref().and_then(|k| find_packs_dir(k));
        let cmsis_include = packs_dir.as_ref().and_then(|p| find_latest_subpath(p, &["ARM", "CMSIS"], &["CMSIS", "Core", "Include"]));
        let stm32f1_include = packs_dir.as_ref().and_then(|p| find_latest_subpath(p, &["Keil", "STM32F1xx_DFP"], &["Device", "Include"]));
        let stm32f1_startup_dir = packs_dir.as_ref().and_then(|p| find_latest_subpath(p, &["Keil", "STM32F1xx_DFP"], &["Device", "Source", "ARM"]));

        let openocd = find_openocd(openocd_override, keil_root.as_deref());
        let openocd_scripts = openocd.as_deref().and_then(find_openocd_scripts);
        let packs = packs_dir.as_deref().map(scan_packs).unwrap_or_default();

        Self {
            builtin_assembler: true,
            keil_root: keil_root.map(path_str),
            armasm: armasm.map(path_str),
            armclang: armclang.map(path_str),
            armlink: armlink.map(path_str),
            fromelf: fromelf.map(path_str),
            arm_gcc: arm_gcc.map(path_str),
            arm_objcopy: arm_objcopy.map(path_str),
            cpackget: cpackget.map(path_str),
            packs_dir: packs_dir.map(path_str),
            cmsis_include: cmsis_include.map(path_str),
            stm32f1_include: stm32f1_include.map(path_str),
            stm32f1_startup_dir: stm32f1_startup_dir.map(path_str),
            openocd: openocd.map(path_str),
            openocd_scripts: openocd_scripts.map(path_str),
            packs,
        }
    }

    pub fn require_armasm(&self) -> Result<&str> {
        self.armasm.as_deref().ok_or_else(|| anyhow!("Keil armasm.exe not found"))
    }

    pub fn require_armclang(&self) -> Result<&str> {
        self.armclang.as_deref().ok_or_else(|| anyhow!("Keil armclang.exe not found"))
    }

    pub fn require_armlink(&self) -> Result<&str> {
        self.armlink.as_deref().ok_or_else(|| anyhow!("Keil armlink.exe not found"))
    }

    pub fn require_fromelf(&self) -> Result<&str> {
        self.fromelf.as_deref().ok_or_else(|| anyhow!("Keil fromelf.exe not found"))
    }

    pub fn pack_list_via_cpackget(&self) -> Result<String> {
        let cg = self.cpackget.as_deref().ok_or_else(|| anyhow!("cpackget.exe not found"))?;
        let packs = self.packs_dir.as_deref().ok_or_else(|| anyhow!("Packs directory not found"))?;
        let out = Command::new(cg).args(["list", "--pack-root", packs]).output()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() {
            return Err(anyhow!("cpackget list failed: {}", text.trim()));
        }
        Ok(text)
    }

    pub fn pack_add(&self, pack_spec: &str) -> Result<String> {
        let cg = self.cpackget.as_deref().ok_or_else(|| anyhow!("cpackget.exe not found"))?;
        let packs = self.packs_dir.as_deref().ok_or_else(|| anyhow!("Packs directory not found"))?;
        let out = Command::new(cg)
            .args([
                "add",
                pack_spec,
                "--pack-root",
                packs,
                "--agree-embedded-license",
                "--no-dependencies",
            ])
            .output()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() {
            return Err(anyhow!("cpackget add failed: {}", text.trim()));
        }
        Ok(text)
    }
}

fn path_str(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

fn find_keil_root(override_path: Option<&str>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(o) = override_path.filter(|s| !s.trim().is_empty()) {
        let p = PathBuf::from(o.trim());
        // If explicit --keil-root is supplied and doesn't exist, honor the override (disables Keil)
        if p.join("ARM").join("ARMCLANG").join("bin").join("armclang.exe").exists() {
            return Some(p);
        }
        return None;
    }
    if let Ok(env_k) = std::env::var("KEIL_ROOT") {
        if !env_k.trim().is_empty() {
            candidates.push(PathBuf::from(env_k.trim()));
        }
    }
    candidates.push(PathBuf::from(r"E:\3-1\3108 - Computer Architecture Sessional\Software\Keil_v5"));
    candidates.push(PathBuf::from(r"C:\Keil_v5"));
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent();
        for _ in 0..6 {
            if let Some(dir) = cur {
                candidates.push(dir.join("Keil_v5"));
                cur = dir.parent();
            }
        }
    }
    candidates
        .into_iter()
        .find(|p| p.join("ARM").join("ARMCLANG").join("bin").join("armclang.exe").exists())
}

fn find_packs_dir(keil_root: &Path) -> Option<PathBuf> {
    let tools_ini = keil_root.join("TOOLS.INI");
    if let Ok(content) = std::fs::read_to_string(&tools_ini) {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("RTEPATH=") {
                let cleaned = rest.trim().trim_matches('"');
                let p = PathBuf::from(cleaned);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let fallback = keil_root.join("Packs");
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}

fn find_latest_subpath(packs_dir: &Path, pkg_prefix: &[&str], tail: &[&str]) -> Option<PathBuf> {
    let mut pkg_dir = packs_dir.to_path_buf();
    for seg in pkg_prefix {
        pkg_dir = pkg_dir.join(seg);
    }
    let entries = std::fs::read_dir(&pkg_dir).ok()?;
    let mut versions: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let mut candidate = e.path();
            for seg in tail {
                candidate = candidate.join(seg);
            }
            if candidate.exists() {
                Some((name, candidate))
            } else {
                None
            }
        })
        .collect();
    versions.sort_by(|a, b| compare_versions(&b.0, &a.0));
    versions.into_iter().next().map(|(_, p)| p)
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    pa.cmp(&pb).then_with(|| a.cmp(b))
}

fn scan_packs(packs_dir: &Path) -> Vec<PackInfo> {
    let mut out = Vec::new();
    let Ok(vendors) = std::fs::read_dir(packs_dir) else {
        return out;
    };
    for v in vendors.filter_map(|e| e.ok()) {
        let vname = v.file_name().to_string_lossy().into_owned();
        if vname.starts_with('.') || !v.path().is_dir() {
            continue;
        }
        let Ok(packs) = std::fs::read_dir(v.path()) else {
            continue;
        };
        for p in packs.filter_map(|e| e.ok()) {
            if !p.path().is_dir() {
                continue;
            }
            let pname = p.file_name().to_string_lossy().into_owned();
            let Ok(vers) = std::fs::read_dir(p.path()) else {
                continue;
            };
            for ver in vers.filter_map(|e| e.ok()) {
                if !ver.path().is_dir() {
                    continue;
                }
                let vstr = ver.file_name().to_string_lossy().into_owned();
                let pdsc = ver.path().join(format!("{}.{}.pdsc", vname, pname));
                if pdsc.exists() {
                    out.push(PackInfo {
                        id: format!("{}::{}@{}", vname, pname, vstr),
                        vendor: vname.clone(),
                        name: pname.clone(),
                        version: vstr,
                        path: path_str(ver.path()),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn find_arm_gnu_toolchain() -> (Option<PathBuf>, Option<PathBuf>) {
    let mut bin_candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            bin_candidates.push(dir.join("arm-gnu-toolchain").join("bin"));
        }
    }
    bin_candidates.push(PathBuf::from(r"E:\3-1\3108 - Computer Architecture Sessional\Software\arm-gnu-toolchain\bin"));
    if let Ok(entries) = std::fs::read_dir(r"C:\Program Files (x86)\Arm GNU Toolchain arm-none-eabi") {
        for e in entries.filter_map(|x| x.ok()) {
            bin_candidates.push(e.path().join("bin"));
        }
    }
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_env) {
            bin_candidates.push(dir);
        }
    }
    let gcc = bin_candidates
        .iter()
        .map(|d| d.join("arm-none-eabi-gcc.exe"))
        .find(|p| p.exists());
    let objcopy = gcc
        .as_ref()
        .and_then(|g| g.parent())
        .map(|d| d.join("arm-none-eabi-objcopy.exe"))
        .filter(|p| p.exists())
        .or_else(|| {
            bin_candidates
                .iter()
                .map(|d| d.join("arm-none-eabi-objcopy.exe"))
                .find(|p| p.exists())
        });
    (gcc, objcopy)
}

fn find_openocd(override_path: Option<&str>, keil_root: Option<&Path>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(o) = override_path.filter(|s| !s.trim().is_empty()) {
        candidates.push(PathBuf::from(o.trim()));
    }
    if let Ok(env_o) = std::env::var("OPENOCD_PATH") {
        if !env_o.trim().is_empty() {
            candidates.push(PathBuf::from(env_o.trim()));
        }
    }
    // Check bundled portable openocd right next to uvcli.exe FIRST (e.g. extension/bin/openocd/bin/openocd.exe)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("openocd").join("bin").join("openocd.exe"));
        }
        let mut cur = exe.parent();
        for _ in 0..6 {
            if let Some(dir) = cur {
                candidates.push(dir.join("openocd").join("bin").join("openocd.exe"));
                cur = dir.parent();
            }
        }
    }
    if let Some(k) = keil_root.and_then(|p| p.parent()) {
        candidates.push(k.join("openocd").join("bin").join("openocd.exe"));
    }
    candidates.push(PathBuf::from(r"E:\3-1\3108 - Computer Architecture Sessional\Software\openocd\bin\openocd.exe"));
    if let Some(found) = candidates.into_iter().find(|p| p.exists()) {
        return Some(found);
    }
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_env) {
            let p = dir.join("openocd.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn find_openocd_scripts(openocd_exe: &Path) -> Option<PathBuf> {
    let bin = openocd_exe.parent()?;
    let root = bin.parent()?;
    [
        root.join("openocd").join("scripts"),
        root.join("share").join("openocd").join("scripts"),
        root.join("scripts"),
    ]
    .into_iter()
    .find(|rel| rel.exists())
}
