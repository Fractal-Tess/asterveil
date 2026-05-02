use std::process::{Command, Stdio};

use crate::prelude::*;

#[derive(Clone, Debug)]
pub struct GpuState {
    pub index: u32,
    pub name: String,
    pub temperature_c: String,
    pub gpu_utilization_pct: String,
    pub graphics_clock_mhz: String,
    pub fan_speed_pct: String,
    pub memory_utilization_pct: String,
    pub memory_used_mib: String,
    pub memory_total_mib: String,
    pub draw_w: String,
    pub limit_w: String,
    pub default_w: String,
    pub min_w: String,
    pub max_w: String,
    pub fan_control_state: Option<String>,
}

pub fn run_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("failed to run {cmd}"))?;

    if !output.status.success() {
        bail!("{cmd} exited with status {}", output.status);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run_sudo_status(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new("sudo")
        .arg("-n")
        .arg(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to run sudo {cmd}"))?;

    if !status.success() {
        bail!("sudo {cmd} exited with status {status}");
    }

    Ok(())
}

pub fn query_gpus(display: &str) -> Result<Vec<GpuState>> {
    let output = run_capture(
        "nvidia-smi",
        &[
            "--query-gpu=index,name,temperature.gpu,utilization.gpu,clocks.current.graphics,fan.speed,utilization.memory,memory.used,memory.total,power.draw,power.limit,power.default_limit,power.min_limit,power.max_limit",
            "--format=csv,noheader,nounits",
        ],
    )?;

    parse_gpu_rows(&output, display)
}

pub fn query_gpu_fan_state(display: &str, index: u32) -> Result<String> {
    run_capture(
        "nvidia-settings",
        &[
            "-c",
            display,
            "-q",
            &format!("[gpu:{index}]/GPUFanControlState"),
            "-t",
        ],
    )
}

pub fn set_power_limit(index: u32, watts: &str) -> Result<()> {
    run_sudo_status("nvidia-smi", &["-i", &index.to_string(), "-pl", watts])
}

pub fn set_manual_fan_control(gpu_indices: &[usize], enabled: bool) -> Result<()> {
    let display = display_from_env();
    let gpus = run_capture(
        "nvidia-smi",
        &["--query-gpu=index", "--format=csv,noheader,nounits"],
    )?;

    let state = if enabled { "1" } else { "0" };
    let mut args: Vec<String> = vec!["-c".to_string(), display.clone()];
    for line in gpus.lines() {
        let index = line.trim();
        if index.is_empty() {
            continue;
        }

        let parsed_index = match index.parse::<usize>() {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !gpu_indices.contains(&parsed_index) {
            continue;
        }

        args.push("-a".to_string());
        args.push(format!("[gpu:{index}]/GPUFanControlState={state}"));
    }

    run_sudo_nvidia_settings(&display, args)
}

pub fn set_all_fans(speed: u32) -> Result<()> {
    let display = display_from_env();
    let fans = run_capture("nvidia-settings", &["-c", &display, "-q", "fans"])?;
    let mut args: Vec<String> = vec!["-c".to_string(), display.clone()];
    let mut found = false;

    for line in fans.lines() {
        if let Some(index) = extract_fan_index(line) {
            found = true;
            args.push("-a".to_string());
            args.push(format!("[fan:{index}]/GPUTargetFanSpeed={speed}"));
        }
    }

    if !found {
        bail!("no NVIDIA fans found");
    }

    run_sudo_nvidia_settings(&display, args)
}

fn run_sudo_nvidia_settings(display: &str, args: Vec<String>) -> Result<()> {
    let status = Command::new("sudo")
        .arg("-n")
        .arg("nvidia-settings")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("DISPLAY", display)
        .status()
        .context("failed to run sudo nvidia-settings")?;

    if !status.success() {
        bail!("sudo nvidia-settings exited with status {status}");
    }

    Ok(())
}

pub fn parse_gpu_rows(output: &str, display: &str) -> Result<Vec<GpuState>> {
    let mut gpus = Vec::new();
    for line in output.lines() {
        let parts: Vec<_> = line.split(',').map(|part| part.trim()).collect();
        if parts.len() < 14 {
            continue;
        }

        let index = parts[0].parse::<u32>().context("invalid GPU index")?;
        gpus.push(GpuState {
            index,
            name: parts[1].to_string(),
            temperature_c: parts[2].to_string(),
            gpu_utilization_pct: parts[3].to_string(),
            graphics_clock_mhz: parts[4].to_string(),
            fan_speed_pct: parts[5].to_string(),
            memory_utilization_pct: parts[6].to_string(),
            memory_used_mib: parts[7].to_string(),
            memory_total_mib: parts[8].to_string(),
            draw_w: parts[9].to_string(),
            limit_w: parts[10].to_string(),
            default_w: parts[11].to_string(),
            min_w: parts[12].to_string(),
            max_w: parts[13].to_string(),
            fan_control_state: query_gpu_fan_state(display, index).ok(),
        });
    }

    Ok(gpus)
}

pub fn extract_fan_index(line: &str) -> Option<u32> {
    let start = line.find("[fan:")?;
    let rest = &line[start + 5..];
    let end = rest.find(']')?;
    rest[..end].parse::<u32>().ok()
}

pub fn resolve_power_value(value: &str, default_w: &str, min_w: &str, max_w: &str) -> Result<String> {
    let lower = value.trim().to_ascii_lowercase();
    let resolved = match lower.as_str() {
        "min" | "eco" => min_w.to_string(),
        "balanced" => {
            let min = min_w.parse::<f64>()?;
            let max = max_w.parse::<f64>()?;
            format!("{:.0}", (min + max) / 2.0)
        }
        "default" => default_w.to_string(),
        "max" | "performance" => max_w.to_string(),
        _ => {
            let watts = value.parse::<f64>().with_context(
                || "power value must be watts or one of min/eco/balanced/default/max/performance",
            )?;
            let min = min_w.parse::<f64>()?;
            let max = max_w.parse::<f64>()?;
            format!("{:.0}", clamp_watts(watts, min, max))
        }
    };

    Ok(resolved)
}

pub fn clamp_watts(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

pub fn format_vram_summary(used_mib: &str, total_mib: &str) -> String {
    let used = used_mib.parse::<f64>().ok();
    let total = total_mib.parse::<f64>().ok();

    match (used, total) {
        (Some(used), Some(total)) if total > 0.0 => {
            format!(
                "{:.1}/{:.1} GiB ({:.0}%)",
                used / 1024.0,
                total / 1024.0,
                (used / total) * 100.0
            )
        }
        _ => format!("{used_mib}/{total_mib} MiB"),
    }
}

pub fn format_percent(value: &str) -> String {
    if value.eq_ignore_ascii_case("n/a") {
        "N/A".to_string()
    } else {
        format!("{value}%")
    }
}

pub fn percent_ratio(value: &str) -> f64 {
    value
        .parse::<f64>()
        .map(|percent| (percent / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0)
}

pub fn display_from_env() -> String {
    std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string())
}

pub fn is_auto_fan_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "default" | "auto"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_power_presets() {
        assert_eq!(
            resolve_power_value("eco", "350", "100", "500").unwrap(),
            "100"
        );
        assert_eq!(
            resolve_power_value("balanced", "350", "100", "500").unwrap(),
            "300"
        );
        assert_eq!(
            resolve_power_value("default", "350", "100", "500").unwrap(),
            "350"
        );
        assert_eq!(
            resolve_power_value("max", "350", "100", "500").unwrap(),
            "500"
        );
    }

    #[test]
    fn clamps_numeric_power_values() {
        assert_eq!(
            resolve_power_value("50", "350", "100", "500").unwrap(),
            "100"
        );
        assert_eq!(
            resolve_power_value("600", "350", "100", "500").unwrap(),
            "500"
        );
    }

    #[test]
    fn extracts_fan_indices() {
        assert_eq!(extract_fan_index("    [0] [fan:0] (Fan 0)"), Some(0));
        assert_eq!(extract_fan_index("    [1] [fan:12] (Fan 12)"), Some(12));
        assert_eq!(extract_fan_index("no fan here"), None);
    }

    #[test]
    fn parses_gpu_rows() {
        let output = "\
0, NVIDIA GeForce RTX 3090, 68, 91, 1890, 62, 54, 18432, 24576, 275.00 W, 350.00 W, 350.00 W, 100.00 W, 500.00 W\n\
1, NVIDIA GeForce RTX 3090, 59, 72, 1710, 48, 43, 12288, 24576, 210.00 W, 350.00 W, 350.00 W, 100.00 W, 500.00 W\n";
        let gpus = parse_gpu_rows(output, ":0").unwrap();
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].index, 0);
        assert_eq!(gpus[1].index, 1);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 3090");
        assert_eq!(gpus[0].temperature_c, "68");
        assert_eq!(gpus[0].gpu_utilization_pct, "91");
        assert_eq!(gpus[0].graphics_clock_mhz, "1890");
        assert_eq!(gpus[0].fan_speed_pct, "62");
        assert_eq!(gpus[0].memory_used_mib, "18432");
    }

    #[test]
    fn handles_fan_default_values() {
        assert!(is_auto_fan_value("default"));
        assert!(is_auto_fan_value("auto"));
        assert!(!is_auto_fan_value("100"));
    }

    #[test]
    fn formats_vram_summary_in_gib() {
        assert_eq!(format_vram_summary("18432", "24576"), "18.0/24.0 GiB (75%)");
    }
}
