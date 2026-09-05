use super::{invalid_data, parse_decimal};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Pressure {
    pub(super) full_avg10: f64,
    pub(super) some_avg10: f64,
}

pub(super) fn read_required(path: &Path) -> io::Result<String> {
    fs::read_to_string(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "workload resource guard: failed to read {}: {error}",
                path.display()
            ),
        )
    })
}

pub(super) fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!(
                "workload resource guard: failed to read {}: {error}",
                path.display()
            ),
        )),
    }
}

pub(super) fn directory_exists(path: &Path) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!(
                "workload resource guard: failed to inspect {}: {error}",
                path.display()
            ),
        )),
    }
}

pub(super) fn parse_meminfo(input: &str) -> io::Result<(u64, u64)> {
    let mut total = None;
    let mut available = None;
    for line in input.lines() {
        parse_meminfo_line(line, &mut total, &mut available)?;
    }

    let total = total.ok_or_else(|| invalid_data("MemTotal is missing from /proc/meminfo"))?;
    let available =
        available.ok_or_else(|| invalid_data("MemAvailable is missing from /proc/meminfo"))?;
    if available > total {
        return Err(invalid_data("MemAvailable exceeds MemTotal"));
    }
    Ok((total, available))
}

fn parse_meminfo_line(
    line: &str,
    total: &mut Option<u64>,
    available: &mut Option<u64>,
) -> io::Result<()> {
    let Some((name, value)) = line.split_once(':') else {
        return Ok(());
    };
    match name {
        "MemTotal" => parse_meminfo_value(name, value, total),
        "MemAvailable" => parse_meminfo_value(name, value, available),
        _ => Ok(()),
    }
}

fn parse_meminfo_value(name: &str, value: &str, slot: &mut Option<u64>) -> io::Result<()> {
    if slot.is_some() {
        return Err(invalid_data(format!("duplicate {name} in /proc/meminfo")));
    }
    let mut fields = value.split_whitespace();
    let amount = parse_decimal(
        fields
            .next()
            .ok_or_else(|| invalid_data(format!("missing value for {name}")))?,
        name,
    )?;
    if fields.next() != Some("kB") || fields.next().is_some() {
        return Err(invalid_data(format!("invalid unit for {name}")));
    }
    let bytes = amount
        .checked_mul(1024)
        .ok_or_else(|| invalid_data(format!("{name} overflows bytes")))?;
    *slot = Some(bytes);
    Ok(())
}

pub(super) fn parse_pressure(input: &str) -> io::Result<Pressure> {
    let mut full = None;
    let mut some = None;
    for line in input.lines() {
        if let Some((kind, avg10)) = parse_pressure_line(line)? {
            record_pressure(kind, avg10, &mut full, &mut some)?;
        }
    }
    Ok(Pressure {
        full_avg10: required_pressure(full, "full")?,
        some_avg10: required_pressure(some, "some")?,
    })
}

fn parse_pressure_line(line: &str) -> io::Result<Option<(&str, f64)>> {
    let mut fields = line.split_whitespace();
    let Some(kind) = fields.next() else {
        return Ok(None);
    };
    if kind != "full" && kind != "some" {
        return Err(invalid_data("unknown memory PSI line"));
    }
    let mut avg10 = None;
    for field in fields {
        parse_pressure_field(kind, field, &mut avg10)?;
    }
    let avg10 = avg10.ok_or_else(|| invalid_data(format!("missing {kind} PSI avg10")))?;
    Ok(Some((kind, avg10)))
}

fn parse_pressure_field(kind: &str, field: &str, avg10: &mut Option<f64>) -> io::Result<()> {
    let Some(value) = field.strip_prefix("avg10=") else {
        return Ok(());
    };
    if avg10.is_some() {
        return Err(invalid_data(format!("duplicate avg10 in {kind} PSI line")));
    }
    let value = value
        .parse::<f64>()
        .map_err(|_| invalid_data(format!("invalid {kind} PSI avg10")))?;
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return Err(invalid_data(format!("invalid {kind} PSI avg10")));
    }
    *avg10 = Some(value);
    Ok(())
}

fn record_pressure(
    kind: &str,
    avg10: f64,
    full: &mut Option<f64>,
    some: &mut Option<f64>,
) -> io::Result<()> {
    let slot = if kind == "full" { full } else { some };
    if slot.replace(avg10).is_some() {
        return Err(invalid_data(format!("duplicate {kind} PSI line")));
    }
    Ok(())
}

fn required_pressure(value: Option<f64>, kind: &str) -> io::Result<f64> {
    value.ok_or_else(|| invalid_data(format!("{kind} PSI line is missing")))
}

pub(super) fn read_pressure(path: &Path) -> io::Result<Pressure> {
    match read_optional(path)? {
        Some(content) => parse_pressure(&content),
        None => Ok(Pressure::default()),
    }
}

#[cfg(test)]
#[path = "procfs_tests.rs"]
mod tests;
