use super::{DESCRIPTION, UNIT, resource_error};
use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};

pub(super) struct UnitStatus {
    fields: BTreeMap<String, String>,
}

impl UnitStatus {
    pub(super) fn parse(output: &str) -> io::Result<Self> {
        let mut fields = BTreeMap::new();
        for line in output.lines().filter(|line| !line.is_empty()) {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| resource_error("invalid systemd status evidence"))?;
            if fields.insert(key.to_string(), value.to_string()).is_some() {
                return Err(resource_error("duplicate systemd status evidence"));
            }
        }
        if !fields.contains_key("LoadState") {
            return Err(resource_error("missing systemd unit status"));
        }
        Ok(Self { fields })
    }

    fn value(&self, key: &str) -> &str {
        self.fields.get(key).map_or("", String::as_str)
    }

    pub(super) fn present(&self) -> bool {
        self.value("LoadState") != "not-found"
    }

    pub(super) fn running(&self) -> bool {
        self.present()
            && !matches!(self.value("ActiveState"), "inactive" | "failed")
            && self.value("SubState") != "exited"
    }

    pub(super) fn verify_owner(&self) -> io::Result<()> {
        if self.value("Transient") == "yes"
            && self
                .value("Description")
                .strip_prefix(DESCRIPTION)
                .is_some_and(|suffix| suffix.starts_with(' ') && suffix.len() > 1)
        {
            Ok(())
        } else {
            Err(resource_error(
                "the reserved mutation unit name belongs to another service; refusing to modify it",
            ))
        }
    }

    pub(super) fn verify_identity(
        &self,
        identity: &str,
        invocation: Option<&str>,
    ) -> io::Result<()> {
        self.verify_owner()?;
        if self.value("Description") != format!("{DESCRIPTION} {identity}")
            || invocation.is_some_and(|expected| self.value("InvocationID") != expected)
        {
            return Err(resource_error(
                "mutation service identity changed; refusing to modify it",
            ));
        }
        Ok(())
    }

    pub(super) fn invocation(&self) -> io::Result<Option<&str>> {
        let value = self.value("InvocationID");
        if value.is_empty() {
            return Ok(None);
        }
        if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(resource_error(
                "missing or invalid mutation service invocation",
            ));
        }
        Ok(Some(value))
    }

    pub(super) fn cgroup(&self) -> io::Result<Option<PathBuf>> {
        let value = self.value("ControlGroup");
        if value.is_empty() {
            return Ok(None);
        }
        let path = Path::new(value);
        if !path.is_absolute()
            || path.file_name().is_none_or(|name| name != UNIT)
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(resource_error("invalid mutation service cgroup path"));
        }
        Ok(Some(
            Path::new("/sys/fs/cgroup").join(path.strip_prefix("/").unwrap()),
        ))
    }
}
