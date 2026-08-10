use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

const CREATED_BY_TAG: &str = "X-AppImage-CreatedBy=autoAppImage";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    let downloads = home.join("Downloads");
    let applications_dir = home.join("Applications");
    let icons_dir = applications_dir.join(".icons");
    let shortcuts_dir = home.join(".local/share/applications");

    fs::create_dir_all(&applications_dir)?;
    fs::create_dir_all(&icons_dir)?;
    fs::create_dir_all(&shortcuts_dir)?;

    cleanup_orphaned_shortcuts(&shortcuts_dir)?;

    move_appimages_from_downloads(&downloads, &applications_dir)?;

    let registered_apps = get_registered_app_paths(&shortcuts_dir)?;

    if applications_dir.exists() && applications_dir.is_dir() {
        for entry in fs::read_dir(&applications_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("AppImage")) {
                ensure_executable(&path)?;

                if registered_apps.contains(&path) {
                    continue;
                }

                println!("⚡ [autoAppImage] New app: {:?}", path.file_name().unwrap());
                if let Err(e) = create_shortcut_for_app(&path, &icons_dir, &shortcuts_dir) {
                    eprintln!("⚠️ Failed to create shortcut for {:?}: {}", path.file_name().unwrap(), e);
                }
            }
        }
    }

    Ok(())
}

fn move_appimages_from_downloads(downloads: &Path, applications_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !downloads.exists() || !downloads.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(downloads)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("AppImage")) {
            if let Some(file_name) = path.file_name() {
                let dest_path = applications_dir.join(file_name);
                if !dest_path.exists() {
                    println!("🚚 [autoAppImage] Moving from Downloads: {:?}", file_name);
                    fs::rename(&path, &dest_path)?;
                }
            }
        }
    }
    Ok(())
}

fn ensure_executable(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut perms = fs::metadata(path)?.permissions();
    if perms.mode() & 0o111 == 0 {
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn get_registered_app_paths(shortcuts_dir: &Path) -> Result<HashSet<PathBuf>, Box<dyn std::error::Error>> {
    let mut registered = HashSet::new();
    if !shortcuts_dir.exists() {
        return Ok(registered);
    }

    for entry in fs::read_dir(shortcuts_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().map_or(false, |ext| ext == "desktop") {
            if let Ok(content) = fs::read_to_string(&path) {
                if content.lines().any(|l| l.trim() == CREATED_BY_TAG) {
                    if let Some(target_path) = extract_exec_path(&content) {
                        registered.insert(target_path);
                    }
                }
            }
        }
    }

    Ok(registered)
}

fn create_shortcut_for_app(
    app_path: &Path,
    icons_dir: &Path,
    shortcuts_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let app_stem = app_path.file_stem().unwrap().to_string_lossy().to_string();
    let temp_extract_path = std::env::temp_dir().join(format!("auto_appimage_extract_{}", app_stem));

    if temp_extract_path.exists() {
        let _ = fs::remove_dir_all(&temp_extract_path);
    }
    fs::create_dir_all(&temp_extract_path)?;

    let status = Command::new(app_path)
        .arg("--appimage-extract")
        .current_dir(&temp_extract_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if let Err(e) = status {
        eprintln!("⚠️ Failed to extract {:?}: {}", app_path.file_name().unwrap(), e);
        let _ = fs::remove_dir_all(&temp_extract_path);
        return Ok(());
    }

    let squashfs_root = temp_extract_path.join("squashfs-root");

    if squashfs_root.exists() {
        let mut embedded_desktop_content = None;
        for sub_entry in WalkDir::new(&squashfs_root).into_iter().filter_map(|e| e.ok()) {
            if sub_entry.path().is_file() && sub_entry.path().extension().map_or(false, |ext| ext == "desktop") {
                if let Ok(content) = fs::read_to_string(sub_entry.path()) {
                    embedded_desktop_content = Some(content);
                    break;
                }
            }
        }

        if let Some(content) = embedded_desktop_content {
            let mut new_lines = Vec::new();
            let mut app_real_name = app_stem.clone();
            let mut raw_icon_name = None;

            for line in content.lines() {
                if line.starts_with("Exec=") {
                    let args = line.trim_start_matches("Exec=").split_whitespace().skip(1).collect::<Vec<&str>>().join(" ");
                    new_lines.push(format!("Exec=\"{}\" {}", app_path.display(), args));
                } else if line.starts_with("Name=") {
                    app_real_name = line.trim_start_matches("Name=").to_string();
                    new_lines.push(line.to_string());
                } else if line.starts_with("Icon=") {
                    raw_icon_name = Some(line.trim_start_matches("Icon=").trim().to_string());
                } else if !line.starts_with("X-AppImage") {
                    new_lines.push(line.to_string());
                }
            }

            let saved_icon_path = find_and_save_icon(&squashfs_root, icons_dir, &app_stem, raw_icon_name.as_deref());

            if let Some(ref icon_p) = saved_icon_path {
                new_lines.push(format!("Icon={}", icon_p.display()));
            } else {
                new_lines.push("Icon=application-x-executable".to_string());
            }

            new_lines.push(CREATED_BY_TAG.to_string());

            let system_id = app_real_name.to_lowercase().replace(" ", "_");
            let final_shortcut_path = shortcuts_dir.join(format!("appimage-{}.desktop", system_id));

            fs::write(&final_shortcut_path, new_lines.join("\n"))?;
            println!("✅ [autoAppImage] Shortcut created: {:?}", final_shortcut_path.file_name().unwrap());
        }
    }

    if temp_extract_path.starts_with(std::env::temp_dir()) {
        let _ = fs::remove_dir_all(&temp_extract_path);
    }

    Ok(())
}

fn icon_quality_score(path: &Path) -> u32 {
    let is_svg = path.extension().map_or(false, |e| e.eq_ignore_ascii_case("svg"));

    let path_str = path.to_string_lossy().to_lowercase();
    let is_scalable_dir = path_str.split('/').any(|part| part == "scalable");

    if is_svg || is_scalable_dir {
        return u32::MAX;
    }

    for part in path.parent().map(|p| p.to_path_buf()).unwrap_or_default().iter() {
        let part_str = part.to_string_lossy();
        if let Some((w, h)) = part_str.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                return w.min(h);
            }
        }
    }

    0
}

fn find_and_save_icon(squashfs_root: &Path, icons_dir: &Path, app_stem: &str, raw_icon_name: Option<&str>) -> Option<PathBuf> {
    if let Some(icon_name) = raw_icon_name {
        let mut best_candidate: Option<(PathBuf, u32)> = None;

        for entry in WalkDir::new(squashfs_root).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.eq_ignore_ascii_case(icon_name) {
                    let score = icon_quality_score(path);
                    let is_better = match &best_candidate {
                        Some((_, best_score)) => score > *best_score,
                        None => true,
                    };
                    if is_better {
                        best_candidate = Some((path.to_path_buf(), score));
                    }
                }
            }
        }

        if let Some((best_path, _)) = best_candidate {
            let ext = best_path.extension().and_then(|e| e.to_str()).unwrap_or("png");
            let dest = icons_dir.join(format!("{}.{}", app_stem, ext));
            if fs::copy(&best_path, &dest).is_ok() {
                return Some(dest);
            }
        }
    }

    let dir_icon = squashfs_root.join(".DirIcon");
    if dir_icon.exists() {
        let real_path = fs::canonicalize(&dir_icon).unwrap_or(dir_icon);
        let ext = real_path.extension().and_then(|e| e.to_str()).unwrap_or("png");
        let dest = icons_dir.join(format!("{}.{}", app_stem, ext));

        if fs::copy(&real_path, &dest).is_ok() {
            return Some(dest);
        }
    }

    None
}

