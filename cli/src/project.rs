use anyhow::{anyhow, Result};
#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AsmSyntax {
    ArmAsm,
    GnuArmClang,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DebugBackendMode {
    Simulator,
    Hardware,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub name: String,
    pub path: String,
    pub abs_path: String,
    pub file_type: u8, // 1=C, 2=ASM, 5=Header/Other
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectGroup {
    pub name: String,
    pub files: Vec<ProjectFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetOptions {
    pub target_name: String,
    pub device: String,
    pub pack_id: String,
    pub output_name: String,
    pub output_dir: String,
    pub xtal_hz: u32,
    pub ro_base: u32,
    pub ro_size: u32,
    pub rw_base: u32,
    pub rw_size: u32,
    pub create_hex: bool,
    pub create_bin: bool,
    pub asm_syntax: AsmSyntax,
    pub c_defines: Vec<String>,
    pub c_includes: Vec<String>,
    pub c_misc: String,
    pub a_defines: Vec<String>,
    pub a_includes: Vec<String>,
    pub a_misc: String,
    pub scatter_file: Option<String>,
    pub ld_misc: String,
    pub debug_mode: DebugBackendMode,
    pub hw_interface: String,
    pub hw_target: String,
}

impl Default for TargetOptions {
    fn default() -> Self {
        Self {
            target_name: "Target 1".to_string(),
            device: "STM32F103C8".to_string(),
            pack_id: "Keil.STM32F1xx_DFP.2.4.1".to_string(),
            output_name: "firmware".to_string(),
            output_dir: "Objects".to_string(),
            xtal_hz: 8_000_000,
            ro_base: 0x0800_0000,
            ro_size: 0x0002_0000,
            rw_base: 0x2000_0000,
            rw_size: 0x0000_2000,
            create_hex: true,
            create_bin: false,
            asm_syntax: AsmSyntax::ArmAsm,
            c_defines: vec!["STM32F10X_MD_VL".to_string()],
            c_includes: Vec::new(),
            c_misc: String::new(),
            a_defines: Vec::new(),
            a_includes: Vec::new(),
            a_misc: String::new(),
            scatter_file: None,
            ld_misc: String::new(),
            debug_mode: DebugBackendMode::Simulator,
            hw_interface: "stlink".to_string(),
            hw_target: "stm32f1x".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UvProject {
    pub project_file: Option<String>,
    pub root_dir: String,
    pub is_standalone: bool,
    pub options: TargetOptions,
    pub groups: Vec<ProjectGroup>,
}

impl UvProject {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(input: &Path) -> Result<Self> {
        let abs = clean_canon(input);
        if abs.is_dir() {
            if let Some(uv) = find_uvprojx_in_dir(&abs) {
                return Self::parse_uvprojx(&uv);
            }
            return Self::from_folder(&abs);
        }

        let ext = abs
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "uvprojx" || ext == "uvproj" {
            return Self::parse_uvprojx(&abs);
        }
        if matches!(ext.as_str(), "s" | "asm" | "c") {
            return Self::from_standalone_file(&abs);
        }

        Err(anyhow!("Unsupported project or source file: {}", abs.display()))
    }

    pub fn from_virtual_file(path: &str) -> Self {
        let norm = norm_slashes(path);
        let (root_dir, file_name) = match norm.rsplit_once('/') {
            Some((dir, f)) => (if dir.is_empty() { "/" } else { dir }.to_string(), f.to_string()),
            None => ("/Workspace".to_string(), norm.clone()),
        };
        let stem = match file_name.rsplit_once('.') {
            Some((s, _)) => s.to_string(),
            None => file_name.clone(),
        };
        let ext = file_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        let file_type = match ext.as_str() {
            "c" => 1,
            "s" | "asm" => 2,
            _ => 5,
        };

        let options = TargetOptions { output_name: stem, output_dir: "Objects".to_string(), ..Default::default() };

        Self {
            project_file: None,
            root_dir,
            is_standalone: true,
            options,
            groups: vec![ProjectGroup {
                name: "Source Group 1".to_string(),
                files: vec![ProjectFile {
                    name: file_name.clone(),
                    path: file_name,
                    abs_path: norm,
                    file_type,
                }],
            }],
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_standalone_file(file: &Path) -> Result<Self> {
        let abs = clean_canon(file);
        let root_dir = abs
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let file_name = abs
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("main.s")
            .to_string();
        let stem = abs
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("firmware")
            .to_string();

        let ext = abs
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let file_type = match ext.as_str() {
            "c" => 1,
            "s" | "asm" => 2,
            _ => 5,
        };

        let options = TargetOptions { output_name: stem, output_dir: ".uvcli_build".to_string(), ..Default::default() };

        Ok(Self {
            project_file: None,
            root_dir: root_dir.to_string_lossy().into_owned(),
            is_standalone: true,
            options,
            groups: vec![ProjectGroup {
                name: "Source Group 1".to_string(),
                files: vec![ProjectFile {
                    name: file_name.clone(),
                    path: file_name,
                    abs_path: abs.to_string_lossy().into_owned(),
                    file_type,
                }],
            }],
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_folder(dir: &Path) -> Result<Self> {
        let abs_dir = clean_canon(dir);
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&abs_dir)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_file() {
                let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
                let ft = match ext.as_str() {
                    "c" => 1,
                    "s" | "asm" => 2,
                    _ => continue,
                };
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                files.push(ProjectFile {
                    name: name.clone(),
                    path: name,
                    abs_path: p.to_string_lossy().into_owned(),
                    file_type: ft,
                });
            }
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        if files.is_empty() {
            return Err(anyhow!("No .uvprojx, .s, or .c files found in {}", abs_dir.display()));
        }
        let options = TargetOptions {
            output_name: abs_dir.file_name().and_then(|s| s.to_str()).unwrap_or("firmware").to_string(),
            output_dir: ".uvcli_build".to_string(),
            ..Default::default()
        };

        Ok(Self {
            project_file: None,
            root_dir: abs_dir.to_string_lossy().into_owned(),
            is_standalone: true,
            options,
            groups: vec![ProjectGroup {
                name: "Source Group 1".to_string(),
                files,
            }],
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn parse_uvprojx(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed reading {}", path.display()))?;
        let root_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mut proj = Self::parse_uvprojx_str(
            &content,
            &path.to_string_lossy(),
            &root_dir.to_string_lossy(),
        )?;
        for g in &mut proj.groups {
            for f in &mut g.files {
                f.abs_path = resolve_rel(&root_dir, &f.path).to_string_lossy().into_owned();
            }
        }
        Ok(proj)
    }

    pub fn parse_uvprojx_str(content: &str, project_path: &str, root_dir: &str) -> Result<Self> {
        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        let proj_file_name = project_path.replace('\\', "/");
        let stem = proj_file_name
            .rsplit('/')
            .next()
            .unwrap_or("firmware")
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or("firmware")
            .to_string();

        let mut options = TargetOptions { output_name: stem, ..Default::default() };

        let mut groups: Vec<ProjectGroup> = Vec::new();
        let mut stack: Vec<String> = Vec::new();
        let mut in_first_target = false;
        let mut done_first_target = false;
        let mut cur_group: Option<ProjectGroup> = None;
        let mut cur_file_name = String::new();
        let mut cur_file_type: u8 = 5;
        let mut cur_file_path = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                    if name == "Target" && !done_first_target {
                        in_first_target = true;
                    } else if in_first_target && name == "Group" {
                        cur_group = Some(ProjectGroup {
                            name: "Source Group 1".to_string(),
                            files: Vec::new(),
                        });
                    } else if in_first_target && name == "File" {
                        cur_file_name.clear();
                        cur_file_path.clear();
                        cur_file_type = 5;
                    }
                    stack.push(name);
                }
                Ok(Event::End(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                    if in_first_target && name == "File" && !cur_file_path.is_empty() {
                        if let Some(g) = cur_group.as_mut() {
                            let resolved = resolve_virtual_path(root_dir, &cur_file_path);
                            let fname = if cur_file_name.is_empty() {
                                cur_file_path
                                    .replace('\\', "/")
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(&cur_file_path)
                                    .to_string()
                            } else {
                                cur_file_name.clone()
                            };
                            g.files.push(ProjectFile {
                                name: fname,
                                path: cur_file_path.clone(),
                                abs_path: resolved,
                                file_type: cur_file_type,
                            });
                        }
                    } else if in_first_target && name == "Group" {
                        if let Some(g) = cur_group.take() {
                            groups.push(g);
                        }
                    } else if name == "Target" && in_first_target {
                        in_first_target = false;
                        done_first_target = true;
                    }
                    stack.pop();
                }
                Ok(Event::Text(e)) => {
                    if !in_first_target {
                        continue;
                    }
                    let text = e.unescape().map(|s| s.into_owned()).unwrap_or_default();
                    let cur = stack.last().map(|s| s.as_str()).unwrap_or("");
                    let parent = stack.iter().rev().nth(1).map(|s| s.as_str()).unwrap_or("");
                    let grand = stack.iter().rev().nth(2).map(|s| s.as_str()).unwrap_or("");

                    match (grand, parent, cur) {
                        (_, "Target", "TargetName") => options.target_name = text,
                        (_, "TargetCommonOption", "Device") => options.device = text,
                        (_, "TargetCommonOption", "PackID") => options.pack_id = text,
                        (_, "TargetCommonOption", "OutputDirectory") => {
                            let cleaned = text.trim_end_matches(&['\\', '/'][..]).to_string();
                            if !cleaned.is_empty() {
                                options.output_dir = cleaned;
                            }
                        }
                        (_, "TargetCommonOption", "OutputName") => {
                            if !text.is_empty() {
                                options.output_name = text;
                            }
                        }
                        (_, "TargetCommonOption", "CreateHexFile") => {
                            options.create_hex = text.trim() == "1";
                        }
                        (_, "Simulator", "UseSimulator") => {
                            options.debug_mode = if text.trim() == "1" {
                                DebugBackendMode::Simulator
                            } else {
                                DebugBackendMode::Hardware
                            };
                        }
                        ("Cads", "VariousControls", "Define") => {
                            options.c_defines = split_list(&text);
                        }
                        ("Cads", "VariousControls", "IncludePath") => {
                            options.c_includes = split_paths(&text);
                        }
                        ("Cads", "VariousControls", "MiscControls") => options.c_misc = text,
                        (_, "Aads", "ClangAsOpt") => {
                            // 1 = armasm (Arm syntax) or Auto Select in MDK; 2/3 = gnu/clang
                            options.asm_syntax = if text.trim() == "3" {
                                AsmSyntax::GnuArmClang
                            } else {
                                AsmSyntax::ArmAsm
                            };
                        }
                        ("Aads", "VariousControls", "Define") => {
                            options.a_defines = split_list(&text);
                        }
                        ("Aads", "VariousControls", "IncludePath") => {
                            options.a_includes = split_paths(&text);
                        }
                        ("Aads", "VariousControls", "MiscControls") => options.a_misc = text,
                        (_, "LDads", "ScatterFile") => {
                            if !text.is_empty() {
                                options.scatter_file = Some(text);
                            }
                        }
                        (_, "LDads", "Misc") => options.ld_misc = text,
                        (_, "OnChipMemories", _) => {}
                        (_, "IROM1", "StartAddress") => {
                            if let Some(v) = parse_hex_u32(&text) {
                                options.ro_base = v;
                            }
                        }
                        (_, "IROM1", "Size") => {
                            if let Some(v) = parse_hex_u32(&text) {
                                options.ro_size = v;
                            }
                        }
                        (_, "IRAM1", "StartAddress") => {
                            if let Some(v) = parse_hex_u32(&text) {
                                options.rw_base = v;
                            }
                        }
                        (_, "IRAM1", "Size") => {
                            if let Some(v) = parse_hex_u32(&text) {
                                options.rw_size = v;
                            }
                        }
                        (_, "Group", "GroupName") => {
                            if let Some(g) = cur_group.as_mut() {
                                g.name = text;
                            }
                        }
                        (_, "File", "FileName") => cur_file_name = text,
                        (_, "File", "FileType") => {
                            cur_file_type = text.trim().parse::<u8>().unwrap_or(5);
                        }
                        (_, "File", "FilePath") => cur_file_path = text,
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(anyhow!("XML parse error in {}: {}", project_path, e)),
                _ => {}
            }
            buf.clear();
        }

        if groups.is_empty() {
            groups.push(ProjectGroup {
                name: "Source Group 1".to_string(),
                files: Vec::new(),
            });
        }

        Ok(Self {
            project_file: Some(project_path.to_string()),
            root_dir: root_dir.to_string(),
            is_standalone: false,
            options,
            groups,
        })
    }

    pub fn new_virtual(
        dir: &str,
        name: &str,
        device: &str,
        template: &str,
    ) -> Result<(Self, Vec<(String, String)>)> {
        let base_dir = norm_slashes(dir).trim_end_matches('/').to_string();
        let root_dir = if base_dir.is_empty() {
            format!("/{}", name)
        } else {
            base_dir
        };
        let (src_name, src_type, src_body) = if template.eq_ignore_ascii_case("c") {
            ("main.c".to_string(), 1u8, DEFAULT_C_TEMPLATE.to_string())
        } else {
            ("main.s".to_string(), 2u8, DEFAULT_ASM_TEMPLATE.to_string())
        };

        let src_abs = format!("{}/{}", root_dir, src_name);
        let proj_abs = format!("{}/{}.uvprojx", root_dir, name);

        let options = TargetOptions { device: device.to_string(), output_name: name.to_string(), ..Default::default() };

        let proj = Self {
            project_file: Some(proj_abs.clone()),
            root_dir,
            is_standalone: false,
            options,
            groups: vec![ProjectGroup {
                name: "Source Group 1".to_string(),
                files: vec![ProjectFile {
                    name: src_name.clone(),
                    path: format!(".\\{}", src_name),
                    abs_path: src_abs.clone(),
                    file_type: src_type,
                }],
            }],
        };

        let xml = proj.to_uvprojx_xml()?;
        Ok((proj, vec![(src_abs, src_body), (proj_abs, xml)]))
    }

    pub fn add_virtual_file(
        &mut self,
        file_path: &str,
        create_template: Option<&str>,
        exists: bool,
    ) -> Result<VirtualFileAdded> {
        let abs = resolve_virtual_path(&self.root_dir, file_path);
        let ext = abs
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();

        let new_file_content = if !exists {
            let content = match create_template {
                Some("asm") => DEFAULT_ASM_TEMPLATE,
                Some("c") => DEFAULT_C_TEMPLATE,
                _ if ext == "s" || ext == "asm" => DEFAULT_ASM_TEMPLATE,
                _ if ext == "c" => DEFAULT_C_TEMPLATE,
                _ => "",
            };
            Some((abs.clone(), content.to_string()))
        } else {
            None
        };

        let file_type = match ext.as_str() {
            "c" => 1,
            "s" | "asm" => 2,
            _ => 5,
        };
        let fname = abs.rsplit('/').next().unwrap_or("file").to_string();
        let rel = make_virtual_rel(&self.root_dir, &abs);

        if self.groups.is_empty() {
            self.groups.push(ProjectGroup {
                name: "Source Group 1".to_string(),
                files: Vec::new(),
            });
        }
        if !self.groups[0]
            .files
            .iter()
            .any(|f| f.abs_path.eq_ignore_ascii_case(&abs))
        {
            self.groups[0].files.push(ProjectFile {
                name: fname.clone(),
                path: rel.clone(),
                abs_path: abs.clone(),
                file_type,
            });
        }

        let updated_xml = if self.project_file.is_some() {
            Some(self.to_uvprojx_xml()?)
        } else {
            None
        };

        Ok((
            ProjectFile {
                name: fname,
                path: rel,
                abs_path: abs,
                file_type,
            },
            updated_xml,
            new_file_content,
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_new(dir: &Path, name: &str, device: &str, template: &str) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        std::fs::create_dir_all(dir.join("Objects"))?;
        std::fs::create_dir_all(dir.join("Listings"))?;

        let (src_name, src_type, src_body) = if template.eq_ignore_ascii_case("c") {
            (
                "main.c".to_string(),
                1u8,
                DEFAULT_C_TEMPLATE.to_string(),
            )
        } else {
            (
                "main.s".to_string(),
                2u8,
                DEFAULT_ASM_TEMPLATE.to_string(),
            )
        };

        let src_path = dir.join(&src_name);
        if !src_path.exists() {
            std::fs::write(&src_path, src_body)?;
        }

        let proj_path = dir.join(format!("{}.uvprojx", name));
        let options = TargetOptions { device: device.to_string(), output_name: name.to_string(), ..Default::default() };

        let proj = Self {
            project_file: Some(proj_path.to_string_lossy().into_owned()),
            root_dir: dir.to_string_lossy().into_owned(),
            is_standalone: false,
            options,
            groups: vec![ProjectGroup {
                name: "Source Group 1".to_string(),
                files: vec![ProjectFile {
                    name: src_name.clone(),
                    path: format!(".\\{}", src_name),
                    abs_path: src_path.to_string_lossy().into_owned(),
                    file_type: src_type,
                }],
            }],
        };

        proj.save()?;
        Ok(proj)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_file(&mut self, file_path: &Path, create_template: Option<&str>) -> Result<ProjectFile> {
        let root = PathBuf::from(&self.root_dir);
        let abs_path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            root.join(file_path)
        };
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !abs_path.exists() {
            let ext = abs_path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
            let content = match create_template {
                Some("asm") => DEFAULT_ASM_TEMPLATE,
                Some("c") => DEFAULT_C_TEMPLATE,
                _ if ext == "s" || ext == "asm" => DEFAULT_ASM_TEMPLATE,
                _ if ext == "c" => DEFAULT_C_TEMPLATE,
                _ => "",
            };
            std::fs::write(&abs_path, content)?;
        }

        let ext = abs_path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        let file_type = match ext.as_str() {
            "c" => 1,
            "s" | "asm" => 2,
            _ => 5,
        };
        let fname = abs_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();
        let rel = make_rel_path(&root, &abs_path);

        if self.groups.is_empty() {
            self.groups.push(ProjectGroup {
                name: "Source Group 1".to_string(),
                files: Vec::new(),
            });
        }
        let abs_str = abs_path.to_string_lossy().into_owned();
        if !self.groups[0].files.iter().any(|f| f.abs_path.eq_ignore_ascii_case(&abs_str)) {
            self.groups[0].files.push(ProjectFile {
                name: fname.clone(),
                path: rel.clone(),
                abs_path: abs_str.clone(),
                file_type,
            });
        }
        if self.project_file.is_some() {
            self.save()?;
        }
        Ok(ProjectFile {
            name: fname,
            path: rel,
            abs_path: abs_str,
            file_type,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self) -> Result<()> {
        let Some(pf) = self.project_file.as_ref() else {
            return Ok(());
        };
        let xml = self.to_uvprojx_xml()?;
        std::fs::write(pf, xml)?;
        Ok(())
    }

    pub fn source_files(&self) -> Vec<&ProjectFile> {
        self.groups
            .iter()
            .flat_map(|g| g.files.iter())
            .filter(|f| f.file_type == 1 || f.file_type == 2)
            .collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn build_dir(&self) -> PathBuf {
        let root = PathBuf::from(&self.root_dir);
        let out = root.join(&self.options.output_dir);
        let _ = std::fs::create_dir_all(&out);
        out
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn axf_path(&self) -> PathBuf {
        self.build_dir().join(format!("{}.axf", self.options.output_name))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn hex_path(&self) -> PathBuf {
        self.build_dir().join(format!("{}.hex", self.options.output_name))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn bin_path(&self) -> PathBuf {
        self.build_dir().join(format!("{}.bin", self.options.output_name))
    }

    pub fn to_uvprojx_xml(&self) -> Result<String> {
        let mut w = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
        w.write_event(Event::Text(BytesText::from_escaped("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n")))?;

        let mut proj = BytesStart::new("Project");
        proj.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
        proj.push_attribute(("xsi:noNamespaceSchemaLocation", "project_projx.xsd"));
        w.write_event(Event::Start(proj))?;

        write_tag(&mut w, "SchemaVersion", "2.1")?;
        w.write_event(Event::Start(BytesStart::new("Header")))?;
        write_tag(&mut w, "###", "### uVision Project, (C) Keil Software")?;
        w.write_event(Event::End(BytesEnd::new("Header")))?;

        w.write_event(Event::Start(BytesStart::new("Targets")))?;
        w.write_event(Event::Start(BytesStart::new("Target")))?;
        write_tag(&mut w, "TargetName", &self.options.target_name)?;
        write_tag(&mut w, "ToolsetNumber", "0x4")?;
        write_tag(&mut w, "ToolsetName", "ARM-ADS")?;

        w.write_event(Event::Start(BytesStart::new("TargetOption")))?;
        w.write_event(Event::Start(BytesStart::new("TargetCommonOption")))?;
        write_tag(&mut w, "Device", &self.options.device)?;
        write_tag(&mut w, "PackID", &self.options.pack_id)?;
        write_tag(&mut w, "OutputDirectory", &format!(".\\{}\\", self.options.output_dir))?;
        write_tag(&mut w, "OutputName", &self.options.output_name)?;
        write_tag(&mut w, "CreateExecutable", "1")?;
        write_tag(&mut w, "CreateHexFile", if self.options.create_hex { "1" } else { "0" })?;
        w.write_event(Event::End(BytesEnd::new("TargetCommonOption")))?;

        w.write_event(Event::Start(BytesStart::new("DebugOption")))?;
        w.write_event(Event::Start(BytesStart::new("Simulator")))?;
        write_tag(
            &mut w,
            "UseSimulator",
            match self.options.debug_mode {
                DebugBackendMode::Simulator => "1",
                DebugBackendMode::Hardware => "0",
            },
        )?;
        w.write_event(Event::End(BytesEnd::new("Simulator")))?;
        w.write_event(Event::End(BytesEnd::new("DebugOption")))?;

        w.write_event(Event::Start(BytesStart::new("TargetArmAds")))?;
        w.write_event(Event::Start(BytesStart::new("ArmAdsMisc")))?;
        write_tag(&mut w, "uAC6", "1")?;
        w.write_event(Event::Start(BytesStart::new("OnChipMemories")))?;
        w.write_event(Event::Start(BytesStart::new("IROM1")))?;
        write_tag(&mut w, "StartAddress", &format!("0x{:x}", self.options.ro_base))?;
        write_tag(&mut w, "Size", &format!("0x{:x}", self.options.ro_size))?;
        w.write_event(Event::End(BytesEnd::new("IROM1")))?;
        w.write_event(Event::Start(BytesStart::new("IRAM1")))?;
        write_tag(&mut w, "StartAddress", &format!("0x{:x}", self.options.rw_base))?;
        write_tag(&mut w, "Size", &format!("0x{:x}", self.options.rw_size))?;
        w.write_event(Event::End(BytesEnd::new("IRAM1")))?;
        w.write_event(Event::End(BytesEnd::new("OnChipMemories")))?;
        w.write_event(Event::End(BytesEnd::new("ArmAdsMisc")))?;

        w.write_event(Event::Start(BytesStart::new("Cads")))?;
        w.write_event(Event::Start(BytesStart::new("VariousControls")))?;
        write_tag(&mut w, "MiscControls", &self.options.c_misc)?;
        write_tag(&mut w, "Define", &self.options.c_defines.join(","))?;
        write_tag(&mut w, "IncludePath", &self.options.c_includes.join(";"))?;
        w.write_event(Event::End(BytesEnd::new("VariousControls")))?;
        w.write_event(Event::End(BytesEnd::new("Cads")))?;

        w.write_event(Event::Start(BytesStart::new("Aads")))?;
        write_tag(
            &mut w,
            "ClangAsOpt",
            match self.options.asm_syntax {
                AsmSyntax::ArmAsm => "1",
                AsmSyntax::GnuArmClang => "3",
            },
        )?;
        w.write_event(Event::Start(BytesStart::new("VariousControls")))?;
        write_tag(&mut w, "MiscControls", &self.options.a_misc)?;
        write_tag(&mut w, "Define", &self.options.a_defines.join(","))?;
        write_tag(&mut w, "IncludePath", &self.options.a_includes.join(";"))?;
        w.write_event(Event::End(BytesEnd::new("VariousControls")))?;
        w.write_event(Event::End(BytesEnd::new("Aads")))?;

        w.write_event(Event::Start(BytesStart::new("LDads")))?;
        write_tag(&mut w, "ScatterFile", self.options.scatter_file.as_deref().unwrap_or(""))?;
        write_tag(&mut w, "Misc", &self.options.ld_misc)?;
        w.write_event(Event::End(BytesEnd::new("LDads")))?;

        w.write_event(Event::End(BytesEnd::new("TargetArmAds")))?;
        w.write_event(Event::End(BytesEnd::new("TargetOption")))?;

        w.write_event(Event::Start(BytesStart::new("Groups")))?;
        for g in &self.groups {
            w.write_event(Event::Start(BytesStart::new("Group")))?;
            write_tag(&mut w, "GroupName", &g.name)?;
            w.write_event(Event::Start(BytesStart::new("Files")))?;
            for f in &g.files {
                w.write_event(Event::Start(BytesStart::new("File")))?;
                write_tag(&mut w, "FileName", &f.name)?;
                write_tag(&mut w, "FileType", &f.file_type.to_string())?;
                write_tag(&mut w, "FilePath", &f.path)?;
                w.write_event(Event::End(BytesEnd::new("File")))?;
            }
            w.write_event(Event::End(BytesEnd::new("Files")))?;
            w.write_event(Event::End(BytesEnd::new("Group")))?;
        }
        w.write_event(Event::End(BytesEnd::new("Groups")))?;

        w.write_event(Event::End(BytesEnd::new("Target")))?;
        w.write_event(Event::End(BytesEnd::new("Targets")))?;
        w.write_event(Event::End(BytesEnd::new("Project")))?;

        Ok(String::from_utf8(w.into_inner().into_inner())?)
    }
}

fn write_tag(w: &mut Writer<Cursor<Vec<u8>>>, tag: &str, text: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))?;
    if !text.is_empty() {
        w.write_event(Event::Text(BytesText::new(text)))?;
    }
    w.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn find_uvprojx_in_dir(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("uvprojx") {
            return Some(p);
        }
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_rel(root: &Path, rel: &str) -> PathBuf {
    let norm = rel.replace('\\', "/");
    let p = PathBuf::from(&norm);
    if p.is_absolute() {
        p
    } else {
        clean_canon(&root.join(p))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn clean_canon(p: &Path) -> PathBuf {
    let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = c.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        c
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn make_rel_path(root: &Path, abs: &Path) -> String {
    if let Ok(rel) = abs.strip_prefix(root) {
        format!(".\\{}", rel.to_string_lossy().replace('/', "\\"))
    } else {
        abs.to_string_lossy().into_owned()
    }
}

fn norm_slashes(p: &str) -> String {
    p.replace('\\', "/")
}

fn normalize_virtual_segments(path: &str) -> String {
    let clean = norm_slashes(path);
    let leading_slash = clean.starts_with('/');
    let mut stack: Vec<&str> = Vec::new();
    for seg in clean.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if !stack.is_empty() && *stack.last().unwrap() != ".." {
                stack.pop();
            } else if !leading_slash {
                stack.push("..");
            }
        } else {
            stack.push(seg);
        }
    }
    if leading_slash {
        format!("/{}", stack.join("/"))
    } else {
        stack.join("/")
    }
}

fn is_virtual_abs(p: &str) -> bool {
    let s = p.replace('\\', "/");
    s.starts_with('/') || (s.len() >= 2 && s.as_bytes()[1] == b':')
}

/// Result of [`ProjectSpec::add_virtual_file`]: the file entry, the created template body
/// (if any) and a replaced `(old, new)` path pair (if any).
pub type VirtualFileAdded = (ProjectFile, Option<String>, Option<(String, String)>);

pub fn resolve_virtual_path(root: &str, rel: &str) -> String {
    let r_clean = norm_slashes(rel);
    if is_virtual_abs(&r_clean) {
        normalize_virtual_segments(&r_clean)
    } else {
        let base = norm_slashes(root);
        let joined = if base.ends_with('/') {
            format!("{}{}", base, r_clean)
        } else {
            format!("{}/{}", base, r_clean)
        };
        normalize_virtual_segments(&joined)
    }
}

fn make_virtual_rel(root: &str, abs: &str) -> String {
    let r = normalize_virtual_segments(root);
    let a = normalize_virtual_segments(abs);
    let prefix = format!("{}/", r.trim_end_matches('/'));
    if let Some(rest) = a.strip_prefix(&prefix) {
        format!(".\\{}", rest.replace('/', "\\"))
    } else {
        a
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split([',', ' ', ';'])
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn split_paths(s: &str) -> Vec<String> {
    s.split(';')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<u32>().ok()
    }
}

pub const DEFAULT_ASM_TEMPLATE: &str = r#";;; Directives
    PRESERVE8
    THUMB

; Vector Table Mapped to Address 0 at Reset
    AREA    RESET, DATA, READONLY
    EXPORT  __Vectors

__Vectors
    DCD     0x20001000      ; Initial Stack Pointer
    DCD     Reset_Handler   ; Reset Vector
    ALIGN

; Code Section
    AREA    MYCODE, CODE, READONLY
    ENTRY
    EXPORT  Reset_Handler

Reset_Handler
    MOV     R0, #12
    ADD     R0, R0, #4
    MOV     R2, #0x20000000
    STR     R0, [R2, #0]
STOP
    B       STOP

    END
"#;

pub const DEFAULT_C_TEMPLATE: &str = r#"#include <stdint.h>

#define RCC_APB2ENR   (*(volatile uint32_t *)0x40021018U)
#define GPIOA_CRL     (*(volatile uint32_t *)0x40010800U)
#define GPIOA_ODR     (*(volatile uint32_t *)0x4001080CU)

void Reset_Handler(void);

__attribute__((section("RESET"), used))
const uint32_t __Vectors[] = {
    0x20001000U,
    (uint32_t)Reset_Handler
};

int main(void) {
    RCC_APB2ENR |= (1U << 2);
    GPIOA_CRL = (GPIOA_CRL & ~0xFU) | 0x2U;
    GPIOA_ODR ^= 1U;
    while (1) {
    }
}

void Reset_Handler(void) {
    main();
    while (1) {}
}
"#;
