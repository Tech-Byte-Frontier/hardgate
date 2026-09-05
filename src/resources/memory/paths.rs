use super::{invalid_data, parse_decimal};
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub(super) struct CgroupMount {
    pub(super) root: PathBuf,
    pub(super) mount_point: PathBuf,
}

pub(super) fn parse_cgroup_path(input: &str) -> io::Result<Option<Vec<OsString>>> {
    let mut cgroup = None;
    let mut saw_line = false;
    for line in input.lines() {
        if line.is_empty() {
            continue;
        }
        saw_line = true;
        let mut fields = line.splitn(3, ':');
        let hierarchy = fields
            .next()
            .ok_or_else(|| invalid_data("malformed /proc/self/cgroup line"))?;
        let controllers = fields
            .next()
            .ok_or_else(|| invalid_data("malformed /proc/self/cgroup line"))?;
        let path = fields
            .next()
            .ok_or_else(|| invalid_data("malformed /proc/self/cgroup line"))?;
        if hierarchy == "0" {
            if !controllers.is_empty() || cgroup.is_some() {
                return Err(invalid_data("malformed unified cgroup membership"));
            }
            cgroup = Some(safe_path_components(path, "cgroup path")?);
        } else {
            parse_decimal(hierarchy, "cgroup hierarchy")?;
            if !path.starts_with('/') {
                return Err(invalid_data("malformed cgroup v1 path"));
            }
        }
    }
    if !saw_line {
        return Err(invalid_data("/proc/self/cgroup is empty"));
    }
    Ok(cgroup)
}

pub(super) fn find_cgroup2_mounts(input: &str) -> io::Result<Vec<CgroupMount>> {
    let mut mounts = Vec::new();
    for line in input.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            continue;
        };
        if separator < 6 || fields.len() <= separator + 1 {
            return Err(invalid_data("malformed /proc/self/mountinfo line"));
        }
        if fields[separator + 1] != "cgroup2" {
            continue;
        }
        let root = decode_mountinfo_path(fields[3])?;
        let mount_point = decode_mountinfo_path(fields[4])?;
        if !root.is_absolute() || !mount_point.is_absolute() {
            return Err(invalid_data("cgroup2 mount paths must be absolute"));
        }
        safe_absolute_components(&root, "cgroup2 mount root")?;
        safe_absolute_components(&mount_point, "cgroup2 mount point")?;
        mounts.push(CgroupMount { root, mount_point });
    }
    if mounts.is_empty() {
        return Err(invalid_data("unified cgroup2 mount is unavailable"));
    }
    Ok(mounts)
}

pub(super) fn cgroup_directories(
    mount: &CgroupMount,
    cgroup_path: &[OsString],
) -> io::Result<Vec<PathBuf>> {
    let mount_components = safe_absolute_components(&mount.mount_point, "cgroup mount point")?;
    let root_components = safe_absolute_components(&mount.root, "cgroup mount root")?;
    let relative = if cgroup_path.starts_with(&root_components) {
        &cgroup_path[root_components.len()..]
    } else {
        cgroup_path
    };
    let mut all_components = mount_components.clone();
    all_components.extend(relative.iter().cloned());
    let mut current = absolute_path(&all_components);
    let mount_path = absolute_path(&mount_components);
    let mut directories = vec![current.clone()];
    while current != mount_path {
        if !current.pop() || !current.starts_with(&mount_path) {
            return Err(invalid_data("cgroup path escaped its mount"));
        }
        directories.push(current.clone());
    }
    Ok(directories)
}

pub(super) fn mount_root_matches(mount: &CgroupMount, cgroup_path: &[OsString]) -> bool {
    safe_absolute_components(&mount.root, "cgroup mount root")
        .map(|root| root.is_empty() || cgroup_path.starts_with(&root))
        .unwrap_or(false)
}

fn decode_mountinfo_path(value: &str) -> io::Result<PathBuf> {
    let mut decoded = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        let Some(first) = chars.next() else {
            return Err(invalid_data("invalid escape in mountinfo path"));
        };
        let Some(second) = chars.next() else {
            return Err(invalid_data("invalid escape in mountinfo path"));
        };
        let Some(third) = chars.next() else {
            return Err(invalid_data("invalid escape in mountinfo path"));
        };
        if !matches!(first, '0'..='7')
            || !matches!(second, '0'..='7')
            || !matches!(third, '0'..='7')
        {
            return Err(invalid_data("invalid escape in mountinfo path"));
        }
        let byte =
            ((first as u8 - b'0') << 6) | ((second as u8 - b'0') << 3) | (third as u8 - b'0');
        if byte == 0 {
            return Err(invalid_data("NUL in mountinfo path"));
        }
        decoded.push(byte as char);
    }
    Ok(PathBuf::from(decoded))
}

fn safe_path_components(value: &str, label: &str) -> io::Result<Vec<OsString>> {
    if !value.starts_with('/') || value.contains('\0') {
        return Err(invalid_data(format!("{label} must be absolute")));
    }
    safe_absolute_components(Path::new(value), label)
}

fn safe_absolute_components(path: &Path, label: &str) -> io::Result<Vec<OsString>> {
    if !path.is_absolute() {
        return Err(invalid_data(format!("{label} must be absolute")));
    }
    let mut components = Vec::new();
    for component in path.components() {
        if let Some(component) = safe_component(component, label)? {
            components.push(component);
        }
    }
    Ok(components)
}

fn safe_component(component: Component<'_>, label: &str) -> io::Result<Option<OsString>> {
    match component {
        Component::RootDir => Ok(None),
        Component::Normal(value) if !value.is_empty() && value != "." && value != ".." => {
            Ok(Some(value.to_owned()))
        }
        Component::Normal(_) | Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
            Err(invalid_data(format!("{label} contains traversal")))
        }
    }
}

fn absolute_path(components: &[OsString]) -> PathBuf {
    let mut path = PathBuf::from("/");
    for component in components {
        path.push(component);
    }
    path
}