fn cleanup_orphaned_shortcuts(shortcuts_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !shortcuts_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(shortcuts_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().map_or(false, |ext| ext == "desktop") {
            if let Ok(content) = fs::read_to_string(&path) {
                if content.lines().any(|l| l.trim() == CREATED_BY_TAG) {
                    if let Some(target_path) = extract_exec_path(&content) {
                        if !target_path.exists() {
                            if let Some(icon_path) = extract_icon_path(&content) {
                                if icon_path.exists() {
                                    let _ = fs::remove_file(&icon_path);
                                    println!("🗑️ [autoAppImage] Icon removed: {:?}", icon_path.file_name().unwrap());
                                }
                            }
                            println!("🗑️ [autoAppImage] Shortcut removed: {:?}", path.file_name().unwrap());
                            fs::remove_file(&path)?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn extract_exec_path(desktop_content: &str) -> Option<PathBuf> {
    for line in desktop_content.lines() {
        if line.trim().starts_with("Exec=") {
            let raw_exec = line.trim().trim_start_matches("Exec=").trim();

            if let Some(rest) = raw_exec.strip_prefix('"') {
                if let Some(end) = rest.find('"') {
                    return Some(PathBuf::from(&rest[..end]));
                }
            } else if let Some(rest) = raw_exec.strip_prefix('\'') {
                if let Some(end) = rest.find('\'') {
                    return Some(PathBuf::from(&rest[..end]));
                }
            }

            if let Some(first_arg) = raw_exec.split_whitespace().next() {
                return Some(PathBuf::from(first_arg));
            }
        }
    }
    None
}

fn extract_icon_path(desktop_content: &str) -> Option<PathBuf> {
    for line in desktop_content.lines() {
        if line.trim().starts_with("Icon=") {
            let icon_val = line.trim().trim_start_matches("Icon=").trim();
            if icon_val.starts_with('/') {
                return Some(PathBuf::from(icon_val));
            }
        }
    }
    None
}